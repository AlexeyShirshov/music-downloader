use std::path::Path;

use music_downloader::models::Track;
use music_downloader::path_pattern::prepare_base_path;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    pattern: String,
    #[serde(rename = "unsafe")]
    unsafe_path: bool,
    track: serde_json::Value,
    expected: String,
}

/// Эталонные значения получены запуском прежней Python-версии.
#[test]
fn path_parity_with_python() {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/paths.json")).expect("fixture is valid");
    assert!(!cases.is_empty());

    for case in cases {
        let track: Track = serde_json::from_value(case.track).expect("track is valid");
        let actual = prepare_base_path(Path::new(&case.pattern), &track, case.unsafe_path);
        assert_eq!(
            actual,
            Path::new(&case.expected),
            "case {} produced {}",
            case.name,
            actual.display()
        );
    }
}
