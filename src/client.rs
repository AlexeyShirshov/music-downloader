//! HTTP-клиент с авторизацией, повторами и разбором ответов API.

use std::fmt;
use std::time::Duration;

use serde::de::DeserializeOwned;

/// Заголовок клиента, который использует yandex-music по умолчанию.
pub const YANDEX_CLIENT_HEADER: &str = "YandexMusicAndroid/24023621";
pub const USER_AGENT: &str = "Yandex-Music-API";
pub const DEFAULT_API_BASE: &str = "https://api.music.yandex.net";
pub const DEFAULT_COVER_BASE: &str = "https://";

pub const YANDEX_OAUTH_BASE: &str = "https://oauth.yandex.ru";
pub const DEFAULT_CLIENT_ID: &str = "23cabbbdc6cd418abb4b39c32c41195d";
pub const DEFAULT_CLIENT_SECRET: &str = "53bc75238f0c4d08a118e51fe9203300";

/// Максимальный размер тела ответа (треки во FLAC бывают большими).
const BODY_LIMIT: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_url: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
}

#[derive(Debug)]
pub enum ApiError {
    /// Ошибка сети/сервера, повтор запроса имеет смысл.
    Network(String),
    /// Ошибка, которую повторять бессмысленно (авторизация, 400, 404).
    Fatal(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Network(message) | ApiError::Fatal(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ApiError {}

pub type ApiResult<T> = Result<T, ApiError>;

fn retryable(status: u16) -> bool {
    !matches!(status, 400 | 401 | 403 | 404)
}

pub struct Client {
    agent: ureq::Agent,
    pub token: String,
    tries: u32,
    retry_delay: Duration,
    pub api_base: String,
    pub cover_base: String,
}

impl Client {
    pub fn new(token: String, timeout_secs: u64, tries: u32, retry_delay_secs: u64) -> Self {
        let timeout = Some(Duration::from_secs(timeout_secs));
        let config = ureq::Agent::config_builder()
            .timeout_connect(timeout)
            .timeout_recv_response(timeout)
            .timeout_per_read(timeout)
            .http_status_as_error(false)
            .build();

        let api_base = std::env::var("MUSIC_DOWNLOADER_API_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_API_BASE.to_string());
        let cover_base = std::env::var("MUSIC_DOWNLOADER_COVER_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_COVER_BASE.to_string());

        Self {
            agent: config.into(),
            token,
            tries,
            retry_delay: Duration::from_secs(retry_delay_secs),
            api_base,
            cover_base,
        }
    }

    /// Выполняет операцию с повторами, как это делает Python-версия.
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
                            "Сетевая ошибка: {message}. Повтор {try_count} из {}",
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
            .map_err(|e| ApiError::Network(format!("Не удалось прочитать ответ: {e}")))?;

        if (200..300).contains(&status) {
            return Ok(body);
        }

        let message = error_message(&body);
        let text = if message.is_empty() {
            format!("HTTP {status}")
        } else {
            format!("HTTP {status}: {message}")
        };
        if retryable(status) {
            Err(ApiError::Network(text))
        } else {
            Err(ApiError::Fatal(text))
        }
    }

    /// GET-запрос к API с заголовками авторизации.
    pub fn api_get(&self, path: &str, query: &[(&str, String)]) -> ApiResult<Vec<u8>> {
        let url = format!("{}{}", self.api_base, path);
        self.with_retries(|| {
            let mut request = self
                .agent
                .get(&url)
                .header("X-Yandex-Music-Client", YANDEX_CLIENT_HEADER)
                .header("User-Agent", USER_AGENT);
            if !self.token.is_empty() {
                request = request.header("Authorization", format!("OAuth {}", self.token));
            }
            for (key, value) in query {
                request = request.query(*key, value.as_str());
            }
            log::debug!("GET {url}");
            match request.call() {
                Ok(response) => self.read_body(response),
                Err(error) => Err(ApiError::Network(format!(
                    "Ошибка запроса к {url}: {error}"
                ))),
            }
        })
    }

    /// GET-запрос к API с разбором JSON и распаковкой секции `result`.
    pub fn api_get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> ApiResult<T> {
        let body = self.api_get(path, query)?;
        let value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|e| ApiError::Fatal(format!("Некорректный ответ сервера: {e}")))?;

        let result = match value.get("result") {
            Some(result) if !result.is_null() => result.clone(),
            _ => value,
        };

        serde_json::from_value(result)
            .map_err(|e| ApiError::Fatal(format!("Не удалось разобрать ответ сервера: {e}")))
    }

    /// Скачивает произвольный URL (файл трека, обложку, текст песни).
    pub fn fetch_bytes(&self, url: &str) -> ApiResult<Vec<u8>> {
        self.with_retries(
            || match self.agent.get(url).header("User-Agent", USER_AGENT).call() {
                Ok(response) => self.read_body(response),
                Err(error) => Err(ApiError::Network(format!("Ошибка загрузки {url}: {error}"))),
            },
        )
    }

    /// Разбирает JSON из скачанного тела.
    pub fn fetch_json<T: DeserializeOwned>(&self, url: &str) -> ApiResult<T> {
        let body = self.fetch_bytes(url)?;
        let value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|e| ApiError::Fatal(format!("Некорректный ответ сервера: {e}")))?;
        let result = match value.get("result") {
            Some(result) if !result.is_null() => result.clone(),
            _ => value,
        };
        serde_json::from_value(result)
            .map_err(|e| ApiError::Fatal(format!("Не удалось разобрать ответ сервера: {e}")))
    }

    /// Запрос кода устройства для OAuth Device Flow.
    pub fn request_device_code(&self) -> ApiResult<DeviceCode> {
        let url = format!("{}/device/code", YANDEX_OAUTH_BASE);
        let device_id: String = {
            use rand::Rng;
            rand::thread_rng()
                .sample_iter(&rand::distributions::Alphanumeric)
                .take(10)
                .map(char::from)
                .collect()
        };

        let mut response = self
            .agent
            .post(&url)
            .send_form([
                ("client_id", DEFAULT_CLIENT_ID),
                ("device_id", device_id.as_str()),
                ("device_name", "YandexMusicAPI"),
            ])
            .map_err(|e| ApiError::Network(format!("Ошибка запроса кода устройства: {e}")))?;

        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(BODY_LIMIT)
            .read_to_vec()
            .map_err(|e| ApiError::Network(format!("Не удалось прочитать ответ: {e}")))?;

        if !(200..300).contains(&status) {
            let message = error_message(&body);
            return Err(ApiError::Fatal(if message.is_empty() {
                format!("HTTP {status}")
            } else {
                format!("HTTP {status}: {message}")
            }));
        }

        serde_json::from_slice(&body)
            .map_err(|e| ApiError::Fatal(format!("Некорректный ответ сервера: {e}")))
    }

    /// Однократный опрос статуса подтверждения кода устройства.
    pub fn poll_device_token(&self, device_code: &str) -> ApiResult<Option<String>> {
        let url = format!("{}/token", YANDEX_OAUTH_BASE);
        let mut response = self
            .agent
            .post(&url)
            .send_form([
                ("grant_type", "device_code"),
                ("code", device_code),
                ("client_id", DEFAULT_CLIENT_ID),
                ("client_secret", DEFAULT_CLIENT_SECRET),
            ])
            .map_err(|e| ApiError::Network(format!("Ошибка запроса токена: {e}")))?;

        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(BODY_LIMIT)
            .read_to_vec()
            .map_err(|e| ApiError::Network(format!("Не удалось прочитать ответ: {e}")))?;

        if (200..300).contains(&status) {
            let token: OAuthToken = serde_json::from_slice(&body)
                .map_err(|e| ApiError::Fatal(format!("Некорректный ответ сервера: {e}")))?;
            return Ok(Some(token.access_token));
        }

        let value: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        let error = value
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or_default();

        if error == "authorization_pending" {
            return Ok(None);
        }

        let message = value
            .get("error_description")
            .and_then(|value| value.as_str())
            .unwrap_or(error);

        Err(ApiError::Fatal(format!("OAuth error: {message}")))
    }
}

fn error_message(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return text.trim().chars().take(300).collect();
    };

    let name = value
        .pointer("/error/name")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let message = value
        .pointer("/error/message")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    if !name.is_empty() && !message.is_empty() {
        return format!("{name}: {message}");
    }
    if !message.is_empty() {
        return message.to_string();
    }
    if !name.is_empty() {
        return name.to_string();
    }

    value
        .pointer("/error")
        .and_then(|error| error.as_str())
        .unwrap_or_default()
        .to_string()
}
