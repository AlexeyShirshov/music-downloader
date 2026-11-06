//! Определение типа обложки по сигнатуре файла.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MimeType {
    Jpeg,
    Png,
}

impl MimeType {
    pub fn as_str(self) -> &'static str {
        match self {
            MimeType::Jpeg => "image/jpeg",
            MimeType::Png => "image/png",
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            MimeType::Jpeg => ".jpg",
            MimeType::Png => ".png",
        }
    }

    pub fn lofty(self) -> lofty::picture::MimeType {
        match self {
            MimeType::Jpeg => lofty::picture::MimeType::Jpeg,
            MimeType::Png => lofty::picture::MimeType::Png,
        }
    }
}

const MAGIC_BYTES: [(MimeType, &[u8]); 2] = [
    (MimeType::Jpeg, &[0xFF, 0xD8, 0xFF]),
    (
        MimeType::Png,
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    ),
];

pub fn guess_mime_type(data: &[u8]) -> Option<MimeType> {
    MAGIC_BYTES
        .iter()
        .find(|(_, magic)| data.starts_with(magic))
        .map(|(mime, _)| *mime)
}
