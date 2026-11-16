//! Turning an uploaded file into page images for the OCR model.
//!
//! * **PDF**: `pdftoppm` (poppler-utils, installed in the Docker image) renders one page at a
//!   time to PNG, 1400 px wide (about 170 dpi for A4: what the probe read cleanly in ~10 s), in
//!   a temporary directory that is removed when the job ends. `pdfinfo` counts the pages when
//!   the text extraction could not.
//! * **Images**: decoded (first frame of a GIF), turned upright by their EXIF orientation and
//!   scaled down to at most 1600 px on the long side. A small PNG or JPEG that needs neither is
//!   sent as it is; one the decoder cannot read is sent as it is when under 4 MB.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};

use super::client::PageImage;

/// Width of a rendered PDF page.
pub const PAGE_WIDTH_PX: u32 = 1400;
/// Longest side of an image sent to the model.
pub const MAX_IMAGE_SIDE: u32 = 1600;
/// An image that cannot be decoded is sent as it is only below this size.
pub const MAX_RAW_IMAGE_BYTES: usize = 4 * 1024 * 1024;
/// Memory the decoder may use for one image (a 48-megapixel photo fits).
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;
const RENDER_TIMEOUT: Duration = Duration::from_secs(90);
const JPEG_QUALITY: u8 = 90;

pub const MSG_IMAGE_UNREADABLE: &str = "這張圖片讀不出來（檔案可能已損壞）。";

/// A temporary directory, removed with everything in it when dropped.
pub struct WorkDir(PathBuf);

