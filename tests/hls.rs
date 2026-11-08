use std::collections::HashMap;

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes128;
use music_downloader::hls::{self, AudioKind, Piece};
use url::Url;

const KEY: [u8; 16] = [0x11; 16];
const IV: [u8; 16] = [0x22; 16];

fn encrypt(key: &[u8; 16], iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    let mut output = data.to_vec();
    let padding = 16 - output.len() % 16;
    output.extend(std::iter::repeat_n(padding as u8, padding));

    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut previous = *iv;
    let (chunks, _) = output.as_chunks_mut::<16>();
    for chunk in chunks {
        for (byte, prev) in chunk.iter_mut().zip(previous) {
            *byte ^= prev;
        }
        let mut block = GenericArray::from(*chunk);
        cipher.encrypt_block(&mut block);
        chunk.copy_from_slice(&block);
        previous = *chunk;
    }
    output
}

fn fetch_from(
    files: HashMap<String, Vec<u8>>,
) -> impl Fn(&str, Option<(u64, u64)>) -> anyhow::Result<Vec<u8>> {
    move |url, range| {
        let body = files
            .get(url)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("нет файла {url}"))?;
        Ok(match range {
            Some((start, end)) => body[start as usize..=end as usize].to_vec(),
            None => body,
        })
    }
}

#[test]
fn parses_vk_shaped_playlist() {
    let base = Url::parse("https://c.ru/p/index.m3u8").unwrap();
    let text = "#EXTM3U\n\
        #EXT-X-VERSION:3\n\
        #EXT-X-MEDIA-SEQUENCE:5\n\
        #EXT-X-KEY:METHOD=AES-128,URI=\"key.bin?siren=1\"\n\
        #EXTINF:2,\n\
        seg-00.mp3?siren=1\n\
        #EXT-X-KEY:METHOD=NONE\n\
        #EXTINF:2,\n\
        seg-01.mp3?siren=1\n\
        #EXT-X-ENDLIST\n";

    let pieces = hls::parse_media(text, &base).unwrap();
    assert_eq!(pieces.len(), 2);
    assert_eq!(pieces[0].url, "https://c.ru/p/seg-00.mp3?siren=1");
    assert_eq!(
        pieces[0].key_url.as_deref(),
        Some("https://c.ru/p/key.bin?siren=1")
    );
    // IV не задан — им становится номер сегмента (media sequence).
    let mut expected = [0u8; 16];
    expected[8..].copy_from_slice(&5u64.to_be_bytes());
    assert_eq!(pieces[0].iv, Some(expected));
    assert_eq!(pieces[1].key_url, None);
    assert_eq!(pieces[1].iv, None);
}

#[test]
fn explicit_iv_wins() {
    let base = Url::parse("https://c.ru/index.m3u8").unwrap();
    let text = "#EXTM3U\n\
        #EXT-X-KEY:METHOD=AES-128,URI=\"key\",IV=0x0102\n\
        #EXTINF:1,\n\
        a.mp3\n";
    let pieces = hls::parse_media(text, &base).unwrap();
    let mut expected = [0u8; 16];
    expected[15] = 0x02;
    expected[14] = 0x01;
    assert_eq!(pieces[0].iv, Some(expected));
}

#[test]
fn downloads_and_decrypts_segments_in_order() {
    let first = b"first segment".to_vec();
    let second = b"second segment, a bit longer".to_vec();
    let encrypted = encrypt(&KEY, &IV, &first);

    let playlist = "#EXTM3U\n\
        #EXT-X-KEY:METHOD=AES-128,URI=\"key\",IV=0x22222222222222222222222222222222\n\
        #EXTINF:1,\n\
        a.mp3\n\
        #EXT-X-KEY:METHOD=NONE\n\
        #EXTINF:1,\n\
        b.mp3\n\
        #EXT-X-ENDLIST\n";

    let files: HashMap<String, Vec<u8>> = HashMap::from([
        (
            "https://c.ru/pl/index.m3u8".to_string(),
            playlist.as_bytes().to_vec(),
        ),
        ("https://c.ru/pl/key".to_string(), KEY.to_vec()),
        ("https://c.ru/pl/a.mp3".to_string(), encrypted),
        ("https://c.ru/pl/b.mp3".to_string(), second.clone()),
    ]);

    let output = hls::download("https://c.ru/pl/index.m3u8", &fetch_from(files)).unwrap();
    let mut expected = first;
    expected.extend_from_slice(&second);
    assert_eq!(output, expected);
}

