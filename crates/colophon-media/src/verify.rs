//! The quarantine step's checks (07-media.md, M2): the real signature decides, the decode is
//! bounded, and the bytes that survive have no metadata.
//!
//! `verify_image` is synchronous and CPU-bound by design: the caller runs it on a blocking
//! thread inside its own deadline, so a pathological file can never stall an async worker.

use std::io::Cursor;

use image::{ImageFormat, ImageReader};
use sha2::{Digest, Sha256};

use crate::asset::MAX_BYTES;

/// The most pixels an image may claim: a bound on decompression bombs (07-media.md).
const MAX_PIXELS: u64 = 25_000_000;
/// The most memory one decode may allocate, in bytes.
const MAX_ALLOC: usize = 256 * 1024 * 1024;

/// What verification proved about the upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// The type the signature and decode proved — never the client's claim.
    pub detected_type: &'static str,
    /// The decoded dimensions.
    pub width: u32,
    /// The decoded dimensions.
    pub height: u32,
    /// Re-encoded bytes: no EXIF, no comments, no chunks the decoder didn't need.
    pub stripped: Vec<u8>,
    /// `stripped`'s digest: what the asset record and any future comparison pin.
    pub sha256: [u8; 32],
}

/// Why the bytes were refused. Every variant words a reason safe to show the uploader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Reject {
    /// Over the 20 MiB bound.
    #[error("the file is larger than 20 MiB")]
    TooLarge,
    /// The signature matched none of the allowed types.
    #[error("the file is not a PNG, JPEG or WebP image")]
    NotAnImage,
    /// It signed in as an image but the bounded decoder refused it.
    #[error("the image could not be decoded")]
    Decode,
    /// It decoded to more pixels than the bound allows.
    #[error("the image is larger than 25 million pixels")]
    TooManyPixels,
    /// It decoded but could not be re-encoded for serving.
    #[error("the image could not be re-encoded")]
    Encode,
}

/// The MIME type a byte string's signature claims, or `None`. Only the first bytes decide —
/// a text file that starts like an image still fails at decode.
pub fn sniff(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if head.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if head.len() >= 12 && head[..4] == *b"RIFF" && head[8..12] == *b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Proves an upload: signature, bounded decode, strip, digest. Pure CPU — the caller supplies
/// the thread and the deadline.
pub fn verify_image(bytes: &[u8]) -> Result<Verified, Reject> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Reject::TooLarge);
    }
    let detected_type = sniff(bytes).ok_or(Reject::NotAnImage)?;
    let format = match detected_type {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        // `sniff` admits no other types.
        _ => ImageFormat::WebP,
    };
    // `max_image_*` are strict limits: a decoder that can't uphold them must fail before it
    // allocates, so a header lying about its frame size is refused without the allocation.
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(MAX_ALLOC as u64);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|_| Reject::Decode)?;
    if u64::from(decoded.width()) * u64::from(decoded.height()) > MAX_PIXELS {
        return Err(Reject::TooManyPixels);
    }
    let mut stripped = Cursor::new(Vec::new());
    match format {
        ImageFormat::Jpeg => decoded
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut stripped,
                90,
            ))
            .map_err(|_| Reject::Encode)?,
        _ => decoded
            .write_to(&mut stripped, format)
            .map_err(|_| Reject::Encode)?,
    }
    let stripped = stripped.into_inner();
    let mut sha256 = [0_u8; 32];
    sha256.copy_from_slice(&Sha256::digest(&stripped));
    Ok(Verified {
        detected_type,
        width: decoded.width(),
        height: decoded.height(),
        stripped,
        sha256,
    })
}

#[cfg(test)]
mod tests {
    use image::DynamicImage;

    use super::*;

    /// A 2×2 PNG built at test time: the smallest real fixture.
    fn tiny_png() -> Vec<u8> {
        let image = DynamicImage::new_rgba8(2, 2);
        let mut out = Cursor::new(Vec::new());
        image
            .write_to(&mut out, ImageFormat::Png)
            .expect("png encodes");
        out.into_inner()
    }

    /// A 2×2 JPEG.
    fn tiny_jpeg() -> Vec<u8> {
        let image = DynamicImage::new_rgb8(2, 2);
        let mut out = Cursor::new(Vec::new());
        image
            .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut out, 80,
            ))
            .expect("jpeg encodes");
        out.into_inner()
    }

    /// A 2×2 WebP.
    fn tiny_webp() -> Vec<u8> {
        let image = DynamicImage::new_rgba8(2, 2);
        let mut out = Cursor::new(Vec::new());
        image
            .write_to(&mut out, ImageFormat::WebP)
            .expect("webp encodes");
        out.into_inner()
    }

    #[test]
    fn signatures() {
        assert_eq!(sniff(&tiny_png()), Some("image/png"));
        assert_eq!(sniff(&tiny_jpeg()), Some("image/jpeg"));
        assert_eq!(sniff(&tiny_webp()), Some("image/webp"));
        assert_eq!(sniff(b"<html><body>hi"), None);
        assert_eq!(sniff(b"GIF89a"), None);
        assert_eq!(sniff(&[0x89]), None);
    }

    #[test]
    fn verifies_each_allowed_type() {
        for (bytes, mime) in [
            (tiny_png(), "image/png"),
            (tiny_jpeg(), "image/jpeg"),
            (tiny_webp(), "image/webp"),
        ] {
            let verified = verify_image(&bytes).expect("a real image verifies");
            assert_eq!(verified.detected_type, mime);
            assert_eq!((verified.width, verified.height), (2, 2));
            assert_ne!(verified.stripped.len(), 0);
        }
    }

    /// M2: a file's declared type is never consulted — HTML that claims nothing is refused, and
    /// so are bytes that only *start* like an image.
    #[test]
    fn refuses_misleading_types() {
        assert_eq!(
            verify_image(b"<html><body>not an image"),
            Err(Reject::NotAnImage)
        );
        let mut fake = b"\x89PNG\r\n\x1a\n".to_vec();
        fake.extend_from_slice(b"then html");
        assert_eq!(verify_image(&fake), Err(Reject::Decode));
    }

    #[test]
    fn refuses_oversize() {
        let huge = vec![0_u8; usize::try_from(MAX_BYTES).expect("20 MiB fits in usize") + 1];
        assert_eq!(verify_image(&huge), Err(Reject::TooLarge));
    }

    /// A PNG header that promises a 60,000×60,000 frame must fail the bounded decode, not
    /// allocate for it (07-media.md: pixel bombs).
    #[test]
    fn refuses_pixel_bombs() {
        // A real, tiny PNG whose IHDR width/height fields are rewritten to huge values.
        let mut bomb = tiny_png();
        bomb[16..20].copy_from_slice(&60_000_u32.to_be_bytes());
        bomb[20..24].copy_from_slice(&60_000_u32.to_be_bytes());
        assert!(matches!(
            verify_image(&bomb),
            Err(Reject::Decode | Reject::TooManyPixels)
        ));
    }
}
