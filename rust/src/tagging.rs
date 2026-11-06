//! Запись тегов в MP3/FLAC/M4A (порт `set_tags`).

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use lofty::config::WriteOptions;
use lofty::picture::{Picture, PictureType};
use lofty::tag::{ItemKey, ItemValue, Tag, TagExt, TagItem, TagType};

use crate::api::Container;
use crate::mime::MimeType;
use crate::models::{Album, Track};
use crate::path_pattern::{album_title, track_title};

#[derive(Debug, Clone)]
pub struct AlbumCover {
    pub data: Vec<u8>,
    pub mime_type: MimeType,
}

fn is_truthy(value: &Option<i64>) -> bool {
    value.is_some_and(|value| value != 0)
}

fn release_date_parts(album: &Album) -> Result<(Option<String>, Option<String>)> {
    let mut iso_date = None;
    let mut release_year = None;

    if let Some(release_date) = album.release_date.as_deref() {
        let date = parse_release_datetime(release_date)
            .with_context(|| format!("Не удалось разобрать дату релиза: {release_date}"))?;
        release_year = Some(date.year().to_string());
        iso_date = Some(date.format("%Y-%m-%d %H:%M:%S").to_string());
    }

    if is_truthy(&album.year) {
        release_year = Some(album.year.unwrap_or_default().to_string());
    }

    Ok((iso_date, release_year))
}

fn parse_release_datetime(source: &str) -> Result<DateTime<Utc>> {
    let source = source.trim();

    if let Ok(date) = DateTime::parse_from_rfc3339(source) {
        return Ok(date.with_timezone(&Utc));
    }

    for format in [
        "%Y-%m-%dT%H:%M:%S%.f%z",
        "%Y-%m-%d %H:%M:%S%.f%z",
        "%Y-%m-%dT%H:%M%z",
        "%Y-%m-%d %H:%M%z",
    ] {
        if let Ok(date) = DateTime::parse_from_str(source, format) {
            return Ok(date.with_timezone(&Utc));
        }
    }

    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(date) = NaiveDateTime::parse_from_str(source, format) {
            return local_to_utc(date);
        }
    }

    if let Ok(date) = NaiveDate::parse_from_str(source, "%Y-%m-%d") {
        return local_to_utc(date.and_hms_opt(0, 0, 0).unwrap_or_default());
    }

    bail!("неподдерживаемый формат даты")
}

fn local_to_utc(date: NaiveDateTime) -> Result<DateTime<Utc>> {
    Local
        .from_local_datetime(&date)
        .earliest()
        .map(|date| date.with_timezone(&Utc))
        .ok_or_else(|| anyhow!("дата не существует в локальном часовом поясе"))
}

fn tag_type_for(container: Container) -> TagType {
    match container {
        Container::Mp3 => TagType::Id3v2,
        Container::Flac => TagType::VorbisComments,
        Container::Mp4 => TagType::Mp4Ilst,
    }
}

fn push_texts(tag: &mut Tag, key: ItemKey, values: &[String], write_empty: bool) {
    if values.is_empty() {
        if write_empty {
            // Python-версия записывает пустой кадр для MP3, сохраняем это поведение.
            tag.push(TagItem::new(key, ItemValue::Text(String::new())));
        }
        return;
    }
    for value in values {
        tag.push(TagItem::new(key, ItemValue::Text(value.clone())));
    }
}

pub fn set_tags(
    path: &Path,
    track: &Track,
    container: Container,
    lyrics: Option<&str>,
    album_cover: Option<&AlbumCover>,
    compatibility_level: i64,
) -> Result<()> {
    let empty_album = Album::default();
    let album = track.albums.first().unwrap_or(&empty_album);
    let album_title = album_title(album);
    let track_title = track_title(track);

    let track_artists: Vec<String> = track
        .artists
        .iter()
        .filter_map(|artist| artist.name.clone())
        .filter(|name| !name.is_empty())
        .collect();
    let album_artists: Vec<String> = album
        .artists
        .iter()
        .filter_map(|artist| artist.name.clone())
        .filter(|name| !name.is_empty())
        .collect();

    let genre = album.genre.clone().filter(|genre| !genre.is_empty());
    let (track_number, disc_number) = album
        .track_position
        .as_ref()
        .map(|position| (position.index, position.volume))
        .unwrap_or((None, None));

    let (iso_date, release_year) = release_date_parts(album)?;
    let date_text = iso_date.clone().or_else(|| release_year.clone());

    let track_url = format!(
        "https://music.yandex.ru/album/{}/track/{}",
        album.id.as_deref().unwrap_or("None"),
        track.id.as_deref().unwrap_or("None"),
    );

    let mut tag = Tag::new(tag_type_for(container));

    tag.insert_text(ItemKey::TrackTitle, track_title);
    tag.insert_text(ItemKey::AlbumTitle, album_title);

    match container {
        Container::Mp4 => {
            let (artists, album_artists) = if compatibility_level == 1 {
                (
                    vec![track_artists.join("; ")],
                    vec![album_artists.join("; ")],
                )
            } else {
                (track_artists.clone(), album_artists.clone())
            };
            push_texts(&mut tag, ItemKey::TrackArtist, &artists, false);
            push_texts(&mut tag, ItemKey::AlbumArtist, &album_artists, false);
        }
        Container::Mp3 | Container::Flac => {
            push_texts(
                &mut tag,
                ItemKey::TrackArtist,
                &track_artists,
                container == Container::Mp3,
            );
            push_texts(
                &mut tag,
                ItemKey::AlbumArtist,
                &album_artists,
                container == Container::Mp3,
            );
        }
    }

    if let Some(date_text) = date_text.clone() {
        // Для MP3 дата пишется вручную: lofty разбирает TDRC как Timestamp
        // и теряет время, а mutagen пишет строку как есть.
        if container != Container::Mp3 {
            tag.insert_text(ItemKey::RecordingDate, date_text);
        }
    }
    if is_truthy(&track_number) {
        tag.insert_text(
            ItemKey::TrackNumber,
            track_number.unwrap_or_default().to_string(),
        );
    }
    if is_truthy(&disc_number) {
        tag.insert_text(
            ItemKey::DiscNumber,
            disc_number.unwrap_or_default().to_string(),
        );
    }
    if let Some(genre) = genre {
        tag.insert_text(ItemKey::Genre, genre);
    }

    if let Some(lyrics) = lyrics.filter(|lyrics| !lyrics.is_empty()) {
        let key = match container {
            // USLT в ID3v2, ©lyr/LYRICS в остальных контейнерах.
            Container::Mp3 => ItemKey::UnsyncLyrics,
            Container::Mp4 | Container::Flac => ItemKey::Lyrics,
        };
        tag.insert_text(key, lyrics.to_string());
    }

    if let Some(cover) = album_cover {
        let picture = Picture::unchecked(cover.data.clone())
            .pic_type(PictureType::CoverFront)
            .mime_type(cover.mime_type.lofty())
            .build();
        tag.push_picture(picture);
    }

    if container == Container::Mp4 || container == Container::Flac {
        tag.insert_text(ItemKey::Comment, track_url.clone());
    }

    tag.save_to_path(path, WriteOptions::default())
        .with_context(|| format!("Не удалось записать теги в {}", path.display()))?;

    if container == Container::Mp3 {
        if let Some(date_text) = date_text {
            // mutagen (ID3TimeStamp) сериализует дату через 'T'.
            let id3_date = date_text.replacen(' ', "T", 1);
            if let Err(error) = add_id3v2_text_frame(path, "TDRC", &id3_date) {
                log::warn!("Не удалось добавить кадр TDRC: {error:#}");
            }
        }
        if let Err(error) = add_id3v2_url_frame(path, "WOAF", &track_url) {
            log::warn!("Не удалось добавить кадр WOAF: {error:#}");
        }
    }

    Ok(())
}

