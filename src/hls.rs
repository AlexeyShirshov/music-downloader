//! Сборка HLS-потока в один файл.
//!
//! VK отдаёт аудио как HLS-плейлист: сегменты AES-128-CBC (часть может быть
//! без шифрования), ключ и IV задаются тегами `EXT-X-KEY`. Здесь плейлист
//! разбирается, сегменты скачиваются по порядку, расшифровываются и
//! склеиваются в один поток.

use std::collections::HashMap;

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, KeyInit};
use aes::Aes128;
use anyhow::{bail, Context, Result};
use url::Url;

/// Сколько вложенных плейлистов разрешаем (master → media).
const MAX_DEPTH: usize = 4;

/// Один сегмент (или init-секция) плейлиста.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub url: String,
    pub range: Option<(u64, u64)>,
    pub key_url: Option<String>,
    pub iv: Option<[u8; 16]>,
}

/// Тип скачанного потока — по первым байтам.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioKind {
    Mp3,
    Mp4,
    Ts,
    Unknown,
}

impl AudioKind {
    pub fn sniff(bytes: &[u8]) -> Self {
        if bytes.starts_with(b"ID3")
            || (bytes.len() > 1 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0)
        {
            return Self::Mp3;
        }
        if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
            return Self::Mp4;
        }
        if bytes.first() == Some(&0x47) {
            return Self::Ts;
        }
        Self::Unknown
    }
}

/// Как ходить в сеть: URL и необязательный диапазон байт (`Range`).
pub type Fetch<'a> = &'a dyn Fn(&str, Option<(u64, u64)>) -> Result<Vec<u8>>;

/// Скачивает плейлист и возвращает склеенный поток.
///
/// `fetch` получает URL и необязательный диапазон байт (`Range`), чтобы
/// вызывающий сам решал, как ходить в сеть (и мог подставить заголовки).
pub fn download(url: &str, fetch: Fetch<'_>) -> Result<Vec<u8>> {
    download_playlist(url, fetch, 0)
}

fn download_playlist(url: &str, fetch: Fetch<'_>, depth: usize) -> Result<Vec<u8>> {
    if depth > MAX_DEPTH {
        bail!("HLS-плейлист вложен слишком глубоко");
    }

    let body = fetch(url, None)?;
    let text = String::from_utf8(body).context("HLS-плейлист не является текстом UTF-8")?;
    if !text.trim_start().starts_with("#EXTM3U") {
        bail!("VK вернул не HLS-плейлист");
    }
    let base = Url::parse(url).with_context(|| format!("Некорректный адрес плейлиста: {url}"))?;

    if let Some(variant) = best_variant(&text, &base)? {
        return download_playlist(&variant, fetch, depth + 1);
    }

    let pieces = parse_media(&text, &base)?;
    if pieces.is_empty() {
        bail!("В HLS-плейлисте нет сегментов");
    }

    let mut keys: HashMap<String, Vec<u8>> = HashMap::new();
    let mut output = Vec::new();
    for piece in pieces {
        let mut bytes = fetch(&piece.url, piece.range)?;
        if let Some(key_url) = &piece.key_url {
            if !keys.contains_key(key_url) {
                let key = fetch(key_url, None)?;
                keys.insert(key_url.clone(), key);
            }
            let key = &keys[key_url];
            let iv = piece
                .iv
                .as_ref()
                .context("Зашифрованный сегмент HLS без IV")?;
            bytes = decrypt_aes_128(key, iv, &bytes)?;
        }
        output.extend_from_slice(&bytes);
    }

    Ok(output)
}

/// Выбирает вариант с наибольшим битрейтом из master-плейлиста.
fn best_variant(text: &str, base: &Url) -> Result<Option<String>> {
    let mut variants: Vec<(u64, String)> = Vec::new();
    let mut bandwidth: Option<u64> = None;

    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let attrs = attributes(rest);
            bandwidth = attrs
                .iter()
                .find(|(name, _)| name == "BANDWIDTH")
                .and_then(|(_, value)| value.parse().ok());
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(bw) = bandwidth.take() {
            variants.push((bw, resolve(base, line)?));
        }
    }

    Ok(variants
        .into_iter()
        .max_by_key(|(bandwidth, _)| *bandwidth)
        .map(|(_, url)| url))
}

