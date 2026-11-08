use std::collections::HashMap;
use std::io::{self, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use clap::{ArgGroup, Parser, ValueEnum};
use regex::Regex;
use sha2::{Digest, Sha256};

use music_downloader::api::{self, Container, CustomDownloadInfo};
use music_downloader::client::Client;
use music_downloader::mime::guess_mime_type;
use music_downloader::models::{Album, Track};
use music_downloader::path_pattern::{
    prepare_base_path, DEFAULT_PATH_PATTERN, DEFAULT_VK_PATH_PATTERN,
};
use music_downloader::tagging::{set_tags, set_tags_with_url, AlbumCover};
use music_downloader::vk::{self, AudioId};

const AUDIO_FILE_SUFFIXES: [&str; 3] = [".mp3", ".flac", ".m4a"];
const TEMPORARY_FILE_NAME_TEMPLATE: &str = ".music-downloader.{}.tmp";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LyricsFormat {
    None,
    Text,
    Lrc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoreQuality {
    Low,
    Normal,
    Lossless,
}

impl CoreQuality {
    fn api_quality(self) -> api::ApiQuality {
        match self {
            CoreQuality::Low => api::ApiQuality::Low,
            CoreQuality::Normal => api::ApiQuality::Normal,
            CoreQuality::Lossless => api::ApiQuality::Lossless,
        }
    }
}

fn checked_int(value: &str, min: i64, max: Option<i64>) -> Result<i64, String> {
    let number: i64 = value
        .parse()
        .map_err(|_| "Значение должно быть целым числом".to_string())?;
    if number >= min && max.is_none_or(|max| number <= max) {
        return Ok(number);
    }
    let mut error = format!("Значение должен быть >= {min}");
    if let Some(max) = max {
        error.push_str(&format!(" и <= {max}"));
    }
    Err(error)
}

fn cover_resolution_arg(value: &str) -> Result<i64, String> {
    if value == "original" {
        return Ok(-1);
    }
    checked_int(value, 100, None)
}

#[derive(Parser, Debug)]
#[command(
    name = "music-downloader",
    version,
    about = "Загрузчик музыки с сервисов Яндекс.Музыка и VK",
    group(ArgGroup::new("id").multiple(false).args([
        "artist_id", "album_id", "track_id", "playlist_id", "url"
    ]))
)]
struct Args {
    #[arg(
        long,
        help_heading = "Общие параметры",
        value_name = "Качество",
        default_value_t = 2,
        value_parser = clap::value_parser!(u8).range(0..=2),
        help = "Качество трека:\n0 - Низкое (AAC 64kbps)\n1 - Оптимальное (AAC 192kbps)\n2 - Лучшее (FLAC)\n(по умолчанию: 2)"
    )]
    quality: u8,

    #[arg(
        long,
        help_heading = "Общие параметры",
        help = "Пропускать уже загруженные треки"
    )]
    skip_existing: bool,

    #[arg(
        long,
        help_heading = "Общие параметры",
        value_enum,
        default_value_t = LyricsFormat::None,
        value_name = "{none,text,lrc}",
        help = "Формат текста песни (по умолчанию: none)"
    )]
    lyrics_format: LyricsFormat,

    #[arg(long, help_heading = "Общие параметры", hide = true)]
    add_lyrics: bool,

    #[arg(
        long,
        help_heading = "Общие параметры",
        help = "Встраивать обложку в аудиофайл"
    )]
    embed_cover: bool,

    #[arg(
        long,
        help_heading = "Общие параметры",
        value_name = "Разрешение обложки",
        default_value = "400",
        value_parser = cover_resolution_arg,
        help = "Разрешение обложки (в пикселях). Передайте \"original\" для загрузки в оригинальном (наилучшем) разрешении (по умолчанию: 400)"
    )]
    cover_resolution: i64,

    #[arg(
        long,
        help_heading = "Общие параметры",
        value_name = "Задержка",
        default_value_t = 0,
        value_parser = clap::value_parser!(u64),
        help = "Задержка между запросами, в секундах (по умолчанию: 0)"
    )]
    delay: u64,

    #[arg(
        long,
        help_heading = "Общие параметры",
        help = "Загружать альбомы, созданные только данным исполнителем"
    )]
    stick_to_artist: bool,

    #[arg(
        long,
        help_heading = "Общие параметры",
        help = "Загружать только музыкальные альбомы (пропускать подкасты и аудиокниги)"
    )]
    only_music: bool,

    #[arg(
        long,
        help_heading = "Общие параметры",
        value_name = "Уровень совместимости",
        default_value_t = 1,
        value_parser = clap::value_parser!(u8).range(0..=1),
        help = "Уровень совместимости, от 0 до 1. См. README для подробного описания (по умолчанию: 1)"
    )]
    compatibility_level: u8,

    #[arg(long, help_heading = "Общие параметры", hide = true)]
    debug: bool,

    #[arg(
        long,
        help_heading = "Сетевые параметры",
        value_name = "Время ожидания",
        default_value_t = 20,
        value_parser = clap::value_parser!(u64).range(1..),
        help = "Время ожидания ответа от сервера, в секундах. Увеличьте если возникают сетевые ошибки (по умолчанию: 20)"
    )]
    timeout: u64,

    #[arg(
        long,
        help_heading = "Сетевые параметры",
        value_name = "Количество попыток",
        default_value_t = 20,
        value_parser = clap::value_parser!(u64),
        help = "Количество попыток при возникновении сетевых ошибок. 0 - бесконечное количество попыток (по умолчанию: 20)"
    )]
    tries: u64,

    #[arg(
        long,
        help_heading = "Сетевые параметры",
        value_name = "Задержка",
        default_value_t = 5,
        value_parser = clap::value_parser!(u64),
        help = "Задержка между повторными запросами при сетевых ошибках (по умолчанию: 5)"
    )]
    retry_delay: u64,

    #[arg(long, help_heading = "ID", group = "id", value_name = "ID исполнителя")]
    artist_id: Option<String>,

    #[arg(long, help_heading = "ID", group = "id", value_name = "ID альбома")]
    album_id: Option<String>,

    #[arg(long, help_heading = "ID", group = "id", value_name = "ID трека")]
    track_id: Option<String>,

    #[arg(
        long,
        help_heading = "ID",
        group = "id",
        value_name = "владелец плейлиста/тип плейлиста"
    )]
    playlist_id: Option<String>,

    #[arg(
        short = 'u',
        long,
        help_heading = "ID",
        group = "id",
        help = "URL исполнителя/альбома/трека/плейлиста"
    )]
    url: Option<String>,

    #[arg(
        long,
        help_heading = "Указание пути",
        help = "Не очищать путь от недопустимых символов"
    )]
    unsafe_path: bool,

    #[arg(
        long,
        help_heading = "Указание пути",
        value_name = "Папка",
        default_value = ".",
        help = "Папка для загрузки музыки (по умолчанию: .)"
    )]
    dir: PathBuf,

    #[arg(
        long,
        help_heading = "Указание пути",
        value_name = "Паттерн",
        help = "Поддерживает следующие заполнители: #number, #track-artist, #album-artist, #title, #album, #year, #artist-id, #album-id, #track-id, #number-padded, #disc-number, #disc-number-padded (по умолчанию: #album-artist/#album/#number - #title; для VK — #track-artist/#title)"
    )]
    path_pattern: Option<PathBuf>,

    #[arg(
        value_name = "URL",
        help = "URL исполнителя/альбома/трека/плейлиста (можно передать без -u)"
    )]
    positional_url: Option<String>,

    #[arg(
        long,
        help_heading = "Авторизация",
        value_name = "Токен",
        help = "Токен Яндекс.Музыки. Если не указан, используется сохранённый токен или запускается OAuth Device Flow"
    )]
    token: Option<String>,

    #[arg(
        long,
        help_heading = "Авторизация",
        value_name = "Токен VK",
        help = "Токен Kate Mobile для ссылок vk.com/vk.ru. Если не указан, читается из MUSIC_DOWNLOADER_VK_TOKEN или сохранённого файла. Значение `-` читает токен из stdin"
    )]
    vk_token: Option<String>,

    #[arg(
        long,
        help_heading = "Авторизация",
        conflicts_with = "vk_token",
        help = "Интерактивно получить токен VK: открыть браузер, принять вставленный URL или токен, проверить и сохранить"
    )]
    vk_login: bool,
}

