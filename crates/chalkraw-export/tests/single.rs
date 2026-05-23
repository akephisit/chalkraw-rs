use chalkraw_core::EditState;
use chalkraw_export::{export_current, ExportFormat, ExportOptions, ExportResize};
use chalkraw_io::decode_image;
use chalkraw_render::RenderDevice;
use std::path::PathBuf;

fn fixture_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); p.pop();
    p.push("tests/fixtures/sample.jpg");
    p
}

#[test]
fn exports_jpeg_at_original_size() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skip: no GPU"); return; }
    };
    let img = decode_image(fixture_path()).unwrap();
    let edit = EditState::default();
    let tmp = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
    export_current(&rd, &img, &edit, tmp.path(), ExportOptions {
        format: ExportFormat::Jpeg { quality: 90 },
        resize: ExportResize::Original,
    }).unwrap();

    let decoded = image::open(tmp.path()).unwrap();
    assert_eq!(decoded.width(), img.width);
    assert_eq!(decoded.height(), img.height);
}

#[test]
fn exports_png_resized_long_edge() {
    let rd = match RenderDevice::new_headless() {
        Ok(rd) => rd,
        Err(_) => { eprintln!("skip: no GPU"); return; }
    };
    let img = decode_image(fixture_path()).unwrap();
    let edit = EditState::default();
    let tmp = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
    export_current(&rd, &img, &edit, tmp.path(), ExportOptions {
        format: ExportFormat::Png,
        resize: ExportResize::LongEdge(512),
    }).unwrap();
    let decoded = image::open(tmp.path()).unwrap();
    assert!(decoded.width() <= 512 && decoded.height() <= 512);
    assert!(decoded.width() == 512 || decoded.height() == 512);
}
