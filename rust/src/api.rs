//! Работа с API Яндекс.Музыки: подпись запросов, скачивание и расшифровка.

use aes::cipher::{KeyIvInit, StreamCipher};
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::client::{ApiError, ApiResult, Client};
use crate::models;
use crate::models::{Album, Artist, ArtistAlbums, Id, Playlist, Track, TrackLyrics};

pub const DEFAULT_SIGN_KEY: &str = "p93jhgh689SBReK6ghtw62";

/// Порядок кодеков важен: он входит в подпись запроса.
pub const CODECS: [&str; 7] = [
    "flac",
    "flac-mp4",
    "mp3",
    "aac",
    "he-aac",
    "aac-mp4",
    "he-aac-mp4",
];

pub const PLAYLIST_FETCH_PAGE_SIZE: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiQuality {
    Low,
    Normal,
    Lossless,
}

impl ApiQuality {
    pub fn as_str(self) -> &'static str {
        match self {
            ApiQuality::Low => "lq",
            ApiQuality::Normal => "nq",
            ApiQuality::Lossless => "lossless",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Mp3,
    Flac,
    Mp4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Flac,
    Mp3,
    Aac,
}

impl Codec {
    pub fn name(self) -> &'static str {
        match self {
            Codec::Flac => "FLAC",
            Codec::Mp3 => "MP3",
            Codec::Aac => "AAC",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileFormat {
    pub container: Container,
    pub codec: Codec,
}

#[derive(Debug, Clone)]
pub struct CustomDownloadInfo {
    pub quality: String,
    pub file_format: FileFormat,
    pub urls: Vec<String>,
    pub decryption_key: Option<String>,
    pub bitrate: i64,
}

fn file_format_for_codec(codec: &str) -> Option<FileFormat> {
    let format = match codec {
        "flac" => FileFormat {
            container: Container::Flac,
            codec: Codec::Flac,
        },
        "flac-mp4" => FileFormat {
            container: Container::Mp4,
            codec: Codec::Flac,
        },
        "mp3" => FileFormat {
            container: Container::Mp3,
            codec: Codec::Mp3,
        },
        "aac" | "he-aac" | "aac-mp4" | "he-aac-mp4" => FileFormat {
            container: Container::Mp4,
            codec: Codec::Aac,
        },
        _ => return None,
    };
    Some(format)
}

fn hmac_sha256(key: &str, message: &str) -> Vec<u8> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("HMAC принимает любой ключ");
    mac.update(message.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// Подпись для `get-file-info` (Android-ключ, как в Python-версии).
pub fn file_info_sign(track_id: &str, quality: &str) -> (i64, String) {
    let timestamp = Utc::now().timestamp();
    (timestamp, file_info_sign_at(track_id, quality, timestamp))
}

pub fn file_info_sign_at(track_id: &str, quality: &str, timestamp: i64) -> String {
    let message = format!(
        "{timestamp}{track_id}{quality}{}{}",
        CODECS.concat(),
        "encraw"
    );
    let digest = hmac_sha256(DEFAULT_SIGN_KEY, &message);
    let encoded = base64::engine::general_purpose::STANDARD.encode(digest);
    // Python-версия отбрасывает последний символ (padding '=').
    encoded[..encoded.len().saturating_sub(1)].to_string()
}

/// Подпись для запроса текста песни.
pub fn lyrics_sign(track_id: &str) -> (i64, String) {
    let timestamp = Utc::now().timestamp();
    (timestamp, lyrics_sign_at(track_id, timestamp))
}

pub fn lyrics_sign_at(track_id: &str, timestamp: i64) -> String {
    let message = format!("{track_id}{timestamp}");
    let digest = hmac_sha256(DEFAULT_SIGN_KEY, &message);
    base64::engine::general_purpose::STANDARD.encode(digest)
}

/// AES-128/192/256-CTR с нулевым счётчиком (как pycryptodome с nonce=12 нулей).
pub fn decrypt_data(data: &[u8], key: &str) -> Result<Vec<u8>> {
    let key = hex::decode(key).context("Ключ расшифровки не является hex-строкой")?;
    let mut buffer = data.to_vec();
    let iv = [0u8; 16];

    match key.len() {
        16 => {
            let mut cipher = ctr::Ctr128BE::<aes::Aes128>::new(key.as_slice().into(), &iv.into());
            cipher.apply_keystream(&mut buffer);
        }
        24 => {
            let mut cipher = ctr::Ctr128BE::<aes::Aes192>::new(key.as_slice().into(), &iv.into());
            cipher.apply_keystream(&mut buffer);
        }
        32 => {
            let mut cipher = ctr::Ctr128BE::<aes::Aes256>::new(key.as_slice().into(), &iv.into());
            cipher.apply_keystream(&mut buffer);
        }
        other => bail!("Неподдерживаемая длина ключа расшифровки: {other} байт"),
    }

    Ok(buffer)
}

fn track_id(track: &Track) -> Result<&str> {
    track
        .id
        .as_deref()
        .ok_or_else(|| anyhow!("У трека отсутствует id"))
}

pub fn get_download_info(
    client: &Client,
    track: &Track,
    quality: ApiQuality,
) -> ApiResult<CustomDownloadInfo> {
    let track_id = track_id(track).map_err(|e| ApiError::Fatal(e.to_string()))?;
    let quality = quality.as_str();
    let (timestamp, sign) = file_info_sign(track_id, quality);

    let query = vec![
        ("ts", timestamp.to_string()),
        ("trackId", track_id.to_string()),
        ("quality", quality.to_string()),
        ("codecs", CODECS.join(",")),
        ("transports", "encraw".to_string()),
        ("sign", sign),
    ];

    let info: models::FileInfo = client.api_get_json("/get-file-info", &query)?;
    let download = info
        .download_info
        .ok_or_else(|| ApiError::Fatal("Сервер не вернул downloadInfo".to_string()))?;

    let raw_codec = download
        .codec
        .as_deref()
        .ok_or_else(|| ApiError::Fatal("Сервер не вернул кодек".to_string()))?;
    let file_format = file_format_for_codec(raw_codec)
        .ok_or_else(|| ApiError::Fatal(format!("Неизвестный кодек: {raw_codec}")))?;

    if download.urls.is_empty() {
        return Err(ApiError::Fatal(
            "Сервер не вернул ссылки на файл".to_string(),
        ));
    }

    Ok(CustomDownloadInfo {
        quality: download.quality.unwrap_or_else(|| quality.to_string()),
        file_format,
        urls: download.urls,
        decryption_key: download.key.filter(|key| !key.is_empty()),
        bitrate: download.bitrate.unwrap_or(0),
    })
}

pub fn download_track(client: &Client, info: &CustomDownloadInfo) -> ApiResult<Vec<u8>> {
    let url = &info.urls[rand::random::<usize>() % info.urls.len()];
    let data = client.fetch_bytes(url)?;
    match &info.decryption_key {
        Some(key) => decrypt_data(&data, key)
            .map_err(|error| ApiError::Fatal(format!("Ошибка расшифровки: {error:#}"))),
        None => Ok(data),
    }
}

pub fn account_status(client: &Client) -> ApiResult<()> {
    let _: serde_json::Value = client.api_get_json("/account/status", &[])?;
    Ok(())
}

pub fn artist_albums(
    client: &Client,
    artist_id: &str,
    page: u32,
) -> ApiResult<Option<ArtistAlbums>> {
    let path = format!("/artists/{artist_id}/direct-albums");
    let query = vec![
        ("sort-by", "year".to_string()),
        ("page", page.to_string()),
        ("page-size", "20".to_string()),
    ];
    client.api_get_json(&path, &query).map(Some)
}

pub fn album_with_tracks(client: &Client, album_id: &str) -> ApiResult<Option<Album>> {
    let path = format!("/albums/{album_id}/with-tracks");
    client.api_get_json(&path, &[]).map(Some)
}

pub fn artists(client: &Client, artist_id: &str) -> ApiResult<Vec<Artist>> {
    client.api_get_json("/artists", &[("artist-ids", artist_id.to_string())])
}

pub fn tracks(client: &Client, ids: &[Id]) -> ApiResult<Vec<Track>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    client.api_get_json(
        "/tracks",
        &[
            ("with-positions", "True".to_string()),
            ("track-ids", ids.join(",")),
        ],
    )
}

pub fn playlist(client: &Client, playlist_id: &str) -> ApiResult<Option<Playlist>> {
    // Поддерживаем и документированный формат `владелец/тип`, и новый `/playlist/{uuid}`.
    if let Some((owner, kind)) = playlist_id.split_once(['/', ':']) {
        let path = format!("/users/{owner}/playlists/{kind}");
        return client.api_get_json(&path, &[]).map(Some);
    }

    let path = format!("/playlist/{playlist_id}");
    client.api_get_json(&path, &[]).map(Some)
}

pub fn playlist_with_tracks(client: &Client, playlist: &Playlist) -> ApiResult<Playlist> {
    let uid = playlist
        .owner
        .as_ref()
        .and_then(|owner| owner.uid)
        .map(|uid| uid.to_string())
        .ok_or_else(|| ApiError::Fatal("У плейлиста нет владельца".to_string()))?;
    let kind = playlist
        .kind
        .map(|kind| kind.to_string())
        .ok_or_else(|| ApiError::Fatal("У плейлиста нет kind".to_string()))?;

    let path = format!("/users/{uid}/playlists/{kind}");
    client.api_get_json(&path, &[])
}

pub fn track_lyrics(
    client: &Client,
    track: &Track,
    format: &str,
) -> ApiResult<Option<TrackLyrics>> {
    let track_id = track_id(track).map_err(|e| ApiError::Fatal(e.to_string()))?;
    let (timestamp, sign) = lyrics_sign(track_id);
    let path = format!("/tracks/{track_id}/lyrics");
    let query = vec![
        ("format", format.to_string()),
        ("timeStamp", timestamp.to_string()),
        ("sign", sign),
    ];
    client.api_get_json(&path, &query).map(Some)
}

pub fn fetch_lyrics_text(client: &Client, lyrics: &TrackLyrics) -> ApiResult<String> {
    let url = lyrics
        .download_url
        .as_deref()
        .ok_or_else(|| ApiError::Fatal("Сервер не вернул ссылку на текст песни".to_string()))?;
    let bytes = client.fetch_bytes(url)?;
    String::from_utf8(bytes).map_err(|e| ApiError::Fatal(format!("Текст песни не в UTF-8: {e}")))
}

pub fn cover_url(client: &Client, track: &Track, size: &str) -> Option<String> {
    track
        .cover_uri
        .as_ref()
        .map(|uri| format!("{}{}", client.cover_base, uri.replace("%%", size)))
}
