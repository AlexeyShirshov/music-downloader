use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use clap::{ArgGroup, Parser, ValueEnum};
use regex::Regex;
use sha2::{Digest, Sha256};

use yandex_music_downloader::api::{self, Container, CustomDownloadInfo};
use yandex_music_downloader::client::Client;
use yandex_music_downloader::mime::guess_mime_type;
use yandex_music_downloader::models::{Album, Track};
use yandex_music_downloader::path_pattern::{prepare_base_path, DEFAULT_PATH_PATTERN};
use yandex_music_downloader::tagging::{set_tags, AlbumCover};

const AUDIO_FILE_SUFFIXES: [&str; 3] = [".mp3", ".flac", ".m4a"];
const TEMPORARY_FILE_NAME_TEMPLATE: &str = ".yandex-music-downloader.{}.tmp";

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
    name = "ymd",
    about = "Загрузчик музыки с сервиса Яндекс.Музыка",
    group(ArgGroup::new("id").required(true).multiple(false).args([
        "artist_id", "album_id", "track_id", "playlist_id", "url"
    ]))
)]
struct Args {
    #[arg(
        long,
        help_heading = "Общие параметры",
        value_name = "Качество",
        default_value_t = 0,
        value_parser = clap::value_parser!(u8).range(0..=2),
        help = "Качество трека:\n0 - Низкое (AAC 64kbps)\n1 - Оптимальное (AAC 192kbps)\n2 - Лучшее (FLAC)\n(по умолчанию: 0)"
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
        default_value = DEFAULT_PATH_PATTERN,
        help = "Поддерживает следующие заполнители: #number, #track-artist, #album-artist, #title, #album, #year, #artist-id, #album-id, #track-id, #number-padded, #disc-number, #disc-number-padded (по умолчанию: #album-artist/#album/#number - #title)"
    )]
    path_pattern: PathBuf,

    #[arg(
        long,
        help_heading = "Авторизация",
        value_name = "Токен",
        help = "Токен для авторизации. Если не указан, используется сохранённый токен или запускается OAuth Device Flow"
    )]
    token: Option<String>,
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
    if let Ok(path) = std::env::var("YMD_TOKEN_FILE") {
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

    base.map(|path| path.join("ymd").join("token"))
}

fn run(mut args: Args) -> Result<()> {
    if args.add_lyrics {
        println!(
            "Аргумент --add-lyrics устарел и будет удален в будущем. Используйте --lyrics-format"
        );
        args.lyrics_format = LyricsFormat::Text;
    }

    if let Some(url) = args.url.clone() {
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

        let save_path = self.args.dir.join(prepare_base_path(
            &self.args.path_pattern,
            &track,
            self.args.unsafe_path,
        ));

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
