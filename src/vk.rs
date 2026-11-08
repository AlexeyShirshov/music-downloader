//! Поддержка VK (vk.com / vk.ru): разбор ссылок и работа с Audio API.
//!
//! VK не отдаёт музыку неавторизованному клиенту, а обычные веб-токены не
//! имеют доступа к методам `audio.*`. Поэтому используется токен приложения
//! Kate Mobile вместе с его `User-Agent`.

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{bail, Result};
use regex::Regex;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::client::{ApiError, ApiResult};
use crate::models::{Artist, Track};

/// `User-Agent` Kate Mobile: обязателен при запросах к Audio API.
pub const VK_USER_AGENT: &str =
    "KateMobileAndroid/56 lite-460 (Android 4.4.2; SDK 19; x86; unknown Android SDK built for x86; en)";
pub const DEFAULT_VK_API_BASE: &str = "https://api.vk.com/method";
pub const DEFAULT_VK_API_VERSION: &str = "5.95";

/// Максимальный размер тела ответа (аудиофайл).
const BODY_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// Идентификатор аудиозаписи VK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioId {
    /// ID владельца: отрицательный у сообществ.
    pub owner_id: i64,
    /// ID самой аудиозаписи.
    pub audio_id: i64,
}

impl AudioId {
    /// Формат, который ожидает API: `-2001370911_36370911`.
    pub fn api_id(self) -> String {
        format!("{}_{}", self.owner_id, self.audio_id)
    }
}

/// Ссылка на страницу трека VK.
pub fn track_url(id: AudioId) -> String {
    format!("https://vk.com/audio{}", id.api_id())
}

/// Разбирает ссылку вида `https://vk.com/audio-2001370911_36370911`.
///
/// Возвращает `None`, если это не ссылка на аудиозапись VK.
pub fn parse_audio_link(input: &str) -> Option<AudioId> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let candidate = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let url = url::Url::parse(&candidate).ok()?;
    let raw_host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    let host = raw_host
        .strip_prefix("www.")
        .or_else(|| raw_host.strip_prefix("m."))
        .unwrap_or(&raw_host);
    if !matches!(host, "vk.com" | "vk.ru" | "vkontakte.ru") {
        return None;
    }

    static AUDIO_PATH: OnceLock<Regex> = OnceLock::new();
    let regex =
        AUDIO_PATH.get_or_init(|| Regex::new(r"^/audio(-?\d+)_(\d+)/?$").expect("regex корректен"));
    let captures = regex.captures(url.path())?;

    Some(AudioId {
        owner_id: captures[1].parse().ok()?,
        audio_id: captures[2].parse().ok()?,
    })
}

/// Извлекает `access_token` из строки, которую пользователь копирует после
/// авторизации. Принимает и полный URL (`https://oauth.vk.com/blank.html#access_token=...`),
/// и сам токен.
pub fn extract_access_token(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    if let Some(index) = input.find("access_token=") {
        let rest = &input[index + "access_token=".len()..];
        let end = rest
            .find(['&', '#', ' ', '\t', '\r', '\n'])
            .unwrap_or(rest.len());
        return valid_token(&rest[..end]);
    }

    valid_token(input)
}

/// VK-токены состоят из ASCII-букв, цифр и `_-.`; всё остальное — не токен.
fn valid_token(token: &str) -> Option<String> {
    let token = token.trim();
    if token.is_empty() || token.len() > 512 {
        return None;
    }
    let allowed = token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    allowed.then(|| token.to_string())
}

/// Пользователь из ответа `users.get`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct VkUser {
    #[serde(default, deserialize_with = "lenient_i64")]
    pub id: Option<i64>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub screen_name: Option<String>,
}

impl VkUser {
    /// Имя для сообщений: «Имя Фамилия», иначе `@screen_name`, иначе `id<id>`.
    pub fn display_name(&self) -> String {
        let name = [self.first_name.as_deref(), self.last_name.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !name.is_empty() {
            return name;
        }
        if let Some(screen_name) = self
            .screen_name
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            return format!("@{screen_name}");
        }
        match self.id {
            Some(id) => format!("id{id}"),
            None => "неизвестный аккаунт".to_string(),
        }
    }
}

/// Аудиозапись из ответа `audio.getById`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct VkAudio {
    #[serde(default, deserialize_with = "lenient_i64")]
    pub id: Option<i64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub owner_id: Option<i64>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub duration: Option<i64>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub genre_id: Option<i64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub date: Option<i64>,
}

/// Число, которое VK иногда отдаёт строкой.
fn lenient_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Number {
        Signed(i64),
        Text(String),
    }

    Ok(match Option::<Number>::deserialize(deserializer)? {
        Some(Number::Signed(value)) => Some(value),
        Some(Number::Text(value)) => value.trim().parse().ok(),
        None => None,
    })
}