struct DownloadableTrack {
    download_info: CustomDownloadInfo,
    path: PathBuf,
    track: Track,
}

struct Downloader {
    args: Args,
    client: Client,
    covers_cache: HashMap<String, AlbumCover>,
    track_counter: i64,
    total_track_count: Option<i64>,
    progress_status: String,
}

fn main() -> ExitCode {
    let args = Args::parse();

    let level = if args.debug { "debug" } else { "error" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level))
        .format_timestamp(Some(env_logger::TimestampPrecision::Seconds))
        .init();

    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

fn obtain_token(client: &mut Client) -> Result<String> {
    let code = client
        .request_device_code()
        .map_err(|error| anyhow!("{error}"))?;
    println!(
        "Открой {} и введи код: {}",
        code.verification_url, code.user_code
    );

    let deadline = Instant::now() + Duration::from_secs(code.expires_in.max(60));
    let interval = Duration::from_secs(code.interval.max(1));

    while Instant::now() < deadline {
        match client.poll_device_token(&code.device_code) {
            Ok(Some(token)) => return Ok(token),
            Ok(None) => {}
            Err(error) => return Err(anyhow!("{error}")),
        }
        std::thread::sleep(interval);
    }

    bail!("Таймаут ожидания подтверждения входа")
}

fn load_saved_token() -> Option<String> {
    let path = token_path()?;
    let token = std::fs::read_to_string(path).ok()?;
    let token = token.trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn save_token(token: &str) {
    if let Some(path) = token_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(&path, token) {
            eprintln!("Не удалось сохранить токен в {}: {error}", path.display());
        } else {
            println!("Токен сохранён в {}", path.display());
        }
    }
}

