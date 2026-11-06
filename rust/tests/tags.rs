use std::path::{Path, PathBuf};

use lofty::file::TaggedFileExt;
use lofty::picture::PictureType;
use lofty::tag::{ItemKey, Tag};
use yandex_music_downloader::api::Container;
use yandex_music_downloader::mime::MimeType;
use yandex_music_downloader::models::Track;
use yandex_music_downloader::tagging::{set_tags, AlbumCover};

const TRACK_JSON: &str = r#"{
    "id": 777,
    "title": "Song",
    "version": "Remix",
    "available": true,
    "artists": [
        {"id": 20, "name": "Track Artist"},
        {"id": 21, "name": "Second Artist"}
    ],
    "albums": [{
        "id": 555,
        "title": "Album",
        "version": "Deluxe",
        "year": 1999,
        "releaseDate": "2020-03-04T05:06:07+03:00",
        "genre": "rock",
        "trackCount": 12,
        "artists": [{"id": 10, "name": "Album Artist"}],
        "trackPosition": {"volume": 2, "index": 5}
    }]
}"#;

fn read_tag(path: &Path) -> Tag {
    lofty::read_from_path(path)
        .expect("audio file is readable")
        .primary_tag()
        .expect("file has a tag")
        .clone()
}

fn strings(tag: &Tag, key: ItemKey) -> Vec<String> {
    tag.get_strings(key).map(str::to_string).collect()
}

fn lyrics(tag: &Tag) -> Vec<String> {
    if tag.get(ItemKey::Lyrics).is_some() {
        strings(tag, ItemKey::Lyrics)
    } else {
        strings(tag, ItemKey::UnsyncLyrics)
    }
}

fn compare_common(expected: &Tag, actual: &Tag) {
    assert_eq!(
        strings(expected, ItemKey::TrackTitle),
        strings(actual, ItemKey::TrackTitle)
    );
    assert_eq!(
        strings(expected, ItemKey::AlbumTitle),
        strings(actual, ItemKey::AlbumTitle)
    );
    assert_eq!(
        strings(expected, ItemKey::AlbumArtist),
        strings(actual, ItemKey::AlbumArtist)
    );
    assert_eq!(
        strings(expected, ItemKey::RecordingDate),
        strings(actual, ItemKey::RecordingDate)
    );
    assert_eq!(
        strings(expected, ItemKey::TrackNumber),
        strings(actual, ItemKey::TrackNumber)
    );
    assert_eq!(
        strings(expected, ItemKey::DiscNumber),
        strings(actual, ItemKey::DiscNumber)
    );
    assert_eq!(
        strings(expected, ItemKey::Genre),
        strings(actual, ItemKey::Genre)
    );
    assert_eq!(lyrics(expected), lyrics(actual));
}

fn compare_picture(actual: &Tag, cover: &[u8]) {
    let pictures = actual.pictures();
    assert_eq!(pictures.len(), 1, "one embedded cover");
    let picture = &pictures[0];
    assert_eq!(picture.mime_type(), Some(&lofty::picture::MimeType::Png));
    assert_eq!(picture.data(), cover);
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/audio")
        .join(name)
}

struct Setup {
    temp: tempfile::TempDir,
}

impl Setup {
    fn new() -> Self {
        Self {
            temp: tempfile::tempdir().expect("temp dir"),
        }
    }

    fn tagged(
        &self,
        base: &str,
        name: &str,
        container: Container,
        compatibility_level: i64,
    ) -> (PathBuf, Track, Vec<u8>) {
        let cover = std::fs::read(fixture("cover.png")).expect("cover");
        self.tagged_with(
            base,
            name,
            container,
            compatibility_level,
            Some("line1\nline2"),
            Some(cover),
        )
    }

    fn tagged_with(
        &self,
        base: &str,
        name: &str,
        container: Container,
        compatibility_level: i64,
        lyrics: Option<&str>,
        cover: Option<Vec<u8>>,
    ) -> (PathBuf, Track, Vec<u8>) {
        let track: Track = serde_json::from_str(TRACK_JSON).expect("track json");
        let path = self.temp.path().join(name);
        std::fs::copy(fixture(base), &path).expect("base file");
        let album_cover = cover.as_ref().map(|data| AlbumCover {
            data: data.clone(),
            mime_type: MimeType::Png,
        });
        set_tags(
            &path,
            &track,
            container,
            lyrics,
            album_cover.as_ref(),
            compatibility_level,
        )
        .expect("tags written");
        (path, track, cover.unwrap_or_default())
    }
}

