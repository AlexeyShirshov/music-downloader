use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use lofty::file::TaggedFileExt;
use serde_json::json;

/// Минимальный HTTP-сервер на std: отдаёт заранее заданные ответы API.
struct MockServer {
    address: SocketAddr,
    _listener: Arc<TcpListener>,
}

impl MockServer {
    fn start(cover: Vec<u8>, audio: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let address = listener.local_addr().unwrap();
        let listener = Arc::new(listener);
        let server_listener = Arc::clone(&listener);
        let server_address = address;

        std::thread::spawn(move || {
            for stream in server_listener.incoming() {
                let Ok(stream) = stream else { continue };
                let cover = cover.clone();
                let audio = audio.clone();
                std::thread::spawn(move || {
                    handle(stream, server_address, &cover, &audio);
                });
            }
        });

        Self {
            address,
            _listener: listener,
        }
    }

    fn api_base(&self) -> String {
        format!("http://{}", self.address)
    }
}

fn handle(mut stream: TcpStream, address: SocketAddr, cover: &[u8], audio: &[u8]) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let full_path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let path = full_path.split('?').next().unwrap_or("/");

    let track = json!({
        "id": "1",
        "title": "Song",
        "available": true,
        "artists": [{"id": 20, "name": "Track Artist"}],
        "albums": [{
            "id": 1,
            "title": "Album",
            "year": 2001,
            "releaseDate": "2020-03-04T05:06:07+03:00",
            "genre": "rock",
            "trackCount": 1,
            "artists": [{"id": 10, "name": "Album Artist"}],
            "trackPosition": {"volume": 1, "index": 1}
        }],
        "coverUri": format!("{address}/cover/%%"),
        "lyricsInfo": {
            "hasAvailableSyncLyrics": true,
            "hasAvailableTextLyrics": true
        }
    });

    let mut second_track = track.clone();
    second_track["id"] = json!("2");
    second_track["title"] = json!("Song 2");
    second_track["albums"][0]["trackPosition"]["index"] = json!(2);

    let (status, content_type, body): (u16, &str, Vec<u8>) = match path {
        "/account/status" => (
            200,
            "application/json",
            json!({"result": {"account": {"uid": 1}}})
                .to_string()
                .into_bytes(),
        ),
        "/artists" => (
            200,
            "application/json",
            json!({"result": [{
                "id": "208167",
                "name": "Artist",
                "counts": {"tracks": 1, "directAlbums": 1, "alsoAlbums": 0, "alsoTracks": 0}
            }]})
            .to_string()
            .into_bytes(),
        ),
        "/artists/208167/direct-albums" => (
            200,
            "application/json",
            json!({
                "result": {
                    "pager": {"page": 0, "perPage": 20, "total": 1},
                    "albums": [{
                        "id": 1,
                        "title": "Album",
                        "metaType": "music",
                        "available": true,
                        "trackCount": 1,
                        "artists": [{"id": 208167, "name": "Artist"}]
                    }]
                }
            })
            .to_string()
            .into_bytes(),
        ),
        "/albums/1/with-tracks" => (
            200,
            "application/json",
            json!({
                "result": {
                    "id": 1,
                    "title": "Album",
                    "year": 2001,
                    "releaseDate": "2020-03-04T05:06:07+03:00",
                    "genre": "rock",
                    "trackCount": 1,
                    "artists": [{"id": 10, "name": "Album Artist"}],
                    "volumes": [[track]]
                }
            })
            .to_string()
            .into_bytes(),
        ),
        "/tracks" => {
            let tracks = if full_path.contains("1,2") || full_path.contains("1%2C2") {
                vec![track.clone(), second_track.clone()]
            } else {
                vec![track.clone()]
            };
            (
                200,
                "application/json",
                json!({"result": tracks}).to_string().into_bytes(),
            )
        }
        "/get-file-info" => (
            200,
            "application/json",
            json!({
                "result": {
                    "downloadInfo": {
                        "trackId": "1",
                        "quality": "lq",
                        "codec": "mp3",
                        "bitrate": 64,
                        "transport": "encraw",
                        "url": format!("http://{address}/file"),
                        "urls": [format!("http://{address}/file")]
                    }
                }
            })
            .to_string()
            .into_bytes(),
        ),
        "/file" => (200, "audio/mpeg", audio.to_vec()),
        "/method/audio.getById" => (
            200,
            "application/json",
            json!({
                "response": [{
                    "id": 36370911,
                    "owner_id": -2001370911,
                    "artist": "VK Artist",
                    "title": "VK Song",
                    "duration": 215,
                    "url": format!("http://{address}/vkfile")
                }]
            })
            .to_string()
            .into_bytes(),
        ),
        "/method/users.get" => (
            200,
            "application/json",
            json!({
                "response": [{
                    "id": 42,
                    "first_name": "Тестовый",
                    "last_name": "Аккаунт",
                    "screen_name": "test"
                }]
            })
            .to_string()
            .into_bytes(),
        ),
        "/vkfile" => (200, "audio/mpeg", audio.to_vec()),
        "/lyrics.txt" => (200, "text/plain", b"line1\nline2".to_vec()),
        "/lyrics.lrc" => (
            200,
            "text/plain",
            b"[00:00.00] line1\n[00:01.00] line2".to_vec(),
        ),
        _ if path == "/tracks/1/lyrics" && full_path.contains("format=LRC") => (
            200,
            "application/json",
            json!({
                "result": {
                    "downloadUrl": format!("http://{address}/lyrics.lrc"),
                    "lyricId": 1,
                    "externalLyricId": "test",
                    "writers": []
                }
            })
            .to_string()
            .into_bytes(),
        ),
        _ if path == "/tracks/1/lyrics" => (
            200,
            "application/json",
            json!({
                "result": {
                    "downloadUrl": format!("http://{address}/lyrics.txt"),
                    "lyricId": 1,
                    "externalLyricId": "test",
                    "writers": []
                }
            })
            .to_string()
            .into_bytes(),
        ),
        "/playlist/123" | "/users/1/playlists/123" => (
            200,
            "application/json",
            json!({
                "result": {
                    "uid": 1,
                    "kind": 123,
                    "title": "Playlist",
                    "trackCount": 2,
                    "owner": {"uid": 1, "login": "owner"},
                    "tracks": [{"id": "1"}, {"id": "2"}]
                }
            })
            .to_string()
            .into_bytes(),
        ),
        _ if path.starts_with("/cover/") => (200, "image/png", cover.to_vec()),
        _ => (
            404,
            "application/json",
            json!({"error": {"name": "not-found", "message": path}})
                .to_string()
                .into_bytes(),
        ),
    };

    let header = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn run_downloader(server: &MockServer, dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_music-downloader"))
        .env("MUSIC_DOWNLOADER_API_BASE_URL", server.api_base())
        .env("MUSIC_DOWNLOADER_COVER_BASE_URL", "http://")
        .env("MUSIC_DOWNLOADER_TOKEN_FILE", dir.join("token"))
        .env("MUSIC_DOWNLOADER_VK_TOKEN_FILE", dir.join("vk_token"))
        .env("RUST_LOG", "debug")
        .arg("--token")
        .arg("test-token")
        .arg("--dir")
        .arg(dir)
        .arg("--quality")
        .arg("0")
        .args(args)
        .output()
        .expect("run music-downloader")
}