fn token_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("MUSIC_DOWNLOADER_TOKEN_FILE") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }

    let base = if let Ok(value) = std::env::var("XDG_CONFIG_HOME") {
        if !value.is_empty() {
            Some(PathBuf::from(value))
        } else {
            None
        }
    } else {
        None
    };

    let base = base.or_else(|| {
        #[cfg(target_os = "windows")]
        {
            std::env::var("APPDATA").ok().map(PathBuf::from)
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".config"))
        }
    });

    base.map(|path| path.join("music-downloader").join("token"))
}

fn run(mut args: Args) -> Result<()> {
    if args.add_lyrics {
        println!(
            "Аргумент --add-lyrics устарел и будет удален в будущем. Используйте --lyrics-format"
        );
        args.lyrics_format = LyricsFormat::Text;
    }

    if args.url.is_none() {
        args.url = args.positional_url.take();
    }

    if args.artist_id.is_none()
        && args.album_id.is_none()
        && args.track_id.is_none()
        && args.playlist_id.is_none()
        && args.url.is_none()
    {
        if args.vk_login {
            vk_login(&args)?;
            return Ok(());
        }
        bail!("Не указан ID или URL. Используйте --help");
    }

    if let Some(url) = args.url.clone() {
        if let Some(audio_id) = vk::parse_audio_link(&url) {
            return run_vk(args, audio_id);
        }
        resolve_url(&mut args, &url)?;
    }

    let token = args.token.clone().unwrap_or_default();
    let mut client = Client::new(
        token,
        args.timeout,
        u32::try_from(args.tries).unwrap_or(u32::MAX),
        args.retry_delay,
    );

    let token = if let Some(token) = args.token.clone().filter(|token| !token.is_empty()) {
        save_token(&token);
        token
    } else if let Some(token) = load_saved_token() {
        println!("Используется сохранённый токен");
        token
    } else {
        let token = obtain_token(&mut client)?;
        save_token(&token);
        println!("access_token: {token}");
        token
    };

    client.token = token;

    api::account_status(&client)?;

    args.path_pattern
        .get_or_insert_with(|| PathBuf::from(DEFAULT_PATH_PATTERN));

    let mut downloader = Downloader {
        args,
        client,
        covers_cache: HashMap::new(),
        track_counter: 0,
        total_track_count: None,
        progress_status: String::new(),
    };
    downloader.run()
}

fn run_vk(args: Args, audio_id: AudioId) -> Result<()> {
    if args.lyrics_format != LyricsFormat::None {
        println!("Тексты песен для VK не поддерживаются, флаг --lyrics-format игнорируется");
    }

    let token = resolve_vk_token(&args)?;
    let client = vk::VkClient::new(
        token,
        args.timeout,
        u32::try_from(args.tries).unwrap_or(u32::MAX),
        args.retry_delay,
    );

    let audio = client
        .get_by_id(audio_id)
        .map_err(|error| anyhow!("{error}"))?;
    let download_url = audio.download_url()?;
    let track = audio.to_track(audio_id);

    let pattern = args
        .path_pattern
        .as_deref()
        .unwrap_or_else(|| Path::new(DEFAULT_VK_PATH_PATTERN));
    let base_path = args
        .dir
        .join(prepare_base_path(pattern, &track, args.unsafe_path));

    if args.skip_existing && existing_file(&base_path) {
        return Ok(());
    }

    let mut target = base_path.as_os_str().to_os_string();
    target.push(".mp3");
    let target_path = PathBuf::from(target);

    if let Some(parent) = target_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        if !parent.is_dir() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Не удалось создать папку {}", parent.display()))?;
        }
    }

    println!("[1/1] [MP3] Загружается {}", target_path.display());
    let data = client
        .download(&download_url)
        .map_err(|error| anyhow!("{error}"))?;

    let source_url = vk::track_url(audio_id);
    let compatibility_level = i64::from(args.compatibility_level);
    write_via_temporary_file(
        &data,
        &target_path,
        Some(&|temporary_path: &Path| {
            set_tags_with_url(
                temporary_path,
                &track,
                Container::Mp3,
                None,
                None,
                compatibility_level,
                &source_url,
            )
        }),
    )?;

    Ok(())
}