/// Разбирает media-плейлист в упорядоченный список сегментов.
pub fn parse_media(text: &str, base: &Url) -> Result<Vec<Piece>> {
    let mut pieces = Vec::new();
    let mut media_sequence: u64 = 0;
    let mut key_url: Option<String> = None;
    let mut key_iv: Option<[u8; 16]> = None;
    let mut pending_range: Option<String> = None;
    let mut pending_map: Option<Piece> = None;
    let mut range_offsets: HashMap<String, u64> = HashMap::new();

    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("#EXT-X-KEY:") {
            let attrs = attributes(rest);
            let method = attrs
                .iter()
                .find(|(name, _)| name == "METHOD")
                .map(|(_, value)| value.to_ascii_uppercase())
                .unwrap_or_else(|| "NONE".to_string());
            match method.as_str() {
                "NONE" => {
                    key_url = None;
                    key_iv = None;
                }
                "AES-128" => {
                    let uri = attrs
                        .iter()
                        .find(|(name, _)| name == "URI")
                        .map(|(_, value)| value.clone())
                        .context("EXT-X-KEY без URI")?;
                    key_url = Some(resolve(base, &uri)?);
                    key_iv = attrs
                        .iter()
                        .find(|(name, _)| name == "IV")
                        .map(|(_, value)| parse_iv(value))
                        .transpose()?;
                }
                other => bail!("HLS использует неподдерживаемое шифрование: {other}"),
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            media_sequence = rest.trim().parse().unwrap_or(0);
            continue;
        }

        if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            let attrs = attributes(rest);
            let uri = attrs
                .iter()
                .find(|(name, _)| name == "URI")
                .map(|(_, value)| value.clone())
                .context("EXT-X-MAP без URI")?;
            let url = resolve(base, &uri)?;
            if key_url.is_some() && key_iv.is_none() {
                bail!("HLS шифрует init-секцию без явного IV");
            }
            let range = match attrs
                .iter()
                .find(|(name, _)| name == "BYTERANGE")
                .map(|(_, value)| value.as_str())
            {
                Some(value) => Some(parse_range(value, &url, &mut range_offsets)?),
                None => None,
            };
            pending_map = Some(Piece {
                url,
                range,
                key_url: key_url.clone(),
                iv: key_iv,
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("#EXT-X-BYTERANGE:") {
            pending_range = Some(rest.trim().to_string());
            continue;
        }

        if line.starts_with('#') {
            continue;
        }

        let url = resolve(base, line)?;
        if let Some(map) = pending_map.take() {
            pieces.push(map);
        }

        let range = match pending_range.take() {
            Some(value) => Some(parse_range(&value, &url, &mut range_offsets)?),
            None => None,
        };

        let iv = match (&key_url, key_iv) {
            (Some(_), Some(iv)) => Some(iv),
            (Some(_), None) => {
                let mut iv = [0u8; 16];
                iv[8..].copy_from_slice(&media_sequence.to_be_bytes());
                Some(iv)
            }
            (None, _) => None,
        };

        pieces.push(Piece {
            url,
            range,
            key_url: key_url.clone(),
            iv,
        });
        media_sequence += 1;
    }

    Ok(pieces)
}

fn parse_iv(value: &str) -> Result<[u8; 16]> {
    let value = value.trim();
    let digits = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    let padded = format!("{digits:0>32}");
    let bytes = hex::decode(&padded).context("Некорректный IV в HLS-плейлисте")?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("IV в HLS-плейлисте должен быть 16 байт"))
}

fn resolve(base: &Url, value: &str) -> Result<String> {
    base.join(value)
        .map(|url| url.to_string())
        .with_context(|| format!("Некорректный адрес в HLS-плейлисте: {value}"))
}

fn parse_range(value: &str, url: &str, offsets: &mut HashMap<String, u64>) -> Result<(u64, u64)> {
    let (length, offset) = match value.split_once('@') {
        Some((length, offset)) => (
            length.parse::<u64>().context("Некорректный BYTERANGE")?,
            Some(offset.parse::<u64>().context("Некорректный BYTERANGE")?),
        ),
        None => (
            value.parse::<u64>().context("Некорректный BYTERANGE")?,
            None,
        ),
    };
    let start = match offset {
        Some(offset) => offset,
        None => *offsets.get(url).unwrap_or(&0),
    };
    offsets.insert(url.to_string(), start + length);
    Ok((start, start + length - 1))
}

fn decrypt_aes_128(key: &[u8], iv: &[u8; 16], data: &[u8]) -> Result<Vec<u8>> {
    if key.len() != 16 {
        bail!("Ключ HLS должен быть длиной 16 байт, а не {}", key.len());
    }
    if data.is_empty() || !data.len().is_multiple_of(16) {
        bail!("Зашифрованный сегмент HLS имеет длину не кратную 16");
    }

    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut output = data.to_vec();
    let mut previous = *iv;
    let (chunks, _) = output.as_chunks_mut::<16>();
    for chunk in chunks {
        let ciphertext = *chunk;
        let mut block = GenericArray::from(ciphertext);
        cipher.decrypt_block(&mut block);
        for (index, byte) in chunk.iter_mut().enumerate() {
            *byte = block[index] ^ previous[index];
        }
        previous = ciphertext;
    }

    // PKCS#7: если дополнение выглядит корректно, срезаем его. Если нет —
    // оставляем данные как есть (как это делает vkpymusic).
    if let Some(&padding) = output.last() {
        let padding = padding as usize;
        if (1..=16).contains(&padding)
            && output.len() >= padding
            && output[output.len() - padding..]
                .iter()
                .all(|byte| *byte as usize == padding)
        {
            output.truncate(output.len() - padding);
        }
    }

    Ok(output)
}

/// Разбирает список атрибутов `KEY=VALUE,KEY="VALUE,WITH,COMMAS"`.
fn attributes(value: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let key_start = index;
        while index < bytes.len() && bytes[index] != b'=' {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        let key = value[key_start..index].trim().to_ascii_uppercase();
        index += 1;

        let parsed = if index < bytes.len() && bytes[index] == b'"' {
            index += 1;
            let value_start = index;
            while index < bytes.len() && bytes[index] != b'"' {
                index += 1;
            }
            let parsed = value[value_start..index].to_string();
            index += 1;
            parsed
        } else {
            let value_start = index;
            while index < bytes.len() && bytes[index] != b',' {
                index += 1;
            }
            value[value_start..index].trim().to_string()
        };
        result.push((key, parsed));

        while index < bytes.len() && bytes[index] != b',' {
            index += 1;
        }
        if index < bytes.len() {
            index += 1;
        }
    }
    result
}
