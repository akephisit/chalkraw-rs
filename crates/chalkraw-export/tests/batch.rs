use chalkraw_core::EditState;
use chalkraw_export::{
    export_batch, BatchItem, BatchOptions, ExportFormat, ExportResize, WatermarkAnchor,
    WatermarkStamp,
};
use chalkraw_render::RenderDevice;
use std::path::PathBuf;

#[allow(unused_imports)]
use chalkraw_core::{ImageLayer, TextLayer, TextColor, WatermarkLayer, WatermarkPreset};

fn fixture_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("tests/fixtures/sample.jpg");
    p
}

#[test]
fn export_batch_writes_one_file_per_item() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let img_path = fixture_path();
    let dir = tempfile::tempdir().unwrap();

    let items = vec![
        BatchItem {
            source_path: img_path.clone(),
            edit: EditState::default(),
            original_name: "a".into(),
        },
        BatchItem {
            source_path: img_path.clone(),
            edit: EditState::default(),
            original_name: "b".into(),
        },
    ];
    let opts = BatchOptions {
        format: ExportFormat::Jpeg { quality: 80 },
        resize: ExportResize::LongEdge(256),
        output_dir: dir.path().to_path_buf(),
        name_pattern: "{name}_test".into(),
        watermark: None,
        watermark_preset: None,
    };
    let results = export_batch(&rd, &items, &opts, |_, _, _| {});
    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(r.error.is_none(), "error: {:?}", r.error);
        assert!(r.output_path.as_ref().unwrap().exists());
    }
}

#[test]
fn export_batch_with_watermark_completes() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let dir = tempfile::tempdir().unwrap();
    let wm_path = dir.path().join("wm.png");
    image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(8, 8, image::Rgba([255, 0, 0, 200]))
        .save(&wm_path)
        .unwrap();

    let items = vec![BatchItem {
        source_path: fixture_path(),
        edit: EditState::default(),
        original_name: "wmtest".into(),
    }];
    let opts = BatchOptions {
        format: ExportFormat::Jpeg { quality: 80 },
        resize: ExportResize::LongEdge(256),
        output_dir: dir.path().to_path_buf(),
        name_pattern: "{name}_wm".into(),
        watermark: Some(WatermarkStamp {
            png_path: wm_path,
            anchor: WatermarkAnchor::BottomRight,
            size_pct: 25.0,
            opacity: 0.8,
            margin_pct: 5.0,
            rotation_deg: 0.0,
        }),
        watermark_preset: None,
    };
    let results = export_batch(&rd, &items, &opts, |_, _, _| {});
    assert_eq!(results.len(), 1);
    assert!(results[0].error.is_none(), "error: {:?}", results[0].error);
    assert!(results[0].output_path.as_ref().unwrap().exists());
}

#[test]
fn export_with_text_layer_completes() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let dir = tempfile::tempdir().unwrap();
    let mut preset = WatermarkPreset::new("text-test".into());
    preset.layers.push(WatermarkLayer::Text(TextLayer {
        text: "© chalkraw".into(),
        font_size_pct: 3.0,
        color: TextColor { r: 255, g: 255, b: 255, a: 255 },
        anchor: chalkraw_core::WatermarkAnchor::BottomRight,
        opacity: 0.9,
        margin_pct: 3.0,
        rotation_deg: 0.0,
    }));

    let items = vec![BatchItem {
        source_path: fixture_path(),
        edit: EditState::default(),
        original_name: "txt".into(),
    }];
    let opts = BatchOptions {
        format: ExportFormat::Jpeg { quality: 80 },
        resize: ExportResize::LongEdge(512),
        output_dir: dir.path().to_path_buf(),
        name_pattern: "{name}_text".into(),
        watermark: None,
        watermark_preset: Some(preset),
    };
    let results = export_batch(&rd, &items, &opts, |_, _, _| {});
    assert_eq!(results.len(), 1);
    assert!(results[0].error.is_none(), "error: {:?}", results[0].error);
    assert!(results[0].output_path.as_ref().unwrap().exists());
}

#[test]
fn export_with_watermark_preset_composites_two_layers() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let dir = tempfile::tempdir().unwrap();
    let wm1_path = dir.path().join("wm1.png");
    let wm2_path = dir.path().join("wm2.png");
    image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(8, 8, image::Rgba([255, 0, 0, 200]))
        .save(&wm1_path)
        .unwrap();
    image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(8, 8, image::Rgba([0, 255, 0, 200]))
        .save(&wm2_path)
        .unwrap();

    let mut preset = WatermarkPreset::new("test".into());
    preset.layers.push(WatermarkLayer::Image(ImageLayer {
        png_path: wm1_path,
        anchor: chalkraw_core::WatermarkAnchor::TopLeft,
        size_pct: 25.0,
        opacity: 1.0,
        margin_pct: 5.0,
        rotation_deg: 0.0,
    }));
    preset.layers.push(WatermarkLayer::Image(ImageLayer {
        png_path: wm2_path,
        anchor: chalkraw_core::WatermarkAnchor::BottomRight,
        size_pct: 25.0,
        opacity: 1.0,
        margin_pct: 5.0,
        rotation_deg: 0.0,
    }));

    let items = vec![BatchItem {
        source_path: fixture_path(),
        edit: EditState::default(),
        original_name: "twolayers".into(),
    }];
    let opts = BatchOptions {
        format: ExportFormat::Jpeg { quality: 80 },
        resize: ExportResize::LongEdge(512),
        output_dir: dir.path().to_path_buf(),
        name_pattern: "{name}_two".into(),
        watermark: None,
        watermark_preset: Some(preset),
    };
    let results = export_batch(&rd, &items, &opts, |_, _, _| {});
    assert_eq!(results.len(), 1);
    assert!(results[0].error.is_none(), "error: {:?}", results[0].error);
    assert!(results[0].output_path.as_ref().unwrap().exists());
}

/// Rotating a 4×8 RGBA image by 90° should produce an 8×4 image.
/// This verifies `rotate_image` is wired correctly and that the lossless
/// 90°-snap path works without GPU involvement.
#[test]
fn watermark_rotation_90_swaps_image_dimensions() {
    let img = image::ImageBuffer::<image::Rgba<u8>, _>::from_pixel(
        4, 8, image::Rgba([255, 0, 0, 255]),
    );
    let rotated = chalkraw_export::rotate_image(&img, 90.0);
    assert_eq!(rotated.dimensions(), (8, 4));
}
