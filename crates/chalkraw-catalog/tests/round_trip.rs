use chalkraw_catalog::Catalog;
use chalkraw_core::{EditState, ImageFormat, Photo};
use chalkraw_io::decode_image;
use std::path::PathBuf;
use tempfile::tempdir;

fn fixture_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("tests/fixtures/sample.jpg");
    p
}

#[test]
fn decode_then_persist_then_reload_restores_edit() {
    let dir = tempdir().unwrap();
    let cat_path = dir.path().join("e2e.chalkraw");

    // 1. Decode fixture.
    let img = decode_image(fixture_path()).unwrap();
    assert_eq!(img.width, 1024);

    // 2. Create catalog, insert photo, store an edit.
    let photo_id = {
        let cat = Catalog::open_or_create(&cat_path, "e2e").unwrap();
        let p = Photo::new(
            fixture_path(),
            [0u8; 32],
            img.width,
            img.height,
            ImageFormat::Jpeg,
        );
        cat.insert_photo(&p).unwrap();
        let mut e = EditState::default();
        e.tone.exposure = 1.7;
        cat.upsert_edit(p.id, &e).unwrap();
        p.id
    };

    // 3. Re-open catalog and verify state.
    let cat = Catalog::open_or_create(&cat_path, "ignored").unwrap();
    let photos = cat.list_photos().unwrap();
    assert_eq!(photos.len(), 1);
    assert_eq!(photos[0].id, photo_id);
    let e = cat.get_edit(photo_id).unwrap();
    assert_eq!(e.tone.exposure, 1.7);
    assert_eq!(e.white_balance.temp_kelvin, 5500.0);
}