impl VkAudio {
    /// Превращает аудиозапись VK в общую модель трека.
    pub fn to_track(&self, requested: AudioId) -> Track {
        let owner_id = self.owner_id.unwrap_or(requested.owner_id);
        let audio_id = self.id.unwrap_or(requested.audio_id);

        let artist = self
            .artist
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| {
                vec![Artist {
                    id: Some(owner_id.to_string()),
                    name: Some(name.to_string()),
                    counts: None,
                }]
            })
            .unwrap_or_default();

        Track {
            id: Some(format!("{owner_id}_{audio_id}")),
            title: self.title.clone(),
            artists: artist,
            duration_ms: self.duration.map(|seconds| seconds.saturating_mul(1000)),
            available: Some(self.url.is_some()),
            ..Track::default()
        }
    }

    /// Ссылка на mp3 (VK может отдать HLS-плейлист).
    pub fn download_url(&self) -> Result<String> {
        let url = self
            .url
            .as_deref()
            .filter(|url| !url.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("VK не вернул ссылку на аудио: трек недоступен или удалён")
            })?;
        mp3_url(url)
    }
}

/// Преобразует HLS-ссылку VK в прямую ссылку на mp3.
///
/// VK отдаёт `.../<hash>/audios/<hash>/index.m3u8?extra=...`, а тот же файл
/// доступен как `.../audios/<hash>.mp3?extra=...` — ровно так это делает
/// `vk_api` (`RE_M3U8_TO_MP3`).
pub fn mp3_url(url: &str) -> Result<String> {
    let path = url.split('?').next().unwrap_or(url);
    if !path.contains(".m3u8") {
        return Ok(url.to_string());
    }

    static M3U8: OnceLock<Regex> = OnceLock::new();
    let regex = M3U8.get_or_init(|| {
        Regex::new(r"/[0-9a-f]+(/audios)?/([0-9a-f]+)/index\.m3u8").expect("regex корректен")
    });

    let Some(captures) = regex.captures(url) else {
        bail!("VK вернул HLS-ссылку, которую не удалось преобразовать в mp3: {url}");
    };

    let prefix = captures.get(1).map(|part| part.as_str()).unwrap_or("");
    let file = &captures[2];
    let replacement = format!("{prefix}/{file}.mp3");
    Ok(regex.replace(url, replacement.as_str()).into_owned())
}

/// Клиент VK API с повторами, как у клиента Яндекс.Музыки.
pub struct VkClient {
    agent: ureq::Agent,
    token: String,
    tries: u32,
    retry_delay: Duration,
    pub api_base: String,
    pub api_version: String,
}

impl VkClient {
    pub fn new(token: String, timeout_secs: u64, tries: u32, retry_delay_secs: u64) -> Self {
        let timeout = Some(Duration::from_secs(timeout_secs));
        let config = ureq::Agent::config_builder()
            .timeout_connect(timeout)
            .timeout_recv_response(timeout)
            .timeout_per_read(timeout)
            .http_status_as_error(false)
            .build();

        let api_base = std::env::var("MUSIC_DOWNLOADER_VK_API_BASE_URL")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_VK_API_BASE.to_string());
        let api_version = std::env::var("MUSIC_DOWNLOADER_VK_API_VERSION")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_VK_API_VERSION.to_string());

        Self {
            agent: config.into(),
            token,
            tries,
            retry_delay: Duration::from_secs(retry_delay_secs),
            api_base,
            api_version,
        }
    }

    fn with_retries<T>(&self, mut operation: impl FnMut() -> ApiResult<T>) -> ApiResult<T> {
        let mut try_count: u32 = 0;
        loop {
            match operation() {
                Ok(value) => return Ok(value),
                Err(ApiError::Fatal(message)) => return Err(ApiError::Fatal(message)),
                Err(ApiError::Network(message)) => {
                    if self.tries == 0 || try_count < self.tries {
                        try_count += 1;
                        log::debug!(
                            "Сетевая ошибка VK: {message}. Повтор {try_count} из {}",
                            if self.tries == 0 {
                                "∞".to_string()
                            } else {
                                self.tries.to_string()
                            }
                        );
                        std::thread::sleep(self.retry_delay);
                        continue;
                    }
                    return Err(ApiError::Network(message));
                }
            }
        }
    }

    fn read_body(&self, mut response: ureq::http::Response<ureq::Body>) -> ApiResult<Vec<u8>> {
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(BODY_LIMIT)
            .read_to_vec()
            .map_err(|error| ApiError::Network(format!("Не удалось прочитать ответ: {error}")))?;

        if (200..300).contains(&status) {
            return Ok(body);
        }

        let text = format!("HTTP {status}");
        if matches!(status, 400 | 401 | 403 | 404) {
            Err(ApiError::Fatal(text))
        } else {
            Err(ApiError::Network(text))
        }
    }

    /// Запрашивает аудиозапись через `audio.getById`.
    pub fn get_by_id(&self, id: AudioId) -> ApiResult<VkAudio> {
        let url = format!("{}/audio.getById", self.api_base.trim_end_matches('/'));
        self.with_retries(|| {
            let request = self
                .agent
                .get(&url)
                .header("User-Agent", VK_USER_AGENT)
                .query("audios", id.api_id())
                .query("access_token", self.token.as_str())
                .query("v", self.api_version.as_str());

            log::debug!("VK audio.getById {}", id.api_id());
            match request.call() {
                Ok(response) => self
                    .read_body(response)
                    .and_then(|body| parse_audio_response(&body, id)),
                Err(error) => Err(ApiError::Network(format!(
                    "Ошибка запроса к VK API: {error}"
                ))),
            }
        })
    }

    /// Проверяет токен и возвращает аккаунт через `users.get`.
    pub fn current_user(&self) -> ApiResult<VkUser> {
        let url = format!("{}/users.get", self.api_base.trim_end_matches('/'));
        self.with_retries(|| {
            let request = self
                .agent
                .get(&url)
                .header("User-Agent", VK_USER_AGENT)
                .query("fields", "screen_name")
                .query("access_token", self.token.as_str())
                .query("v", self.api_version.as_str());

            log::debug!("VK users.get");
            match request.call() {
                Ok(response) => self
                    .read_body(response)
                    .and_then(|body| parse_user_response(&body)),
                Err(error) => Err(ApiError::Network(format!(
                    "Ошибка запроса к VK API: {error}"
                ))),
            }
        })
    }

    /// Скачивает аудиофайл по прямой ссылке.
    pub fn download(&self, url: &str) -> ApiResult<Vec<u8>> {
        let body = self.with_retries(|| {
            match self
                .agent
                .get(url)
                .header("User-Agent", VK_USER_AGENT)
                .call()
            {
                Ok(response) => self.read_body(response),
                Err(error) => Err(ApiError::Network(format!("Ошибка загрузки {url}: {error}"))),
            }
        })?;

        if body.starts_with(b"#EXTM3U") {
            return Err(ApiError::Fatal(
                "VK вернул HLS-плейлист вместо mp3. Попробуйте ещё раз или скачайте трек позже"
                    .to_string(),
            ));
        }

        if body.is_empty() {
            return Err(ApiError::Fatal("VK вернул пустой файл".to_string()));
        }

        let head = &body[..body.len().min(256)];
        if head.starts_with(b"<!DOCTYPE")
            || head.starts_with(b"<html")
            || head.starts_with(b"<?xml")
        {
            return Err(ApiError::Fatal(
                "VK вернул HTML-страницу вместо аудиофайла: ссылка устарела или запрос заблокирован"
                    .to_string(),
            ));
        }

        Ok(body)
    }
}

