//! Модели ответов API Яндекс.Музыки.
//!
//! API отдаёт `camelCase`, идентификаторы приходят то строками, то числами,
//! поэтому все поля необязательные, а идентификаторы нормализуются в строки.

use serde::{Deserialize, Deserializer};

/// Идентификатор, который API отдаёт строкой или числом.
pub type Id = String;

fn string_or_number<'de, D>(deserializer: D) -> Result<Option<Id>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum RawId {
        String(String),
        Signed(i64),
        Unsigned(u64),
    }

    Ok(
        Option::<RawId>::deserialize(deserializer)?.map(|value| match value {
            RawId::String(s) => s,
            RawId::Signed(n) => n.to_string(),
            RawId::Unsigned(n) => n.to_string(),
        }),
    )
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Track {
    #[serde(deserialize_with = "string_or_number")]
    pub id: Option<Id>,
    pub real_id: Option<Id>,
    pub title: Option<String>,
    pub version: Option<String>,
    pub available: Option<bool>,
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub cover_uri: Option<String>,
    pub lyrics_info: Option<LyricsInfo>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Album {
    #[serde(deserialize_with = "string_or_number")]
    pub id: Option<Id>,
    pub title: Option<String>,
    pub version: Option<String>,
    pub year: Option<i64>,
    pub release_date: Option<String>,
    pub genre: Option<String>,
    pub track_count: Option<i64>,
    pub meta_type: Option<String>,
    pub available: Option<bool>,
    pub artists: Vec<Artist>,
    pub volumes: Option<Vec<Vec<Track>>>,
    pub track_position: Option<TrackPosition>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Artist {
    #[serde(deserialize_with = "string_or_number")]
    pub id: Option<Id>,
    pub name: Option<String>,
    pub counts: Option<Counts>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Counts {
    pub tracks: Option<i64>,
    pub direct_albums: Option<i64>,
    pub also_albums: Option<i64>,
    pub also_tracks: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackPosition {
    pub volume: Option<i64>,
    pub index: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LyricsInfo {
    pub has_available_sync_lyrics: bool,
    pub has_available_text_lyrics: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtistAlbums {
    pub albums: Vec<Album>,
    pub pager: Option<Pager>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Pager {
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Playlist {
    pub uid: Option<i64>,
    pub kind: Option<i64>,
    pub title: Option<String>,
    pub track_count: Option<i64>,
    pub owner: Option<User>,
    pub tracks: Vec<TrackShort>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct User {
    pub uid: Option<i64>,
    pub login: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackShort {
    #[serde(deserialize_with = "string_or_number")]
    pub id: Option<Id>,
    pub album_id: Option<Id>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadInfo {
    #[serde(deserialize_with = "string_or_number")]
    pub track_id: Option<Id>,
    pub quality: Option<String>,
    pub codec: Option<String>,
    pub bitrate: Option<i64>,
    pub transport: Option<String>,
    pub url: Option<String>,
    pub urls: Vec<String>,
    pub key: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FileInfo {
    pub download_info: Option<DownloadInfo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackLyrics {
    pub download_url: Option<String>,
    pub lyric_id: Option<i64>,
    pub external_lyric_id: Option<String>,
    pub writers: Vec<String>,
}
