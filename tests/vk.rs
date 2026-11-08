use music_downloader::vk::{
    extract_access_token, extract_vk_cookies, mp3_url, parse_audio_link, parse_web_token, AudioId,
    VkAudio, VkUser,
};

#[test]
fn parses_vk_audio_links() {
    let expected = AudioId {
        owner_id: -2001370911,
        audio_id: 36370911,
    };

    for link in [
        "https://vk.ru/audio-2001370911_36370911",
        "https://vk.com/audio-2001370911_36370911",
        "http://m.vk.com/audio-2001370911_36370911",
        "https://www.vk.com/audio-2001370911_36370911",
        "vk.com/audio-2001370911_36370911",
        "https://vk.com/audio-2001370911_36370911?from=search",
    ] {
        assert_eq!(parse_audio_link(link), Some(expected), "{link}");
    }
}

#[test]
fn parses_user_audio_links() {
    assert_eq!(
        parse_audio_link("https://vk.com/audio456239017_456239269"),
        Some(AudioId {
            owner_id: 456239017,
            audio_id: 456239269,
        })
    );
}

#[test]
fn ignores_non_audio_links() {
    for link in [
        "https://music.yandex.ru/album/1/track/2",
        "https://vk.com/audio_playlist-1_2",
        "https://vk.com/audio",
        "https://example.com/audio-1_2",
    ] {
        assert_eq!(parse_audio_link(link), None, "{link}");
    }
}

#[test]
fn converts_hls_links_to_mp3() {
    assert_eq!(
        mp3_url("https://psv4.userapi.com/s/v1/abc123/audios/def456/index.m3u8?extra=token")
            .unwrap(),
        "https://psv4.userapi.com/s/v1/audios/def456.mp3?extra=token"
    );
    assert_eq!(
        mp3_url("https://host/abc123/def456/index.m3u8").unwrap(),
        "https://host/def456.mp3"
    );

    let direct = "https://psv4.userapi.com/s/v1/audios/def456/track.mp3?extra=token";
    assert_eq!(mp3_url(direct).unwrap(), direct);
}

#[test]
fn parses_api_response_and_builds_track() {
    let audio: VkAudio = serde_json::from_str(
        r#"{
            "id": "36370911",
            "owner_id": -2001370911,
            "artist": "Artist",
            "title": "Song",
            "duration": 215,
            "url": "https://example.com/song.mp3"
        }"#,
    )
    .expect("api response");

    assert_eq!(audio.id, Some(36370911));
    assert_eq!(audio.owner_id, Some(-2001370911));
    assert_eq!(audio.duration, Some(215));
    assert_eq!(
        audio.download_url().unwrap(),
        "https://example.com/song.mp3"
    );

    let track = audio.to_track(AudioId {
        owner_id: 0,
        audio_id: 0,
    });
    assert_eq!(track.id.as_deref(), Some("-2001370911_36370911"));
    assert_eq!(track.title.as_deref(), Some("Song"));
    assert_eq!(track.artists[0].name.as_deref(), Some("Artist"));
    assert_eq!(track.duration_ms, Some(215_000));
    assert!(track.albums.is_empty());
}

#[test]
fn extracts_token_from_pasted_url_or_token() {
    assert_eq!(
        extract_access_token(
            "https://oauth.vk.com/blank.html#access_token=vk1.a.ABC_def-123&expires_in=0&user_id=1"
        )
        .as_deref(),
        Some("vk1.a.ABC_def-123")
    );
    assert_eq!(
        extract_access_token("  vk1.a.token_123  ").as_deref(),
        Some("vk1.a.token_123")
    );
    assert_eq!(
        extract_access_token("https://vk.com/audio1_2").as_deref(),
        None
    );
    assert_eq!(extract_access_token("").as_deref(), None);
    assert_eq!(extract_access_token("   ").as_deref(), None);
}

#[test]
fn formats_account_names() {
    let user = VkUser {
        id: Some(42),
        first_name: Some("Иван".to_string()),
        last_name: Some("Петров".to_string()),
        screen_name: Some("ivan".to_string()),
    };
    assert_eq!(user.display_name(), "Иван Петров");

    let user = VkUser {
        id: Some(42),
        screen_name: Some("ivan".to_string()),
        ..VkUser::default()
    };
    assert_eq!(user.display_name(), "@ivan");

    let user = VkUser {
        id: Some(42),
        ..VkUser::default()
    };
    assert_eq!(user.display_name(), "id42");
}

#[test]
fn reads_cookies_from_header() {
    let cookies = extract_vk_cookies("p=abc123; remixsid=def456").unwrap();
    assert_eq!(cookies, "p=abc123; remixsid=def456");

    // Заголовок передаётся как есть: порядок и одноимённые куки важны.
    let header = "Cookie: remixlang=0; remixsid=def; YandexBrowser=1; remixsid=ghi";
    assert_eq!(
        extract_vk_cookies(header).unwrap(),
        "remixlang=0; remixsid=def; YandexBrowser=1; remixsid=ghi"
    );
}

#[test]
fn reads_cookies_from_netscape_file() {
    let text = "# Netscape HTTP Cookie File\n\
        .vk.ru\tTRUE\t/\tTRUE\t1800000000\tp\tabc\n\
        .vk.ru\tTRUE\t/\tTRUE\t1800000000\tremixsid\tdef\n";
    assert_eq!(extract_vk_cookies(text).unwrap(), "p=abc; remixsid=def");
}

#[test]
fn reads_cookies_from_json_export() {
    let text =
        r#"[{"name":"p","value":"abc","domain":".vk.ru"},{"name":"remixsid","value":"def"}]"#;
    assert_eq!(extract_vk_cookies(text).unwrap(), "p=abc; remixsid=def");
}

#[test]
fn accepts_any_session_cookie_without_p() {
    assert_eq!(
        extract_vk_cookies("remixnsid=def").unwrap(),
        "remixnsid=def"
    );
    assert_eq!(
        extract_vk_cookies("remixsid=def; remixdsid=ghi").unwrap(),
        "remixsid=def; remixdsid=ghi"
    );
}

#[test]
fn filters_cookies_of_other_sites() {
    let text = ".example.com\tTRUE\t/\tTRUE\t1800000000\tsessionid\tsecret\n\
        .vk.ru\tTRUE\t/\tTRUE\t1800000000\tremixsid\tdef\n";
    assert_eq!(extract_vk_cookies(text).unwrap(), "remixsid=def");
}

#[test]
fn reports_missing_session_cookie() {
    let error = extract_vk_cookies("p=abc").unwrap_err().to_string();
    assert!(error.contains("remixsid"), "{error}");
    let error = extract_vk_cookies("foo=bar").unwrap_err().to_string();
    assert!(error.contains("Cookie"), "{error}");
}

#[test]
fn parses_web_token_response() {
    let token = parse_web_token(
        br#"{"type":"okay","data":{"access_token":"vk1.a.TEST","expires":1784473661}}"#,
    )
    .unwrap();
    assert_eq!(token.access_token, "vk1.a.TEST");
    assert_eq!(token.expires, 1784473661);

    let error = parse_web_token(br#"{"type":"error","error_info":"unauthorized"}"#)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unauthorized"), "{error}");
}