#[test]
fn picks_best_variant_from_master_playlist() {
    let master = "#EXTM3U\n\
        #EXT-X-STREAM-INF:BANDWIDTH=64000\n\
        low.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=256000\n\
        high.m3u8\n";
    let media = "#EXTM3U\n#EXTINF:1,\na.mp3\n#EXT-X-ENDLIST\n";

    let files: HashMap<String, Vec<u8>> = HashMap::from([
        (
            "https://c.ru/master.m3u8".to_string(),
            master.as_bytes().to_vec(),
        ),
        (
            "https://c.ru/low.m3u8".to_string(),
            media.as_bytes().to_vec(),
        ),
        (
            "https://c.ru/high.m3u8".to_string(),
            media.as_bytes().to_vec(),
        ),
        ("https://c.ru/a.mp3".to_string(), b"high".to_vec()),
    ]);

    let output = hls::download("https://c.ru/master.m3u8", &fetch_from(files)).unwrap();
    assert_eq!(output, b"high");
}

#[test]
fn supports_byte_ranges() {
    let payload = b"0123456789abcdef".to_vec();
    let playlist = "#EXTM3U\n\
        #EXTINF:1,\n\
        #EXT-X-BYTERANGE:8@0\n\
        all.mp3\n\
        #EXTINF:1,\n\
        #EXT-X-BYTERANGE:8\n\
        all.mp3\n\
        #EXT-X-ENDLIST\n";
    let files: HashMap<String, Vec<u8>> = HashMap::from([
        (
            "https://c.ru/index.m3u8".to_string(),
            playlist.as_bytes().to_vec(),
        ),
        ("https://c.ru/all.mp3".to_string(), payload),
    ]);

    let output = hls::download("https://c.ru/index.m3u8", &fetch_from(files)).unwrap();
    assert_eq!(output, b"0123456789abcdef");
}

#[test]
fn init_section_is_kept() {
    let playlist = "#EXTM3U\n\
        #EXT-X-MAP:URI=\"init.mp4\"\n\
        #EXTINF:1,\n\
        seg.m4s\n\
        #EXT-X-ENDLIST\n";
    let files: HashMap<String, Vec<u8>> = HashMap::from([
        (
            "https://c.ru/index.m3u8".to_string(),
            playlist.as_bytes().to_vec(),
        ),
        ("https://c.ru/init.mp4".to_string(), b"init".to_vec()),
        ("https://c.ru/seg.m4s".to_string(), b"media".to_vec()),
    ]);

    let output = hls::download("https://c.ru/index.m3u8", &fetch_from(files)).unwrap();
    assert_eq!(output, b"initmedia");
}

#[test]
fn rejects_unknown_encryption() {
    let base = Url::parse("https://c.ru/index.m3u8").unwrap();
    let text = "#EXTM3U\n\
        #EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"key\"\n\
        #EXTINF:1,\n\
        a.mp3\n";
    let error = hls::parse_media(text, &base).unwrap_err().to_string();
    assert!(error.contains("SAMPLE-AES"), "{error}");
}

#[test]
fn sniffs_containers() {
    assert_eq!(AudioKind::sniff(b"ID3\x04"), AudioKind::Mp3);
    assert_eq!(AudioKind::sniff(&[0xFF, 0xFB, 0x90]), AudioKind::Mp3);
    assert_eq!(AudioKind::sniff(b"\0\0\0\x20ftypM4A "), AudioKind::Mp4);
    assert_eq!(AudioKind::sniff(&[0x47, 0x40, 0x00]), AudioKind::Ts);
    assert_eq!(AudioKind::sniff(b"<html>"), AudioKind::Unknown);
}

#[test]
fn piece_is_plain_data() {
    // Тип должен оставаться простым, чтобы его можно было проверять в тестах.
    let piece = Piece {
        url: "https://c.ru/a".to_string(),
        range: None,
        key_url: None,
        iv: None,
    };
    assert_eq!(piece.url, "https://c.ru/a");
}
