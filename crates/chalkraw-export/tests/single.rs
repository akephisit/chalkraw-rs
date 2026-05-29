use chalkraw_core::EditState;
use chalkraw_export::{export_current, ExportFormat, ExportOptions, ExportResize};
use chalkraw_io::decode_image;
use chalkraw_render::RenderDevice;
use std::path::PathBuf;

/// Smoke test: exporting with clarity=100 produces pixels that differ from
/// clarity=0. Proves that the Phase 2E blur passes are actually running in
/// export_current (they were silently zero before v0.15.4).
#[test]
fn clarity_affects_exported_pixels() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let img = decode_image(fixture_path()).unwrap();

    // Export with clarity = 0 (baseline).
    let tmp0 = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
    let edit0 = EditState::default(); // clarity defaults to 0
    export_current(
        &rd,
        &img,
        &edit0,
        tmp0.path(),
        ExportOptions {
            format: ExportFormat::Png,
            resize: ExportResize::LongEdge(128),
        },
    )
    .unwrap();

    // Export with clarity = 100.
    let tmp100 = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
    let mut edit100 = EditState::default();
    edit100.presence.clarity = 100.0;
    export_current(
        &rd,
        &img,
        &edit100,
        tmp100.path(),
        ExportOptions {
            format: ExportFormat::Png,
            resize: ExportResize::LongEdge(128),
        },
    )
    .unwrap();

    let pixels0 = image::open(tmp0.path()).unwrap().to_rgb8().into_raw();
    let pixels100 = image::open(tmp100.path()).unwrap().to_rgb8().into_raw();

    // At least some pixels must differ; if every pixel is identical the blur
    // passes are still not running.
    let differing = pixels0
        .iter()
        .zip(pixels100.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        differing > 0,
        "clarity=100 and clarity=0 exports are identical — Phase 2E blurs are not running in export"
    );
}

fn fixture_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("tests/fixtures/sample.jpg");
    p
}

#[test]
fn exports_jpeg_at_original_size() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let img = decode_image(fixture_path()).unwrap();
    let edit = EditState::default();
    let tmp = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
    export_current(
        &rd,
        &img,
        &edit,
        tmp.path(),
        ExportOptions {
            format: ExportFormat::Jpeg { quality: 90 },
            resize: ExportResize::Original,
        },
    )
    .unwrap();

    let decoded = image::open(tmp.path()).unwrap();
    assert_eq!(decoded.width(), img.width);
    assert_eq!(decoded.height(), img.height);
}

#[test]
fn exports_png_resized_long_edge() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => {
            eprintln!("skip: no GPU");
            return;
        }
    };
    let img = decode_image(fixture_path()).unwrap();
    let edit = EditState::default();
    let tmp = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
    export_current(
        &rd,
        &img,
        &edit,
        tmp.path(),
        ExportOptions {
            format: ExportFormat::Png,
            resize: ExportResize::LongEdge(512),
        },
    )
    .unwrap();
    let decoded = image::open(tmp.path()).unwrap();
    assert!(decoded.width() <= 512 && decoded.height() <= 512);
    assert!(decoded.width() == 512 || decoded.height() == 512);
}