/// Lofty не умеет писать произвольные ID3-кадры, поэтому TDRC/WOAF добавляются
/// в уже записанный тег (место под них берётся из padding).
fn add_id3v2_url_frame(path: &Path, frame_id: &str, url: &str) -> Result<()> {
    add_id3v2_frame(path, frame_id, url.as_bytes())
}

fn add_id3v2_text_frame(path: &Path, frame_id: &str, text: &str) -> Result<()> {
    let mut content = Vec::with_capacity(1 + text.len());
    content.push(3); // UTF-8, как пишет mutagen
    content.extend_from_slice(text.as_bytes());
    add_id3v2_frame(path, frame_id, &content)
}

fn add_id3v2_frame(path: &Path, frame_id: &str, content: &[u8]) -> Result<()> {
    let mut data = std::fs::read(path)?;
    if data.len() < 10 || &data[..3] != b"ID3" {
        bail!("нет ID3v2-тега");
    }

    let major = data[3];
    if major != 4 {
        bail!("поддерживается только ID3v2.4");
    }

    let tag_size = syncsafe_to_u32(&data[6..10]) as usize;
    let tag_end = 10 + tag_size;
    if data.len() < tag_end {
        bail!("обрезанный ID3v2-тег");
    }

    let frame = build_frame(frame_id, content)?;
    let padding_start = find_id3v2_padding(&data[10..tag_end]).unwrap_or(tag_size);
    if tag_size - padding_start < frame.len() {
        bail!("недостаточно padding для кадра {frame_id}");
    }

    let mut tag_bytes = data[10..tag_end].to_vec();
    tag_bytes.splice(padding_start..padding_start, frame);
    tag_bytes.truncate(tag_size);
    data[10..tag_end].copy_from_slice(&tag_bytes);

    std::fs::write(path, data)?;
    Ok(())
}

fn syncsafe_to_u32(bytes: &[u8]) -> u32 {
    (u32::from(bytes[0] & 0x7F) << 21)
        | (u32::from(bytes[1] & 0x7F) << 14)
        | (u32::from(bytes[2] & 0x7F) << 7)
        | u32::from(bytes[3] & 0x7F)
}

fn u32_to_syncsafe(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7F) as u8,
        ((value >> 14) & 0x7F) as u8,
        ((value >> 7) & 0x7F) as u8,
        (value & 0x7F) as u8,
    ]
}

fn build_frame(frame_id: &str, content: &[u8]) -> Result<Vec<u8>> {
    if frame_id.len() != 4 {
        bail!("идентификатор кадра должен быть длиной 4 байта");
    }
    let mut frame = Vec::with_capacity(10 + content.len());
    frame.extend_from_slice(frame_id.as_bytes());
    frame.extend_from_slice(&u32_to_syncsafe(content.len() as u32));
    frame.extend_from_slice(&[0, 0]); // flags
    frame.extend_from_slice(content);
    Ok(frame)
}

/// Возвращает смещение первого байта padding (нулевой идентификатор кадра).
fn find_id3v2_padding(body: &[u8]) -> Option<usize> {
    let mut offset = 0;
    while offset + 10 <= body.len() {
        if body[offset] == 0 {
            return Some(offset);
        }
        let size = syncsafe_to_u32(&body[offset + 4..offset + 8]) as usize;
        if size == 0 && body[offset + 4..offset + 8].iter().all(|byte| *byte == 0) {
            return Some(offset);
        }
        offset += 10 + size;
    }
    Some(offset.min(body.len()))
}