fn fixtures() -> (Vec<u8>, Vec<u8>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio");
    (
        std::fs::read(root.join("cover.png")).unwrap(),
        std::fs::read(root.join("base.mp3")).unwrap(),
    )
}

#[test]
fn album_download_end_to_end() {
    let (cover, audio) = fixtures();
    let server = MockServer::start(cover, audio);
    let dir = tempfile::tempdir().unwrap();

    let output = run_downloader(&server, dir.path(), &["--album-id", "1"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Загружается") && stdout.contains("Album Artist/Album/1 - Song.mp3"),
        "{stdout}"
    );
    assert!(stdout.contains("[1/1]"), "{stdout}");

    let file = dir.path().join("Album Artist/Album/1 - Song.mp3");
    assert!(file.is_file(), "downloaded file exists");
    let tag = lofty::read_from_path(&file)
        .unwrap()
        .primary_tag()
        .unwrap()
        .clone();
    assert_eq!(
        tag.get_string(lofty::tag::ItemKey::TrackTitle),
        Some("Song")
    );
    assert!(
        tag.pictures().is_empty(),
        "cover is not embedded by default"
    );
    assert!(
        dir.path().join("Album Artist/Album/cover.png").is_file(),
        "cover file written next to track"
    );
}

#[test]
fn artist_download_end_to_end() {
    let (cover, audio) = fixtures();
    let server = MockServer::start(cover, audio);
    let dir = tempfile::tempdir().unwrap();

    let output = run_downloader(&server, dir.path(), &["--artist-id", "208167"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path().join("Album Artist/Album/1 - Song.mp3").is_file());
}

#[test]
fn text_lyrics_are_embedded() {
    let (cover, audio) = fixtures();
    let server = MockServer::start(cover, audio);
    let dir = tempfile::tempdir().unwrap();

    let output = run_downloader(
        &server,
        dir.path(),
        &["--album-id", "1", "--lyrics-format", "text"],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let file = dir.path().join("Album Artist/Album/1 - Song.mp3");
    let tag = lofty::read_from_path(&file)
        .unwrap()
        .primary_tag()
        .unwrap()
        .clone();
    let lyrics: Vec<&str> = tag.get_strings(lofty::tag::ItemKey::UnsyncLyrics).collect();
    assert_eq!(lyrics, vec!["line1\nline2"]);
}

#[test]
fn lrc_lyrics_are_written_to_a_file() {
    let (cover, audio) = fixtures();
    let server = MockServer::start(cover, audio);
    let dir = tempfile::tempdir().unwrap();

    let output = run_downloader(
        &server,
        dir.path(),
        &["--album-id", "1", "--lyrics-format", "lrc"],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let lrc = dir.path().join("Album Artist/Album/1 - Song.lrc");
    assert!(lrc.is_file());
    assert_eq!(
        std::fs::read_to_string(lrc).unwrap(),
        "[00:00.00] line1\n[00:01.00] line2"
    );

    // В режиме LRC текст не встраивается в теги.
    let file = dir.path().join("Album Artist/Album/1 - Song.mp3");
    let tag = lofty::read_from_path(&file)
        .unwrap()
        .primary_tag()
        .unwrap()
        .clone();
    assert_eq!(
        tag.get_strings(lofty::tag::ItemKey::UnsyncLyrics)
            .collect::<Vec<_>>(),
        Vec::<&str>::new()
    );
}

#[test]
fn cover_can_be_embedded() {
    let (cover, audio) = fixtures();
    let server = MockServer::start(cover, audio);
    let dir = tempfile::tempdir().unwrap();

    let output = run_downloader(&server, dir.path(), &["--album-id", "1", "--embed-cover"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let file = dir.path().join("Album Artist/Album/1 - Song.mp3");
    let tag = lofty::read_from_path(&file)
        .unwrap()
        .primary_tag()
        .unwrap()
        .clone();
    assert_eq!(tag.pictures().len(), 1);
    assert!(!dir.path().join("Album Artist/Album/cover.png").exists());
}

#[test]
fn playlist_download_end_to_end() {
    let (cover, audio) = fixtures();
    let server = MockServer::start(cover, audio);
    let dir = tempfile::tempdir().unwrap();

    let output = run_downloader(&server, dir.path(), &["--playlist-id", "1/123"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path().join("Album Artist/Album/1 - Song.mp3").is_file());
    assert!(dir
        .path()
        .join("Album Artist/Album/2 - Song 2.mp3")
        .is_file());
}

#[test]
fn skip_existing_does_not_download_twice() {
    let (cover, audio) = fixtures();
    let server = MockServer::start(cover, audio);
    let dir = tempfile::tempdir().unwrap();

    let first = run_downloader(&server, dir.path(), &["--album-id", "1"]);
    assert!(first.status.success());
    assert!(String::from_utf8_lossy(&first.stdout).contains("Загружается"));

    let second = run_downloader(&server, dir.path(), &["--album-id", "1", "--skip-existing"]);
    assert!(second.status.success());
    assert!(!String::from_utf8_lossy(&second.stdout).contains("Загружается"));
}

#[test]
fn vk_track_download_end_to_end() {
    let (cover, audio) = fixtures();
    let server = MockServer::start(cover, audio);
    let dir = tempfile::tempdir().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_music-downloader"))
        .env(
            "MUSIC_DOWNLOADER_VK_API_BASE_URL",
            format!("{}/method", server.api_base()),
        )
        .env(
            "MUSIC_DOWNLOADER_VK_TOKEN_FILE",
            dir.path().join("vk_token"),
        )
        .arg("--vk-token")
        .arg("test-vk-token")
        .arg("--dir")
        .arg(dir.path())
        .arg("https://vk.ru/audio-2001370911_36370911")
        .output()
        .expect("run music-downloader");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("VK Artist/VK Song.mp3"), "{stdout}");

    let file = dir.path().join("VK Artist/VK Song.mp3");
    assert!(file.is_file(), "downloaded VK track exists");

    let tag = lofty::read_from_path(&file)
        .unwrap()
        .primary_tag()
        .unwrap()
        .clone();
    assert_eq!(
        tag.get_string(lofty::tag::ItemKey::TrackTitle),
        Some("VK Song")
    );
    assert_eq!(
        tag.get_string(lofty::tag::ItemKey::TrackArtist),
        Some("VK Artist")
    );
}

#[test]
fn vk_login_accepts_pasted_url_and_saves_token() {
    let (cover, audio) = fixtures();
    let server = MockServer::start(cover, audio);
    let dir = tempfile::tempdir().unwrap();
    let token_file = dir.path().join("vk_token");

    let mut child = Command::new(env!("CARGO_BIN_EXE_music-downloader"))
        .env(
            "MUSIC_DOWNLOADER_VK_API_BASE_URL",
            format!("{}/method", server.api_base()),
        )
        .env("MUSIC_DOWNLOADER_VK_TOKEN_FILE", &token_file)
        .env("MUSIC_DOWNLOADER_NO_BROWSER", "1")
        .arg("--vk-login")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn music-downloader");

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"https://oauth.vk.com/blank.html#access_token=vk1.a.TEST_TOKEN&expires_in=0\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Тестовый Аккаунт"), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(&token_file).unwrap(),
        "vk1.a.TEST_TOKEN"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&token_file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
