// Erzeugt das Programmsymbol (.ico) und bettet es unter Windows in die .exe ein.

#[path = "src/icon.rs"]
mod icon;

use std::io::Write;

fn write_ico(path: &std::path::Path) {
    let sizes = [16u32, 24, 32, 48, 64, 128];
    let mut images = Vec::new();
    for &s in &sizes {
        let rgba = icon::icon_rgba(s);
        let mut bmp = Vec::new();
        // BITMAPINFOHEADER
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&(s as i32).to_le_bytes());
        bmp.extend_from_slice(&((s * 2) as i32).to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&32u16.to_le_bytes());
        bmp.extend_from_slice(&[0u8; 24]);
        // Pixel (BGRA, von unten nach oben)
        for y in (0..s).rev() {
            for x in 0..s {
                let i = ((y * s + x) * 4) as usize;
                bmp.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
            }
        }
        // AND-Maske (alles 0, Alpha wird verwendet)
        let row = s.div_ceil(32) * 4;
        bmp.extend(std::iter::repeat_n(0u8, (row * s) as usize));
        images.push((s, bmp));
    }
    let mut f = std::fs::File::create(path).unwrap();
    f.write_all(&[0, 0, 1, 0]).unwrap();
    f.write_all(&(images.len() as u16).to_le_bytes()).unwrap();
    let mut offset = 6 + 16 * images.len() as u32;
    for (s, data) in &images {
        let b = if *s >= 256 { 0 } else { *s as u8 };
        f.write_all(&[b, b, 0, 0]).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap();
        f.write_all(&32u16.to_le_bytes()).unwrap();
        f.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        f.write_all(&offset.to_le_bytes()).unwrap();
        offset += data.len() as u32;
    }
    for (_, data) in &images {
        f.write_all(data).unwrap();
    }
}

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico = out.join("easymysql.ico");
    write_ico(&ico);
    // Kopie fuer das Setup-Skript
    let _ = std::fs::create_dir_all("target");
    let _ = std::fs::copy(&ico, "target/easymysql.ico");

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico.to_str().unwrap());
        res.set("ProductName", "EasyMySQL");
        res.set("FileDescription", "EasyMySQL");
        res.set("CompanyName", "EasyMySQL");
        res.set("LegalCopyright", "MIT-Lizenz");
        res.compile().unwrap();
    }
}