fn resolve_vk_token(args: &Args) -> Result<String> {
    if args.vk_login {
        return vk_login(args);
    }

    if let Some(raw) = args
        .vk_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let raw = if raw == "-" {
            read_line("")?
        } else {
            raw.to_string()
        };
        let token = vk::extract_access_token(&raw).ok_or_else(|| {
            anyhow!("Не удалось разобрать VK-токен: ожидается access_token или URL с ним")
        })?;
        save_vk_token(&token);
        return Ok(token);
    }

    if let Ok(value) = std::env::var("MUSIC_DOWNLOADER_VK_TOKEN") {
        let value = value.trim();
        if !value.is_empty() {
            let token = vk::extract_access_token(value).unwrap_or_else(|| value.to_string());
            return Ok(token);
        }
    }

    if let Some(token) = load_saved_vk_token() {
        println!("Используется сохранённый VK-токен");
        return Ok(token);
    }

    // Токена нет: если можем спросить пользователя, сразу запускаем тот же
    // интерактивный вход, что и `--vk-login`, — иначе он просто увидел бы
    // инструкцию, открыл браузер руками и запустил программу ещё раз.
    // Без терминала (скрипт, CI, перенаправленный stdin) спрашивать некого,
    // поэтому там остаётся сообщение с инструкцией.
    if io::stdin().is_terminal() {
        println!("Токен VK не найден — запускаю интерактивный вход (--vk-login).");
        return vk_login(args);
    }

    let location = vk_token_path()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "~/.config/music-downloader/vk_token".to_string());
    bail!(
        "Для VK нужен токен Kate Mobile. Запустите с --vk-login, чтобы получить и сохранить его \
         интерактивно, либо получите токен на https://vkhost.github.io (Kate Mobile) и передайте \
         через --vk-token, переменную окружения MUSIC_DOWNLOADER_VK_TOKEN или сохраните в {location}"
    )
}

/// Ссылка, где можно получить токен Kate Mobile.
const VK_TOKEN_URL: &str = "https://vkhost.github.io/";

/// Полуавтоматическое получение токена VK.
///
/// Полностью автоматический OAuth, как у Яндекс.Музыки, для VK невозможен:
/// методов `audio.*` нет у сторонних приложений, а device flow VK не
/// поддерживает. Поэтому пользователь проходит авторизацию в браузере сам, а
/// программа забирает результат, проверяет токен и сохраняет его.
fn vk_login(args: &Args) -> Result<String> {
    println!("VK не выдаёт доступ к музыке сторонним приложениям, поэтому вход нельзя");
    println!("сделать полностью автоматическим, как у Яндекс.Музыки.");
    println!();
    println!("1. Откройте {VK_TOKEN_URL} и выберите Kate Mobile.");
    println!("2. Разрешите доступ и скопируйте адрес страницы целиком");
    println!("   (https://oauth.vk.com/blank.html#access_token=...) или сам access_token.");
    if !browser_disabled() {
        if let Err(error) = open_browser(VK_TOKEN_URL) {
            println!("Не удалось открыть браузер: {error}");
            println!("Откройте ссылку вручную: {VK_TOKEN_URL}");
        }
    }
    println!();

    let input = read_line("Вставьте URL или access_token: ")?;
    let token = vk::extract_access_token(&input).ok_or_else(|| {
        anyhow!("Это не похоже на VK-токен: ожидается access_token или URL с access_token=")
    })?;

    let client = vk::VkClient::new(
        token.clone(),
        args.timeout,
        u32::try_from(args.tries).unwrap_or(u32::MAX),
        args.retry_delay,
    );
    let user = client
        .current_user()
        .map_err(|error| anyhow!("Токен не подошёл: {error}"))?;
    println!("Токен действителен, аккаунт: {}", user.display_name());

    save_vk_token(&token);
    Ok(token)
}

