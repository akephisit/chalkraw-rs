use chalkraw_core::EditState;
use chalkraw_export::{
    export_batch, BatchItem, BatchOptions, ExportFormat, ExportResize, WatermarkAnchor,
    WatermarkStamp,
};
use chalkraw_render::RenderDevice;
use std::path::PathBuf;

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
        }),
    };
    let results = export_batch(&rd, &items, &opts, |_, _, _| {});
    assert_eq!(results.len(), 1);
    assert!(results[0].error.is_none(), "error: {:?}", results[0].error);
    assert!(results[0].output_path.as_ref().unwrap().exists());
}
