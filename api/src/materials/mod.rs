//! The teacher's material library (docs/API.md「Phase 2.2：教材與註記」): what may be uploaded,
//! text extraction, storage of the original file, annotations and the hard delete.
//!
//! Rules that hold everywhere in this module (docs/BLUEPRINT.md §13.3): a teacher's own
//! (`personal`) material belongs to exactly one teacher and is never readable by another account;
//! it is never shared, put in the common knowledge base or used for training; deleting a material
//! removes the original object, the extracted text, the paragraphs and the annotations.
//!
//! Since Phase 2.9 the platform library sits beside it ([`catalog`]): `platform` materials
//! (教材參考, worksheet samples of 平台範本) are uploaded by ManageKnowledge, read and used by
//! every teacher, and changed only by ManageKnowledge. Annotations stay personal on both.

pub mod access;
pub mod annotations;
pub mod browse;
pub mod catalog;
pub mod detect;
pub mod extract;
pub mod ocr;
pub mod store;
pub mod thumbnail;

/// Pasted text (same bound as before for the lesson SOP).
pub const MAX_PASTED_CHARS: usize = crate::sop::MAX_MATERIAL_CHARS;
/// Text taken out of an uploaded document. Longer documents are cut here and marked
/// `text_truncated`; the original file is kept whole.
pub const MAX_EXTRACTED_CHARS: usize = 300_000;
pub const MAX_TITLE_CHARS: usize = crate::sop::MAX_TITLE_CHARS;
pub const MAX_SOURCE_NOTE_CHARS: usize = crate::sop::MAX_FIELD_CHARS;
pub const MAX_URL_CHARS: usize = 2_000;

const MB: u64 = 1024 * 1024;
/// Per-file limits by kind. Sized for the 2-core / 8 GB host: an upload is held in memory
/// while it is checked and hashed, and at most [`MAX_CONCURRENT_UPLOADS`] run at once.
pub const MAX_DOCUMENT_BYTES: u64 = 20 * MB;
pub const MAX_IMAGE_BYTES: u64 = 10 * MB;
pub const MAX_AUDIO_BYTES: u64 = 30 * MB;
pub const MAX_VIDEO_BYTES: u64 = 100 * MB;
/// The largest request body the upload route accepts (the largest file plus form overhead).
pub const MAX_UPLOAD_REQUEST_BYTES: usize = (MAX_VIDEO_BYTES + MB) as usize;
pub const MAX_CONCURRENT_UPLOADS: usize = 2;
/// Per teacher: all stored files together, and the number of materials of any kind.
pub const TEACHER_QUOTA_BYTES: u64 = 1024 * MB;
pub const TEACHER_MAX_MATERIALS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Document,
    Image,
    Audio,
    Video,
    Link,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Document => "document",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Link => "link",
        }
    }

    pub fn max_bytes(self) -> u64 {
        match self {
            Self::Document => MAX_DOCUMENT_BYTES,
            Self::Image => MAX_IMAGE_BYTES,
            Self::Audio => MAX_AUDIO_BYTES,
            Self::Video => MAX_VIDEO_BYTES,
            Self::Text | Self::Link => 0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "文字",
            Self::Document => "文件",
            Self::Image => "圖片",
            Self::Audio => "音訊",
            Self::Video => "影片",
            Self::Link => "網址",
        }
    }
}

/// One accepted upload format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileType {
    /// Lower-case, as stored (`jpeg` is stored as `jpg`).
    pub ext: &'static str,
    pub kind: Kind,
    /// The Content-Type the file is served with. Never taken from the upload.
    pub content_type: &'static str,
}

/// Everything a teacher may upload. Anything else is refused.
pub const FILE_TYPES: [FileType; 12] = [
    FileType {
        ext: "txt",
        kind: Kind::Document,
        content_type: "text/plain; charset=utf-8",
    },
    FileType {
        ext: "docx",
        kind: Kind::Document,
        content_type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    },
    FileType {
        ext: "pdf",
        kind: Kind::Document,
        content_type: "application/pdf",
    },
    FileType {
        ext: "png",
        kind: Kind::Image,
        content_type: "image/png",
    },
    FileType {
        ext: "jpg",
        kind: Kind::Image,
        content_type: "image/jpeg",
    },
    FileType {
        ext: "webp",
        kind: Kind::Image,
        content_type: "image/webp",
    },
    FileType {
        ext: "gif",
        kind: Kind::Image,
        content_type: "image/gif",
    },
    FileType {
        ext: "mp3",
        kind: Kind::Audio,
        content_type: "audio/mpeg",
    },
    FileType {
        ext: "m4a",
        kind: Kind::Audio,
        content_type: "audio/mp4",
    },
    FileType {
        ext: "wav",
        kind: Kind::Audio,
        content_type: "audio/wav",
    },
    FileType {
        ext: "mp4",
        kind: Kind::Video,
        content_type: "video/mp4",
    },
    FileType {
        ext: "webm",
        kind: Kind::Video,
        content_type: "video/webm",
    },
];

/// 「.txt、.docx、.pdf、…」 for messages.
pub fn allowed_extensions_label() -> String {
    FILE_TYPES
        .iter()
        .map(|t| format!(".{}", t.ext))
        .collect::<Vec<_>>()
        .join("、")
}