/// Читает одну строку из stdin; пустое приглашение отключает подсказку.
fn read_line(prompt: &str) -> Result<String> {
    if !prompt.is_empty() {
        eprint!("{prompt}");
        io::stderr()
            .flush()
            .context("Не удалось вывести приглашение")?;
    }

    let mut buffer = String::new();
    io::stdin()
        .read_line(&mut buffer)
        .context("Не удалось прочитать строку из stdin")?;

    let line = buffer.trim();
    if line.is_empty() {
        bail!("Пустой ввод");
    }
    Ok(line.to_string())
}

/// Переменная окружения, отключающая автоматическое открытие браузера
/// (нужна тестам и неинтерактивным запускам).
const NO_BROWSER_ENV: &str = "MUSIC_DOWNLOADER_NO_BROWSER";

/// Сколько ждать завершения команды открытия, чтобы отличить «браузер уже
/// запускается» от «команда упала» (нет `xdg-open`, битая ассоциация протокола).
const BROWSER_PROBE: Duration = Duration::from_millis(800);

fn browser_disabled() -> bool {
    match std::env::var(NO_BROWSER_ENV) {
        Ok(value) => !value.is_empty() && value != "0",
        Err(_) => false,
    }
}

/// Открывает URL в браузере по умолчанию.
///
/// Способы перебираются по очереди, пока какой-нибудь не запустится: на
/// Windows это `cmd /C start` (а затем `rundll32`), на macOS — `open`, на
/// остальных Unix-системах — `xdg-open`, `gio open`, `wslview` и
/// `sensible-browser`. Ошибка возвращается, только если не сработал ни один
/// способ, — тогда ссылку нужно открыть вручную.
fn open_browser(url: &str) -> Result<()> {
    let candidates = browser_commands(url);
    let mut errors = Vec::new();
    for mut command in candidates {
        let program = command.get_program().to_string_lossy().into_owned();
        match launch_browser(&mut command) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(format!("{program}: {error}")),
        }
    }

    bail!("ни один способ не сработал ({})", errors.join("; "))
}

#[cfg(target_os = "windows")]
fn browser_commands(url: &str) -> Vec<Command> {
    vec![
        // `start` — встроенная команда cmd. Пустой первый аргумент — заголовок
        // окна: без него `start` принимает URL за заголовок и ничего не открывает.
        command("cmd", &["/C", "start", "", url]),
        command("rundll32", &["url.dll,FileProtocolHandler", url]),
    ]
}

#[cfg(target_os = "macos")]
fn browser_commands(url: &str) -> Vec<Command> {
    vec![command("open", &[url])]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn browser_commands(url: &str) -> Vec<Command> {
    vec![
        command("xdg-open", &[url]),
        command("gio", &["open", url]),
        // WSL: без wslview ссылка откроется в браузере Windows.
        command("wslview", &[url]),
        command("sensible-browser", &[url]),
    ]
}

#[cfg(not(any(
    target_os = "windows",
    target_os = "macos",
    all(unix, not(target_os = "macos"))
)))]
fn browser_commands(_url: &str) -> Vec<Command> {
    Vec::new()
}

fn command(program: &str, arguments: &[&str]) -> Command {
    let mut command = Command::new(program);
    command.args(arguments);
    command
}

/// Запускает команду открытия и коротко ждёт результат: если процесс успел
/// завершиться с ошибкой, пробуем следующий способ. Долго работающий процесс
/// (сам браузер) считаем успехом и не ждём его.
fn launch_browser(command: &mut Command) -> Result<()> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("не удалось запустить команду")?;

    let deadline = Instant::now() + BROWSER_PROBE;
    loop {
        match child.try_wait().context("не удалось дождаться команды")? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => bail!("команда завершилась с ошибкой: {status}"),
            None if Instant::now() >= deadline => return Ok(()),
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

fn load_saved_vk_token() -> Option<String> {
    let token = std::fs::read_to_string(vk_token_path()?).ok()?;
    let token = token.trim().to_string();
    (!token.is_empty()).then_some(token)
}

fn save_vk_token(token: &str) {
    let Some(path) = vk_token_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match write_secret(&path, token) {
        Ok(()) => println!("VK-токен сохранён в {}", path.display()),
        Err(error) => eprintln!(
            "Не удалось сохранить VK-токен в {}: {error}",
            path.display()
        ),
    }
}

fn vk_token_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("MUSIC_DOWNLOADER_VK_TOKEN_FILE") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    token_path().map(|path| path.with_file_name("vk_token"))
}

fn write_secret(path: &Path, contents: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }

    let mut file = options.open(path)?;
    file.write_all(contents.as_bytes())
}

