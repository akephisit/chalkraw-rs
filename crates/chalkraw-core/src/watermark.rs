use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type WatermarkId = uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum WatermarkAnchor {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageLayer {
    pub png_path: PathBuf,
    pub anchor: WatermarkAnchor,
    /// Percent of output long edge (1..50).
    pub size_pct: f32,
    /// 0..1
    pub opacity: f32,
    /// Percent of output long edge (0..20).
    pub margin_pct: f32,
    /// -180..180. Not applied in Phase 5A; placeholder for Phase 5 polish.
    pub rotation_deg: f32,
}

impl Default for ImageLayer {
    fn default() -> Self {
        Self {
            png_path: PathBuf::new(),
            anchor: WatermarkAnchor::BottomRight,
            size_pct: 15.0,
            opacity: 0.8,
            margin_pct: 3.0,
            rotation_deg: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WatermarkLayer {
    Image(ImageLayer),
    // Text variant added in Phase 5B.
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WatermarkPreset {
    pub id: WatermarkId,
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub layers: Vec<WatermarkLayer>,
}

impl WatermarkPreset {
    pub fn new(name: String) -> Self {
        Self {
            id: uuid::Uuid::now_v7(),
            name,
            created_at: chrono::Utc::now(),
            layers: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watermark_preset_roundtrips_through_bincode() {
        let mut p = WatermarkPreset::new("Studio".into());
        p.layers.push(WatermarkLayer::Image(ImageLayer {
            png_path: PathBuf::from("/logo.png"),
            anchor: WatermarkAnchor::BottomRight,
            size_pct: 15.0,
            opacity: 0.75,
            margin_pct: 3.0,
            rotation_deg: 0.0,
        }));
        let bytes = bincode::serialize(&p).unwrap();
        let back: WatermarkPreset = bincode::deserialize(&bytes).unwrap();
        assert_eq!(p, back);
    }
}