#[test]
fn mp3_tags_match_python_output() {
    let setup = Setup::new();
    let (path, _, cover) = setup.tagged("base.mp3", "test.mp3", Container::Mp3, 1);
    let actual = read_tag(&path);
    let expected = read_tag(&fixture("mut.mp3"));

    compare_common(&expected, &actual);
    assert_eq!(
        strings(&actual, ItemKey::TrackArtist),
        vec!["Track Artist".to_string(), "Second Artist".to_string()]
    );
    assert_eq!(
        strings(&actual, ItemKey::AlbumArtist),
        vec!["Album Artist".to_string()]
    );
    assert_eq!(actual.pictures()[0].pic_type(), PictureType::CoverFront);
    compare_picture(&actual, &cover);
    assert_eq!(lyrics(&actual), vec!["line1\nline2".to_string()]);

    // Python-версия пишет кадр WOAF (в mutagen он остался пустым);
    // в Rust-порте туда попадает настоящий URL трека.
    let raw = std::fs::read(&path).unwrap();
    assert!(raw.windows(4).any(|window| window == b"WOAF"));
    assert!(raw
        .windows(b"https://music.yandex.ru/album/555/track/777".len())
        .any(|window| window == b"https://music.yandex.ru/album/555/track/777"));
}

#[test]
fn flac_tags_match_python_output() {
    let setup = Setup::new();
    let (path, _, cover) = setup.tagged("base.flac", "test.flac", Container::Flac, 1);
    let actual = read_tag(&path);
    let expected = read_tag(&fixture("mut.flac"));

    compare_common(&expected, &actual);
    assert_eq!(
        strings(&actual, ItemKey::TrackArtist),
        vec!["Track Artist".to_string(), "Second Artist".to_string()]
    );
    assert_eq!(
        strings(&actual, ItemKey::AlbumArtist),
        vec!["Album Artist".to_string()]
    );
    assert_eq!(
        strings(&actual, ItemKey::Comment),
        vec!["https://music.yandex.ru/album/555/track/777".to_string()]
    );
    assert_eq!(actual.pictures()[0].pic_type(), PictureType::CoverFront);
    compare_picture(&actual, &cover);
    assert_eq!(lyrics(&actual), vec!["line1\nline2".to_string()]);
}

#[test]
fn m4a_tags_match_python_output_compatibility_1() {
    let setup = Setup::new();
    let (path, _, cover) = setup.tagged("base.m4a", "test.m4a", Container::Mp4, 1);
    let actual = read_tag(&path);
    let expected = read_tag(&fixture("mut.m4a"));

    compare_common(&expected, &actual);
    assert_eq!(
        strings(&actual, ItemKey::TrackArtist),
        vec!["Track Artist; Second Artist".to_string()]
    );
    assert_eq!(
        strings(&actual, ItemKey::AlbumArtist),
        vec!["Album Artist".to_string()]
    );
    assert_eq!(
        strings(&actual, ItemKey::Comment),
        vec!["https://music.yandex.ru/album/555/track/777".to_string()]
    );
    compare_picture(&actual, &cover);
    assert_eq!(lyrics(&actual), vec!["line1\nline2".to_string()]);
}

#[test]
fn m4a_tags_match_python_output_compatibility_0() {
    let setup = Setup::new();
    let (path, _, _) = setup.tagged_with("base.m4a", "test0.m4a", Container::Mp4, 0, None, None);
    let actual = read_tag(&path);
    let expected = read_tag(&fixture("mut0.m4a"));

    compare_common(&expected, &actual);
    assert_eq!(
        strings(&actual, ItemKey::TrackArtist),
        vec!["Track Artist".to_string(), "Second Artist".to_string()]
    );
    assert!(actual.pictures().is_empty());
    assert_eq!(lyrics(&actual), Vec::<String>::new());
    assert_eq!(
        strings(&actual, ItemKey::Comment),
        vec!["https://music.yandex.ru/album/555/track/777".to_string()]
    );
}