fn resolve_url(args: &mut Args, url: &str) -> Result<()> {
    static ARTIST_RE: OnceLock<Regex> = OnceLock::new();
    static ALBUM_RE: OnceLock<Regex> = OnceLock::new();
    static TRACK_RE: OnceLock<Regex> = OnceLock::new();
    static PLAYLIST_RE: OnceLock<Regex> = OnceLock::new();

    let path = url::Url::parse(url)
        .map(|url| url.path().to_string())
        .unwrap_or_else(|_| url.to_string());

    let artist_regex = ARTIST_RE.get_or_init(|| Regex::new(r"artist/(\d+)$").unwrap());
    let album_regex = ALBUM_RE.get_or_init(|| Regex::new(r"album/(\d+)$").unwrap());
    let track_regex = TRACK_RE.get_or_init(|| Regex::new(r"track/(\d+)").unwrap());
    let playlist_regex = PLAYLIST_RE.get_or_init(|| Regex::new(r"playlists/(.+)$").unwrap());

    if let Some(captures) = artist_regex.captures(&path) {
        args.artist_id = Some(captures[1].to_string());
    } else if let Some(captures) = album_regex.captures(&path) {
        args.album_id = Some(captures[1].to_string());
    } else if let Some(captures) = track_regex.captures(&path) {
        args.track_id = Some(captures[1].to_string());
    } else if let Some(captures) = playlist_regex.captures(&path) {
        args.playlist_id = Some(captures[1].to_string());
    } else {
        bail!("Параметер url указан в неверном формате");
    }
    Ok(())
}

impl Downloader {
    fn run(&mut self) -> Result<()> {
        if let Some(artist_id) = self.args.artist_id.clone() {
            self.run_artist(&artist_id)
        } else if let Some(album_id) = self.args.album_id.clone() {
            self.run_album(&album_id)
        } else if let Some(track_id) = self.args.track_id.clone() {
            self.run_track(&track_id)
        } else if let Some(playlist_id) = self.args.playlist_id.clone() {
            self.run_playlist(&playlist_id)
        } else {
            bail!("Invalid ID argument");
        }
    }

    fn run_artist(&mut self, artist_id: &str) -> Result<()> {
        let artists = api::artists(&self.client, artist_id)?;
        let artist = artists
            .first()
            .ok_or_else(|| anyhow!("Исполнитель {artist_id} не найден"))?;
        self.total_track_count = artist.counts.as_ref().and_then(|counts| counts.tracks);

        let mut page: i64 = 0;
        while let Some(albums_info) =
            api::artist_albums(&self.client, artist_id, page.max(0) as u32)?
        {
            for album in &albums_info.albums {
                if self.filter_album(album, artist_id)? {
                    let Some(album_id) = album.id.clone() else {
                        continue;
                    };
                    for track in self.album_tracks(&album_id)? {
                        self.process_track(track)?;
                    }
                } else if let Some(count) = album.track_count.filter(|count| *count != 0) {
                    if let Some(total) = self.total_track_count.as_mut() {
                        *total -= count;
                    }
                }
            }

            let Some(pager) = albums_info.pager else {
                break;
            };
            page = pager.page + 1;
            if pager.per_page <= 0 || pager.per_page * page >= pager.total {
                break;
            }
        }
        Ok(())
    }

    fn filter_album(&self, album: &Album, artist_id: &str) -> Result<bool> {
        let title = album.title.clone().unwrap_or_else(|| "None".to_string());

        if album.id.is_none() || !album.available.unwrap_or(false) {
            println!("Альбом \"{title}\" не доступен для скачивания");
            return Ok(false);
        }
        if self.args.only_music && album.meta_type.as_deref() != Some("music") {
            println!("Альбом \"{title}\" пропущен т.к. не является музыкальным");
            return Ok(false);
        }
        if self.args.stick_to_artist
            && album
                .artists
                .first()
                .and_then(|artist| artist.id.as_deref())
                != Some(artist_id)
        {
            println!("Альбом \"{title}\" пропущен из-за флага --stick-to-artist");
            return Ok(false);
        }

        Ok(true)
    }

    fn run_album(&mut self, album_id: &str) -> Result<()> {
        if let Some(album) = api::album_with_tracks(&self.client, album_id)? {
            self.total_track_count = album.track_count;
        }
        for track in self.album_tracks(album_id)? {
            self.process_track(track)?;
        }
        Ok(())
    }

    fn run_track(&mut self, track_id: &str) -> Result<()> {
        let tracks = api::tracks(&self.client, &[track_id.to_string()])?;
        self.total_track_count = Some(1);
        for track in tracks {
            self.process_track(track)?;
        }
        Ok(())
    }

