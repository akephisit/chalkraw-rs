use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Tiff,
    Raw(RawFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RawFormat {
    CanonCr2,
    CanonCr3,
    NikonNef,
    SonyArw,
    FujiRaf,
    PentaxPef,
    OlympusOrf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Flag {
    #[default]
    None,
    Pick,
    Reject,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExifMetadata {
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    pub shutter_speed: Option<String>,
    pub aperture: Option<f32>,
    pub focal_length: Option<f32>,
    pub captured_at: Option<DateTime<Utc>>,
}

pub type PhotoId = Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    pub id: PhotoId,
    pub original_path: PathBuf,
    pub file_hash: [u8; 32],
    pub imported_at: DateTime<Utc>,
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub exif: ExifMetadata,
    pub thumbnail: Vec<u8>,
    pub flag: Flag,
}

impl Photo {
    pub fn new(
        original_path: PathBuf,
        file_hash: [u8; 32],
        width: u32,
        height: u32,
        format: ImageFormat,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            original_path,
            file_hash,
            imported_at: Utc::now(),
            width,
            height,
            format,
            exif: ExifMetadata::default(),
            thumbnail: Vec::new(),
            flag: Flag::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_new_assigns_v7_uuid_and_now() {
        let before = Utc::now();
        let p = Photo::new(
            PathBuf::from("/tmp/a.jpg"),
            [0u8; 32],
            1024,
            768,
            ImageFormat::Jpeg,
        );
        let after = Utc::now();
        assert_eq!(p.width, 1024);
        assert_eq!(p.height, 768);
        assert_eq!(p.format, ImageFormat::Jpeg);
        assert_eq!(p.flag, Flag::None);
        assert!(p.imported_at >= before && p.imported_at <= after);
        // UUID v7 variant bits
        assert_eq!(p.id.get_version(), Some(uuid::Version::SortRand));
    }

    #[test]
    fn photo_roundtrips_through_serde() {
        let p = Photo::new(
            PathBuf::from("/tmp/a.jpg"),
            [7u8; 32],
            100,
            100,
            ImageFormat::Raw(RawFormat::CanonCr2),
        );
        let bytes = bincode::serialize(&p).unwrap();
        let back: Photo = bincode::deserialize(&bytes).unwrap();
        assert_eq!(p, back);
    }
}
