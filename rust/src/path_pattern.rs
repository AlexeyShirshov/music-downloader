//! Построение пути сохранения по шаблону (порт `prepare_base_path`).

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

use crate::models::{Album, Track};

pub const DEFAULT_PATH_PATTERN: &str = "#album-artist/#album/#number - #title";
pub const DEFAULT_COVER_RESOLUTION: i64 = 400;

/// 255 минус самый длинный суффикс (".flac").
pub const MAX_FILE_NAME_BYTE_LENGTH_WITHOUT_SUFFIX: usize = 250;

fn safe_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        // Аналог Python-регулярки `([^\w\-\'() ]|^\s+|[.\s]+$)`.
        // `\w` в Python — это буквы/цифры/подчёркивание (без combining marks).
        Regex::new(r"([^\p{Alphabetic}\p{Nd}\p{Nl}\p{No}_\-\'() ]|^\s+|[.\s]+$)")
            .expect("регулярное выражение корректно")
    })
}

fn unsafe_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"[/\\]+").expect("регулярное выражение корректно"))
}

pub fn sanitize(value: &str, unsafe_path: bool) -> String {
    let regex = if unsafe_path {
        unsafe_regex()
    } else {
        safe_regex()
    };
    regex.replace_all(value, "_").into_owned()
}

/// `str(None)` в Python — это "None"; отсутствующие значения в шаблоне
/// исторически превращаются именно в него.
fn py_str(value: Option<String>) -> String {
    value.unwrap_or_else(|| "None".to_string())
}

pub fn full_title(title: &Option<String>, version: &Option<String>) -> String {
    let mut result = title.clone().unwrap_or_default();
    if let Some(version) = version {
        if !version.is_empty() {
            result.push_str(" (");
            result.push_str(version);
            result.push(')');
        }
    }
    result
}

pub fn track_title(track: &Track) -> String {
    full_title(&track.title, &track.version)
}

pub fn album_title(album: &Album) -> String {
    full_title(&album.title, &album.version)
}

fn zfill(value: i64, width: usize) -> String {
    let text = value.to_string();
    if text.len() >= width {
        text
    } else {
        format!("{}{}", "0".repeat(width - text.len()), text)
    }
}

/// Обрезает строку до `target` байт по границам символов.
pub fn truncate_to_byte_length(text: &str, target: usize) -> String {
    if text.len() <= target {
        return text.to_string();
    }

    let mut total = text.len();
    let mut cut = text.len();
    for (index, character) in text.char_indices().rev() {
        if total <= target {
            break;
        }
        total -= character.len_utf8();
        cut = index;
    }
    text[..cut].to_string()
}

pub fn prepare_base_path(pattern: &Path, track: &Track, unsafe_path: bool) -> PathBuf {
    let pattern_str = pattern.to_string_lossy().into_owned();
    let album = track.albums.first();
    let album_artist = album.and_then(|album| album.artists.first());
    let track_artist = track.artists.first();
    let track_position = album.and_then(|album| album.track_position.as_ref());

    let mut is_multi_disc = false;
    if let Some(volumes) = album.and_then(|album| album.volumes.as_ref()) {
        if volumes.len() > 1 {
            is_multi_disc = true;
        }
    }
    if !is_multi_disc {
        if let Some(volume) = track_position.and_then(|position| position.volume) {
            if volume > 1 {
                is_multi_disc = true;
            }
        }
    }

    let has_disc_placeholder =
        pattern_str.contains("#disc-number") || pattern_str.contains("#disc-number-padded");

    let mut disc_number_val = None;
    let mut disc_number_padded_val = None;
    let mut number_val = None;
    let mut number_padded_val = None;

    if let Some(volume) = track_position.and_then(|position| position.volume) {
        disc_number_val = Some(volume.to_string());
        let disc_count = album
            .and_then(|album| album.volumes.as_ref())
            .filter(|volumes| !volumes.is_empty())
            .map(|volumes| volumes.len() as i64)
            .unwrap_or(volume);
        let pad_length = std::cmp::max(2, disc_count.to_string().len());
        disc_number_padded_val = Some(zfill(volume, pad_length));
    }

    if let Some(index) = track_position.and_then(|position| position.index) {
        let index_str = index.to_string();
        let max_track_count = match album
            .and_then(|album| album.volumes.as_ref())
            .filter(|volumes| !volumes.is_empty())
        {
            Some(volumes) => volumes
                .iter()
                .map(Vec::len)
                .max()
                .map(|len| len as i64)
                .filter(|count| *count != 0),
            None => album
                .and_then(|album| album.track_count)
                .filter(|count| *count != 0),
        };
        let pad_length = max_track_count
            .map(|count| count.to_string().len())
            .unwrap_or(2);
        let padded_index_str = zfill(index, pad_length);

        let volume = track_position.and_then(|position| position.volume);
        if is_multi_disc && !has_disc_placeholder && volume.unwrap_or(0) != 0 {
            let volume = volume.unwrap_or_default();
            number_val = Some(format!("{volume}.{index_str}"));
            number_padded_val = Some(format!("{volume}.{padded_index_str}"));
        } else {
            number_val = Some(index_str);
            number_padded_val = Some(padded_index_str);
        }
    }

    let replacements: [(&str, String); 12] = [
        ("#number-padded", py_str(number_padded_val)),
        ("#number", py_str(number_val)),
        ("#disc-number-padded", py_str(disc_number_padded_val)),
        ("#disc-number", py_str(disc_number_val)),
        (
            "#album-artist",
            py_str(album_artist.and_then(|artist| artist.name.clone())),
        ),
        (
            "#track-artist",
            py_str(track_artist.and_then(|artist| artist.name.clone())),
        ),
        (
            "#artist-id",
            py_str(track_artist.and_then(|artist| artist.id.clone())),
        ),
        (
            "#album-id",
            py_str(album.and_then(|album| album.id.clone())),
        ),
        ("#track-id", py_str(track.id.clone())),
        ("#title", track_title(track)),
        (
            "#album",
            album.map(album_title).unwrap_or_else(|| "None".to_string()),
        ),
        (
            "#year",
            py_str(
                album
                    .and_then(|album| album.year)
                    .map(|year| year.to_string()),
            ),
        ),
    ];

    let mut path_str = pattern_str;
    for (placeholder, value) in replacements {
        let replacement = sanitize(&value, unsafe_path);
        path_str = path_str.replace(placeholder, &replacement);
    }

    let parsed = Path::new(&path_str);
    let mut result = PathBuf::new();
    let mut any = false;
    for component in parsed.components() {
        match component {
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => {
                result.push(component.as_os_str());
                any = true;
            }
            Component::ParentDir => {
                result.push("..");
                any = true;
            }
            Component::Normal(part) => {
                let part = part.to_string_lossy();
                result.push(truncate_to_byte_length(
                    &part,
                    MAX_FILE_NAME_BYTE_LENGTH_WITHOUT_SUFFIX,
                ));
                any = true;
            }
        }
    }

    if any {
        result
    } else {
        PathBuf::from(".")
    }
}