    fn run_playlist(&mut self, playlist_id: &str) -> Result<()> {
        let playlist = api::playlist(&self.client, playlist_id)?
            .ok_or_else(|| anyhow!("Плейлист {playlist_id} не найден"))?;
        self.total_track_count = playlist.track_count;

        let full = api::playlist_with_tracks(&self.client, &playlist)?;
        let ids: Vec<String> = full
            .tracks
            .iter()
            .filter_map(|track| track.id.clone())
            .collect();

        for chunk in ids.chunks(api::PLAYLIST_FETCH_PAGE_SIZE) {
            let tracks = api::tracks(&self.client, chunk)?;
            for track in tracks {
                self.process_track(track)?;
            }
        }
        Ok(())
    }

    fn album_tracks(&self, album_id: &str) -> Result<Vec<Track>> {
        let Some(album) = api::album_with_tracks(&self.client, album_id)? else {
            return Ok(Vec::new());
        };
        let Some(volumes) = album.volumes.clone() else {
            return Ok(Vec::new());
        };
        if volumes.is_empty() {
            return Ok(Vec::new());
        }

        let mut result = Vec::new();
        for volume in &volumes {
            for track in volume {
                let mut track = track.clone();
                if let Some(first_album) = track.albums.first_mut() {
                    first_album.volumes = Some(volumes.clone());
                }
                result.push(track);
            }
        }
        Ok(result)
    }

    fn process_track(&mut self, track: Track) -> Result<()> {
        if self.total_track_count.is_some_and(|total| total != 0) {
            self.track_counter += 1;
            self.progress_status = format!(
                "[{}/{}] ",
                self.track_counter,
                self.total_track_count.unwrap_or_default()
            );
        }

        if !track.available.unwrap_or(false) {
            println!(
                "{}Трек {} - {} не доступен для скачивания",
                self.progress_status,
                get_artists(&track),
                track.title.clone().unwrap_or_else(|| "None".to_string())
            );
            return Ok(());
        }

        let pattern = self
            .args
            .path_pattern
            .as_deref()
            .unwrap_or_else(|| Path::new(DEFAULT_PATH_PATTERN));
        let save_path =
            self.args
                .dir
                .join(prepare_base_path(pattern, &track, self.args.unsafe_path));

        if self.args.skip_existing && existing_file(&save_path) {
            return Ok(());
        }

        if let Some(save_dir) = save_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            if !save_dir.is_dir() {
                std::fs::create_dir_all(save_dir)
                    .with_context(|| format!("Не удалось создать папку {}", save_dir.display()))?;
            }
        }

        let quality = match self.args.quality {
            0 => CoreQuality::Low,
            1 => CoreQuality::Normal,
            _ => CoreQuality::Lossless,
        };
        let downloadable = to_downloadable_track(&self.client, track, quality, &save_path)?;
        let bitrate = downloadable.download_info.bitrate;
        let mut format_info = format!("[{}", downloadable.download_info.file_format.codec.name());
        if bitrate > 0 {
            format_info.push_str(&format!(" {bitrate}kbps"));
        }
        format_info.push(']');

        println!(
            "{}{} Загружается {}",
            self.progress_status,
            format_info,
            downloadable.path.display()
        );

        self.download_track(downloadable)?;

