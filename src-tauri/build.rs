fn main() {
    // Create a minimal placeholder icon if one doesn't exist yet.
    let icon_dir = std::path::Path::new("icons");
    let icon_path = icon_dir.join("icon.ico");
    if !icon_path.exists() {
        std::fs::create_dir_all(icon_dir).ok();
        std::fs::write(&icon_path, minimal_ico()).expect("writing placeholder icon");
    }

    tauri_build::build()
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