fn parse_audio_response(body: &[u8], requested: AudioId) -> ApiResult<VkAudio> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|error| ApiError::Fatal(format!("Некорректный ответ VK API: {error}")))?;

    if let Some(error) = api_error(&value) {
        return Err(error);
    }

    let response = value
        .get("response")
        .ok_or_else(|| ApiError::Fatal("VK API вернул ответ без поля response".to_string()))?;
    let audios: Vec<VkAudio> = serde_json::from_value(response.clone())
        .map_err(|error| ApiError::Fatal(format!("Не удалось разобрать ответ VK API: {error}")))?;

    audios.into_iter().next().ok_or_else(|| {
        ApiError::Fatal(format!(
            "VK не вернул аудиозапись {}: она удалена, скрыта или недоступна аккаунту",
            requested.api_id()
        ))
    })
}

fn parse_user_response(body: &[u8]) -> ApiResult<VkUser> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|error| ApiError::Fatal(format!("Некорректный ответ VK API: {error}")))?;

    if let Some(error) = api_error(&value) {
        return Err(error);
    }

    let response = value
        .get("response")
        .ok_or_else(|| ApiError::Fatal("VK API вернул ответ без поля response".to_string()))?;
    let users: Vec<VkUser> = serde_json::from_value(response.clone())
        .map_err(|error| ApiError::Fatal(format!("Не удалось разобрать ответ VK API: {error}")))?;

    users
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::Fatal("VK не вернул данные аккаунта".to_string()))
}

/// Превращает поле `error` из ответа VK API в ошибку клиента.
fn api_error(value: &Value) -> Option<ApiError> {
    let error = value.get("error")?;
    let code = error
        .get("error_code")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let message = error
        .get("error_msg")
        .and_then(Value::as_str)
        .unwrap_or("неизвестная ошибка");
    let text = explain_error(code, format!("VK API: {message} (код {code})"));
    Some(if matches!(code, 6 | 10) || code >= 500 {
        ApiError::Network(text)
    } else {
        ApiError::Fatal(text)
    })
}

fn explain_error(code: i64, message: String) -> String {
    match code {
        5 => format!(
            "{message}. Токен недействителен или не имеет доступа к аудио: получите токен Kate Mobile заново (https://vkhost.github.io)"
        ),
        15 | 100 | 201 => format!(
            "{message}. Нет доступа к этой аудиозаписи: проверьте ссылку и права аккаунта"
        ),
        _ => message,
    }
}
