fn main() {
    // Keep the repository text-only while still providing the tiny Windows
    // resource icon required by tauri-build during local development.
    let icon_dir = std::path::Path::new("icons");
    let icon = icon_dir.join("icon.ico");
    std::fs::create_dir_all(icon_dir).expect("create icon directory");
    // 1x1, 32-bit BGRA ICO (header + DIB + pixel + AND mask).
    const ICO: &[u8] = &[
        0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 32, 0, 48, 0, 0, 0, 22, 0, 0, 0, 40, 0, 0, 0, 1, 0, 0,
        0, 2, 0, 0, 0, 1, 0, 32, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 255, 169, 119, 255, 0, 0, 0, 0,
    ];
    let needs_write = std::fs::read(&icon)
        .map(|bytes| bytes != ICO)
        .unwrap_or(true);
    if needs_write {
        std::fs::write(&icon, ICO).expect("write development icon");
    }
    tauri_build::build()
}