        if self.args.delay > 0 {
            std::thread::sleep(Duration::from_secs(self.args.delay));
        }
        Ok(())
    }

    fn download_track(&mut self, track_info: DownloadableTrack) -> Result<()> {
        let target_path = track_info.path.clone();
        let track = track_info.track.clone();

        let mut text_lyrics = None;
        if self.args.lyrics_format != LyricsFormat::None {
            if let Some(lyrics_info) = track.lyrics_info.as_ref() {
                if self.args.lyrics_format == LyricsFormat::Lrc
                    && lyrics_info.has_available_sync_lyrics
                {
                    let lrc_path = target_path.with_extension("lrc");
                    if !lrc_path.is_file() {
                        if let Some(lyrics) = api::track_lyrics(&self.client, &track, "LRC")? {
                            let text = api::fetch_lyrics_text(&self.client, &lyrics)?;
                            write_via_temporary_file(text.as_bytes(), &lrc_path, None)?;
                        }
                    }
                } else if lyrics_info.has_available_text_lyrics {
                    if let Some(lyrics) = api::track_lyrics(&self.client, &track, "TEXT")? {
                        text_lyrics = Some(api::fetch_lyrics_text(&self.client, &lyrics)?);
                    }
                }
            }
        }

        let mut cover = None;
        if track.cover_uri.is_some() {
            let cover_size = if self.args.cover_resolution == -1 {
                "orig".to_string()
            } else {
                format!(
                    "{}x{}",
                    self.args.cover_resolution, self.args.cover_resolution
                )
            };
            let url = api::cover_url(&self.client, &track, &cover_size)
                .ok_or_else(|| anyhow!("У трека нет обложки"))?;
            let cover_bytes = self.client.fetch_bytes(&url)?;
            let mime_type =
                guess_mime_type(&cover_bytes).ok_or_else(|| anyhow!("Unknown cover mime type"))?;
            let album_cover = AlbumCover {
                data: cover_bytes,
                mime_type,
            };

            if self.args.embed_cover {
                let album_id = track
                    .albums
                    .first()
                    .and_then(|album| album.id.clone())
                    .filter(|id| !id.is_empty());
                if let Some(album_id) = album_id {
                    if let Some(cached) = self.covers_cache.get(&album_id) {
                        cover = Some(cached.clone());
                    } else {
                        self.covers_cache.insert(album_id, album_cover.clone());
                        cover = Some(album_cover);
                    }
                }
            } else {
                let cover_path = target_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(format!("cover{}", mime_type.suffix()));
                if !cover_path.is_file() {
                    write_via_temporary_file(&album_cover.data, &cover_path, None)?;
                }
            }
        }

        let track_data = api::download_track(&self.client, &track_info.download_info)
            .map_err(|error| anyhow!("{error}"))?;

        let container = track_info.download_info.file_format.container;
        let lyrics = text_lyrics.as_deref();
        let compatibility_level = i64::from(self.args.compatibility_level);
        write_via_temporary_file(
            &track_data,
            &target_path,
            Some(&|temporary_path: &Path| {
                set_tags(
                    temporary_path,
                    &track,
                    container,
                    lyrics,
                    cover.as_ref(),
                    compatibility_level,
                )
            }),
        )?;

        Ok(())
    }
}

fn get_artists(track: &Track) -> String {
    track
        .artists
        .iter()
        .map(|artist| artist.name.clone().unwrap_or_default())
        .collect::<Vec<_>>()
        .join(", ")
}

fn existing_file(base_path: &Path) -> bool {
    AUDIO_FILE_SUFFIXES.iter().any(|suffix| {
        let mut path = base_path.as_os_str().to_os_string();
        path.push(suffix);
        PathBuf::from(path).is_file()
    })
}

fn to_downloadable_track(
    client: &Client,
    track: Track,
    quality: CoreQuality,
    base_path: &Path,
) -> Result<DownloadableTrack> {
    let download_info = api::get_download_info(client, &track, quality.api_quality())
        .map_err(|error| anyhow!("{error}"))?;
    let suffix = match download_info.file_format.container {
        Container::Mp3 => ".mp3",
        Container::Mp4 => ".m4a",
        Container::Flac => ".flac",
    };

    let mut target = base_path.as_os_str().to_os_string();
    target.push(suffix);
    Ok(DownloadableTrack {
        download_info,
        path: PathBuf::from(target),
        track,
    })
}

type TemporaryFileHook<'a> = &'a dyn Fn(&Path) -> Result<()>;

fn write_via_temporary_file(
    data: &[u8],
    target_path: &Path,
    hook: Option<TemporaryFileHook<'_>>,
) -> Result<PathBuf> {
    let target_name = target_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let hash = hex::encode(Sha256::digest(target_name.as_bytes()));
    let temporary_name = TEMPORARY_FILE_NAME_TEMPLATE.replace("{}", &hash);
    let temporary_file = target_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(temporary_name);

    let result = (|| -> Result<()> {
        let mut file = std::fs::File::create(&temporary_file)
            .with_context(|| format!("Не удалось создать {}", temporary_file.display()))?;
        file.write_all(data)
            .with_context(|| format!("Не удалось записать {}", temporary_file.display()))?;
        drop(file);

        if let Some(hook) = hook {
            hook(&temporary_file)?;
        }
        rename_overwrite(&temporary_file, target_path)
    })();

    if result.is_err() && temporary_file.exists() {
        let _ = std::fs::remove_file(&temporary_file);
    }
    result?;
    Ok(target_path.to_path_buf())
}

fn rename_overwrite(source: &Path, target: &Path) -> Result<()> {
    match std::fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(error) if target.exists() => {
            std::fs::remove_file(target)
                .with_context(|| format!("Не удалось удалить {}", target.display()))?;
            std::fs::rename(source, target).with_context(|| {
                format!(
                    "Не удалось переименовать {} в {}: {error}",
                    source.display(),
                    target.display()
                )
            })
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "Не удалось переименовать {} в {}",
                source.display(),
                target.display()
            )
        }),
    }
}
