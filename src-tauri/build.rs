use std::path::{Path, PathBuf};

fn main() {
    // Create a minimal placeholder icon if one doesn't exist yet.
    let icon_dir = std::path::Path::new("icons");
    let icon_path = icon_dir.join("icon.ico");
    if !icon_path.exists() {
        std::fs::create_dir_all(icon_dir).ok();
        std::fs::write(&icon_path, minimal_ico()).expect("writing placeholder icon");
    }

    // Compile the seeded Dilithium2 keygen C code.
    // It links against PQClean internal functions already provided by
    // pqcrypto-dilithium. We only need the header files for types/constants.
    let (dilithium_headers, common_headers) = find_pqclean_headers();
    cc::Build::new()
        .file("c/seeded_keygen.c")
        .include(&dilithium_headers)
        .include(&common_headers)
        .compile("chronx_seeded_keygen");

    tauri_build::build()
}

/// Locate the PQClean header directories inside the cargo registry.
fn find_pqclean_headers() -> (PathBuf, PathBuf) {
    let cargo_home = std::env::var("CARGO_HOME").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| ".".to_string());
        format!("{}/.cargo", home)
    });
    let registry = Path::new(&cargo_home).join("registry").join("src");
    if let Ok(entries) = std::fs::read_dir(&registry) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("pqcrypto-dilithium-0.5.0");
            if candidate.exists() {
                let clean = candidate.join("pqclean/crypto_sign/dilithium2/clean");
                let common = candidate.join("pqclean/common");
                if clean.exists() && common.exists() {
                    return (clean, common);
                }
            }
        }
    }
    panic!("pqcrypto-dilithium-0.5.0 headers not found in cargo registry");
}

/// Generate a minimal valid 16×16 32bpp ICO file (dark-blue square).
fn minimal_ico() -> Vec<u8> {
    let w: u32 = 16;
    let h: u32 = 16;

    // BITMAPINFOHEADER: 40 bytes
    let mut bmp: Vec<u8> = Vec::new();
    bmp.extend_from_slice(&40u32.to_le_bytes()); // biSize
    bmp.extend_from_slice(&w.to_le_bytes()); // biWidth
    bmp.extend_from_slice(&(h * 2).to_le_bytes()); // biHeight (doubled in ICO)
    bmp.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    bmp.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    bmp.extend_from_slice(&0u32.to_le_bytes()); // biCompression
    bmp.extend_from_slice(&0u32.to_le_bytes()); // biSizeImage
    bmp.extend_from_slice(&0u32.to_le_bytes()); // biXPelsPerMeter
    bmp.extend_from_slice(&0u32.to_le_bytes()); // biYPelsPerMeter
    bmp.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    bmp.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    // BGRA pixel data: dark blue, fully opaque
    for _ in 0..(w * h) {
        bmp.extend_from_slice(&[0x1d, 0x4e, 0xd8, 0xff]);
    }

    // AND mask: w pixels per row, each row padded to 4 bytes
    // 16 pixels / 8 = 2 bytes → padded to 4 bytes per row
    for _ in 0..h {
        bmp.extend_from_slice(&[0u8; 4]);
    }

    let img_size = bmp.len() as u32;
    let img_offset: u32 = 6 + 16; // ICO header (6) + one dir entry (16)

    let mut ico: Vec<u8> = Vec::new();
    // ICO file header: 6 bytes
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type = ICO
    ico.extend_from_slice(&1u16.to_le_bytes()); // image count

    // Directory entry: 16 bytes
    ico.push(w as u8); // width
    ico.push(h as u8); // height
    ico.push(0); // color count
    ico.push(0); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // planes
    ico.extend_from_slice(&32u16.to_le_bytes()); // bit count
    ico.extend_from_slice(&img_size.to_le_bytes()); // image data size
    ico.extend_from_slice(&img_offset.to_le_bytes()); // offset to image data

    ico.extend(bmp);
    ico
}