/// The accepted type for a file name, by its extension (case-insensitive).
pub fn file_type_for_name(filename: &str) -> Option<FileType> {
    let ext = filename.rsplit_once('.')?.1.trim().to_ascii_lowercase();
    let ext = if ext == "jpeg" { "jpg" } else { ext.as_str() };
    FILE_TYPES.iter().copied().find(|t| t.ext == ext)
}

/// Whether the bytes really are what the extension says (magic numbers; text must decode).
/// A renamed executable or an HTML page called `.png` is refused here.
pub fn content_matches(file_type: FileType, data: &[u8]) -> bool {
    let starts = |magic: &[u8]| data.starts_with(magic);
    let at = |offset: usize, magic: &[u8]| {
        data.len() >= offset + magic.len() && &data[offset..offset + magic.len()] == magic
    };
    match file_type.ext {
        "txt" => extract::decode_text(data).is_some(),
        "docx" => starts(b"PK\x03\x04"),
        "pdf" => data.windows(5).take(1024).any(|window| window == b"%PDF-"),
        "png" => starts(b"\x89PNG\r\n\x1a\n"),
        "jpg" => starts(b"\xFF\xD8\xFF"),
        "gif" => starts(b"GIF87a") || starts(b"GIF89a"),
        "webp" => starts(b"RIFF") && at(8, b"WEBP"),
        "wav" => starts(b"RIFF") && at(8, b"WAVE"),
        "mp3" => starts(b"ID3") || (data.len() >= 2 && data[0] == 0xFF && (data[1] & 0xE0) == 0xE0),
        "m4a" | "mp4" => at(4, b"ftyp"),
        "webm" => starts(b"\x1A\x45\xDF\xA3"),
        _ => false,
    }
}

/// A file name safe to store and to put in a Content-Disposition header: no path, no control
/// characters, bounded length.
pub fn clean_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && *c != '"')
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        return "material".to_string();
    }
    cleaned.chars().take(200).collect()
}

/// `http(s)://host…` only: a link material is a reference the teacher opens, nothing is fetched.
pub fn valid_link(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !host.is_empty()
        && url.chars().count() <= MAX_URL_CHARS
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_listed_extensions_are_accepted() {
        assert_eq!(file_type_for_name("Lesson 3.PDF").unwrap().ext, "pdf");
        assert_eq!(file_type_for_name("photo.JPEG").unwrap().ext, "jpg");
        assert_eq!(file_type_for_name("a.b.docx").unwrap().kind, Kind::Document);
        for bad in [
            "run.exe",
            "page.html",
            "icon.svg",
            "old.doc",
            "noext",
            "x.pdf.zip",
            "",
        ] {
            assert!(file_type_for_name(bad).is_none(), "{bad}");
        }
        assert!(allowed_extensions_label().contains(".docx"));
    }

    #[test]
    fn content_must_match_the_extension() {
        let ty = |n: &str| file_type_for_name(n).unwrap();
        assert!(content_matches(ty("a.pdf"), b"%PDF-1.7\n..."));
        assert!(!content_matches(ty("a.pdf"), b"<html>not a pdf</html>"));
        assert!(content_matches(ty("a.png"), b"\x89PNG\r\n\x1a\n...."));
        assert!(!content_matches(ty("a.png"), b"<script>alert(1)</script>"));
        assert!(content_matches(ty("a.jpg"), b"\xFF\xD8\xFF\xE0"));
        assert!(content_matches(ty("a.webp"), b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(!content_matches(ty("a.webp"), b"RIFF\0\0\0\0WAVEfmt "));
        assert!(content_matches(ty("a.wav"), b"RIFF\0\0\0\0WAVEfmt "));
        assert!(content_matches(ty("a.mp3"), b"ID3\x03\0"));
        assert!(content_matches(ty("a.mp3"), b"\xFF\xFB\x90\0"));
        assert!(content_matches(ty("a.mp4"), b"\0\0\0\x20ftypisom"));
        assert!(content_matches(ty("a.m4a"), b"\0\0\0\x20ftypM4A "));
        assert!(content_matches(ty("a.webm"), b"\x1A\x45\xDF\xA3...."));
        assert!(content_matches(ty("a.docx"), b"PK\x03\x04...."));
        assert!(content_matches(ty("a.txt"), "Hello 你好".as_bytes()));
        assert!(!content_matches(
            ty("a.txt"),
            b"\x00\x01\x02\xFF\xFE\x00binary\x00"
        ));
    }

    #[test]
    fn filenames_and_links_are_cleaned() {
        assert_eq!(
            clean_filename("C:\\Users\\me\\L3 reading.docx"),
            "L3 reading.docx"
        );
        assert_eq!(clean_filename("../../etc/passwd"), "passwd");
        assert_eq!(clean_filename("a\"b\n.pdf"), "ab.pdf");
        assert_eq!(clean_filename("  "), "material");
        assert!(valid_link("https://example.org/article?id=3"));
        assert!(valid_link("http://example.org"));
        for bad in [
            "ftp://x.org/a",
            "javascript:alert(1)",
            "https://",
            "https://a b.org",
            "example.org",
        ] {
            assert!(!valid_link(bad), "{bad}");
        }
    }
}
