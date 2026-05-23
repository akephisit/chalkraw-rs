/// One-off tool: generates tests/fixtures/sample.jpg (1024×768) for use in unit tests.
/// Run with: cargo run -p chalkraw-io --example gen_fixture
use image::{ImageBuffer, Rgb};
use std::path::Path;

fn main() {
    let width = 1024u32;
    let height = 768u32;

    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(width, height, |x, _y| {
        // Base colour teal
        let mut r: u8 = 40;
        let mut g: u8 = 80;
        let b: u8 = 120;
        // Add yellow stripes every 64px
        if (x / 64) % 2 == 0 {
            r = 220;
            g = 200;
        }
        Rgb([r, g, b])
    });

    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures");
    std::fs::create_dir_all(out_dir).unwrap();
    let out_path = Path::new(out_dir).join("sample.jpg");
    img.save(&out_path).expect("failed to save JPEG");
    println!("Wrote {}", out_path.display());
}
