use yandex_music_downloader::api::{decrypt_data, file_info_sign_at, lyrics_sign_at};
use yandex_music_downloader::mime::{guess_mime_type, MimeType};
use yandex_music_downloader::path_pattern::truncate_to_byte_length;

/// Эталоны посчитаны Python-версией (hmac/hashlib/base64 и pycryptodome).
#[test]
fn file_info_signature_matches_python() {
    assert_eq!(
        file_info_sign_at("53404", "nq", 1_700_000_000),
        "3toawVKiO2ILXUv5bPGq3vvoMoX06I4QgwLptuWNUL4"
    );
}

#[test]
fn lyrics_signature_matches_python() {
    assert_eq!(
        lyrics_sign_at("53404", 1_700_000_000),
        "88d7CiJPxOVnEMfukYLD4NsgixIqjMLXQr6YqXAC88M="
    );
}

#[test]
fn aes_ctr_decryption_matches_pycryptodome() {
    let key = "000102030405060708090a0b0c0d0e0f";
    // Шифротекст из Python (AES-CTR, nonce = 12 нулей).
    let cipher = hex::decode(
        "c6a03934838a5d8567468b69adc5d6766357018681d5a2095162a7f879e93315\
         69f7a570bdbe80abcba050434cac9eb2899c191d005ff40f68640c65a08ac269",
    )
    .unwrap();
    let plain = decrypt_data(&cipher, key).expect("decryption works");
    let expected: Vec<u8> = (0..64u8).collect();
    assert_eq!(plain, expected);
}

#[test]
fn mime_detection() {
    assert_eq!(
        guess_mime_type(&[0xFF, 0xD8, 0xFF, 0x00]),
        Some(MimeType::Jpeg)
    );
    assert_eq!(
        guess_mime_type(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]),
        Some(MimeType::Png)
    );
    assert_eq!(guess_mime_type(b"GIF89a"), None);
}

#[test]
fn truncation_is_utf8_safe() {
    assert_eq!(truncate_to_byte_length("abcdef", 3), "abc");
    // "Ж" занимает 2 байта, 125 символов = 250 байт.
    let text = "Ж".repeat(200);
    assert_eq!(truncate_to_byte_length(&text, 250).len(), 250);
    assert!(truncate_to_byte_length(&text, 250)
        .chars()
        .all(|c| c == 'Ж'));
}