impl WorkDir {
    pub fn new() -> std::io::Result<Self> {
        let dir = std::env::temp_dir().join(format!("manna-ocr-{}", crate::auth::new_token()));
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Whether `pdftoppm` can be started at all.
pub async fn have_pdftoppm() -> bool {
    tokio::process::Command::new("pdftoppm")
        .arg("-v")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .is_ok()
}

async fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<std::process::Output, String> {
    let child = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    match tokio::time::timeout(RENDER_TIMEOUT, child).await {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(e)) => Err(format!("{program} could not start: {e}")),
        Err(_) => Err(format!("{program} timed out")),
    }
}

/// Pages of a PDF, by `pdfinfo`.
pub async fn pdf_page_count(pdf: &Path) -> Result<i32, String> {
    let out = run("pdfinfo", &[pdf.as_os_str()]).await?;
    if !out.status.success() {
        return Err(format!("pdfinfo failed ({})", out.status));
    }
    parse_pdfinfo_pages(&String::from_utf8_lossy(&out.stdout))
        .ok_or_else(|| "pdfinfo printed no page count".to_string())
}

pub fn parse_pdfinfo_pages(stdout: &str) -> Option<i32> {
    stdout
        .lines()
        .find_map(|l| l.strip_prefix("Pages:"))
        .and_then(|n| n.trim().parse().ok())
}

/// Page `page` (from 1) of `pdf` as PNG bytes. The file is written into `dir` and removed.
pub async fn render_pdf_page(pdf: &Path, dir: &Path, page: i32) -> Result<Vec<u8>, String> {
    let prefix = dir.join(format!("page-{page}"));
    let page_arg = page.to_string();
    let width = PAGE_WIDTH_PX.to_string();
    let args: Vec<&std::ffi::OsStr> = vec![
        "-png".as_ref(),
        "-f".as_ref(),
        page_arg.as_ref(),
        "-l".as_ref(),
        page_arg.as_ref(),
        "-scale-to-x".as_ref(),
        width.as_ref(),
        "-scale-to-y".as_ref(),
        "-1".as_ref(),
        "-singlefile".as_ref(),
        pdf.as_os_str(),
        prefix.as_os_str(),
    ];
    let out = run("pdftoppm", &args).await?;
    let file = prefix.with_extension("png");
    let bytes = tokio::fs::read(&file).await;
    let _ = tokio::fs::remove_file(&file).await;
    if !out.status.success() {
        return Err(format!("pdftoppm failed ({})", out.status));
    }
    bytes.map_err(|e| format!("pdftoppm wrote no page: {e}"))
}

/// What is sent for an uploaded image (see the module comment). CPU-bound: call it from
/// `spawn_blocking`.
pub fn prepare_image(data: &[u8], ext: &str) -> Result<PageImage, String> {
    let raw_mime = match ext {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return Err(MSG_IMAGE_UNREADABLE.to_string()),
    };
    let as_is = || {
        if data.len() <= MAX_RAW_IMAGE_BYTES {
            Ok(PageImage {
                bytes: data.to_vec(),
                mime: raw_mime,
            })
        } else {
            Err(MSG_IMAGE_UNREADABLE.to_string())
        }
    };
    let Ok((image, orientation)) = decode(data) else {
        return as_is();
    };
    let too_big = image.width().max(image.height()) > MAX_IMAGE_SIDE;
    let upright = orientation == Orientation::NoTransforms;
    if !too_big && upright && matches!(ext, "png" | "jpg") && data.len() <= MAX_RAW_IMAGE_BYTES {
        return as_is();
    }
    let mut image = image;
    image.apply_orientation(orientation);
    if too_big {
        image = image.resize(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE, FilterType::Triangle);
    }
    encode(&image, ext).map_err(|_| MSG_IMAGE_UNREADABLE.to_string())
}

fn decode(data: &[u8]) -> image::ImageResult<(DynamicImage, Orientation)> {
    let mut reader = ImageReader::new(Cursor::new(data)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    Ok((DynamicImage::from_decoder(decoder)?, orientation))
}

/// PNG for drawings and screenshots (png, gif), JPEG for photos (jpg, webp).
fn encode(image: &DynamicImage, ext: &str) -> image::ImageResult<PageImage> {
    let mut out = Cursor::new(Vec::new());
    if matches!(ext, "png" | "gif") {
        image.write_to(&mut out, ImageFormat::Png)?;
        Ok(PageImage {
            bytes: out.into_inner(),
            mime: "image/png",
        })
    } else {
        let rgb = DynamicImage::ImageRgb8(image.to_rgb8());
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
        rgb.write_with_encoder(encoder)?;
        Ok(PageImage {
            bytes: out.into_inner(),
            mime: "image/jpeg",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        }));
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn size_of(bytes: &[u8]) -> (u32, u32) {
        let image = image::load_from_memory(bytes).unwrap();
        (image.width(), image.height())
    }

    #[test]
    fn small_images_go_as_they_are_and_large_ones_are_scaled_down() {
        let small = png(200, 100);
        let sent = prepare_image(&small, "png").unwrap();
        assert_eq!(sent.bytes, small);
        assert_eq!(sent.mime, "image/png");

        let large = prepare_image(&png(2400, 1200), "png").unwrap();
        assert_eq!(large.mime, "image/png");
        assert_eq!(size_of(&large.bytes), (1600, 800));

        let tall = prepare_image(&png(900, 3200), "png").unwrap();
        assert_eq!(size_of(&tall.bytes), (450, 1600));
    }

    #[test]
    fn photos_become_jpeg_and_unreadable_images_go_raw_when_small() {
        let mut jpeg = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image::RgbImage::new(2000, 1000))
            .write_to(&mut jpeg, ImageFormat::Jpeg)
            .unwrap();
        let sent = prepare_image(&jpeg.into_inner(), "jpg").unwrap();
        assert_eq!(sent.mime, "image/jpeg");
        assert_eq!(size_of(&sent.bytes), (1600, 800));

        let junk = b"\x89PNG\r\n\x1a\n not really a png".to_vec();
        let raw = prepare_image(&junk, "png").unwrap();
        assert_eq!(raw.bytes, junk, "small undecodable images are sent as is");
        let big_junk = [b"\x89PNG\r\n\x1a\n".as_slice(), &vec![0u8; 5 * 1024 * 1024]].concat();
        assert_eq!(
            prepare_image(&big_junk, "png"),
            Err(MSG_IMAGE_UNREADABLE.to_string())
        );
    }

    #[test]
    fn pdfinfo_page_count() {
        let out = "Title:          x\nPages:          12\nEncrypted:      no\n";
        assert_eq!(parse_pdfinfo_pages(out), Some(12));
        assert_eq!(parse_pdfinfo_pages("Title: x\n"), None);
    }

    #[test]
    fn the_work_dir_is_removed() {
        let dir = WorkDir::new().unwrap();
        let path = dir.path().to_path_buf();
        std::fs::write(path.join("page-1.png"), b"x").unwrap();
        drop(dir);
        assert!(!path.exists());
    }
}
