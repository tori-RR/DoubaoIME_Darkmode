//! A data-only PE image. Contains generated icon pixels, never vendor code/assets.
use crate::avatar;
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use std::io::Cursor;
use std::sync::{Mutex, OnceLock};

const RVA: u32 = 4096;
const SIZES: [u32; 10] = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256];

struct CachedResource {
    key: String,
    bytes: Vec<u8>,
}

fn word(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn dword(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn align(value: usize, alignment: usize) -> usize {
    value.div_ceil(alignment) * alignment
}
fn directory(out: &mut [u8], at: usize, entries: &[(u32, u32)]) {
    word(out, at + 14, entries.len() as u16);
    for (i, (id, target)) in entries.iter().enumerate() {
        dword(out, at + 16 + i * 8, *id);
        dword(out, at + 20 + i * 8, *target);
    }
}

pub fn build(png: &[u8]) -> Result<Vec<u8>, String> {
    avatar::validate_stored(png)?;
    // Status is refreshed for theme/font edits too. Keep at most one generated
    // DLL in memory so unchanged taskbar pixels do not trigger ten PNG encodes.
    static CACHE: OnceLock<Mutex<Option<CachedResource>>> = OnceLock::new();
    let key = crate::safe_fs::hash(png);
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(entry) = cache.lock() {
        if let Some(value) = &*entry {
            if value.key == key {
                return Ok(value.bytes.clone());
            }
        }
    }
    let value = build_uncached(png)?;
    if let Ok(mut entry) = cache.lock() {
        *entry = Some(CachedResource {
            key,
            bytes: value.clone(),
        });
    }
    Ok(value)
}

fn build_uncached(png: &[u8]) -> Result<Vec<u8>, String> {
    let source = image::load_from_memory(png).map_err(|e| e.to_string())?;
    let mut pixels = Vec::new();
    for side in SIZES {
        let mut output = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(
            source
                .resize_exact(side, side, FilterType::Lanczos3)
                .to_rgba8(),
        )
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
        pixels.push(output.into_inner());
    }
    let mut group = vec![0; 6 + 14 * pixels.len()];
    word(&mut group, 2, 1);
    word(&mut group, 4, pixels.len() as u16);
    for (i, (side, pixel)) in SIZES.iter().zip(&pixels).enumerate() {
        let at = 6 + 14 * i;
        group[at] = *side as u8;
        group[at + 1] = *side as u8;
        word(&mut group, at + 4, 1);
        word(&mut group, at + 6, 32);
        dword(&mut group, at + 8, pixel.len() as u32);
        word(&mut group, at + 12, (i + 1) as u16);
    }
    pixels.push(group);
    // Root -> type -> id -> language -> IMAGE_RESOURCE_DATA_ENTRY -> bytes.
    let icons = 32;
    let groups = icons + 16 + SIZES.len() * 8;
    let languages = groups + 24;
    let data_entries = languages + pixels.len() * 24;
    let data_start = data_entries + pixels.len() * 16;
    let mut resources = vec![0; data_start];
    directory(
        &mut resources,
        0,
        &[
            (3, 0x8000_0000 | icons as u32),
            (14, 0x8000_0000 | groups as u32),
        ],
    );
    let ids: Vec<_> = (0..SIZES.len())
        .map(|i| ((i + 1) as u32, 0x8000_0000 | (languages + i * 24) as u32))
        .collect();
    directory(&mut resources, icons, &ids);
    directory(
        &mut resources,
        groups,
        &[(1, 0x8000_0000 | (languages + SIZES.len() * 24) as u32)],
    );
    for (i, pixel) in pixels.iter().enumerate() {
        directory(
            &mut resources,
            languages + i * 24,
            &[(0, (data_entries + i * 16) as u32)],
        );
        let at = align(resources.len(), 4);
        resources.resize(at, 0);
        dword(&mut resources, data_entries + i * 16, RVA + at as u32);
        dword(
            &mut resources,
            data_entries + i * 16 + 4,
            pixel.len() as u32,
        );
        resources.extend_from_slice(pixel);
    }
    let raw_size = align(resources.len(), 512);
    let mut out = vec![0; 512 + raw_size];
    out[..2].copy_from_slice(b"MZ");
    dword(&mut out, 0x3c, 64);
    out[64..68].copy_from_slice(b"PE\0\0");
    word(&mut out, 68, 0x8664); // AMD64; loaded as data, not executable code.
    word(&mut out, 70, 1);
    word(&mut out, 84, 240);
    word(&mut out, 86, 0x2022);
    let opt = 88;
    word(&mut out, opt, 0x20b);
    dword(&mut out, opt + 8, raw_size as u32);
    out[opt + 24..opt + 32].copy_from_slice(&0x0001_8000_0000u64.to_le_bytes());
    dword(&mut out, opt + 32, 4096);
    dword(&mut out, opt + 36, 512);
    word(&mut out, opt + 40, 6);
    word(&mut out, opt + 48, 6);
    dword(
        &mut out,
        opt + 56,
        align(RVA as usize + resources.len(), 4096) as u32,
    );
    dword(&mut out, opt + 60, 512);
    word(&mut out, opt + 68, 2);
    word(&mut out, opt + 70, 0x140);
    out[opt + 72..opt + 80].copy_from_slice(&0x100000u64.to_le_bytes());
    out[opt + 80..opt + 88].copy_from_slice(&0x1000u64.to_le_bytes());
    out[opt + 88..opt + 96].copy_from_slice(&0x100000u64.to_le_bytes());
    out[opt + 96..opt + 104].copy_from_slice(&0x1000u64.to_le_bytes());
    dword(&mut out, opt + 108, 16);
    dword(&mut out, opt + 128, RVA);
    dword(&mut out, opt + 132, resources.len() as u32);
    let section = opt + 240;
    out[section..section + 8].copy_from_slice(b".rsrc\0\0\0");
    dword(&mut out, section + 8, resources.len() as u32);
    dword(&mut out, section + 12, RVA);
    dword(&mut out, section + 16, raw_size as u32);
    dword(&mut out, section + 20, 512);
    dword(&mut out, section + 36, 0x4000_0040); // READ | INITIALIZED_DATA, never EXECUTE.
    out[512..512 + resources.len()].copy_from_slice(&resources);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            32,
            32,
            image::Rgba([17, 83, 213, 255]),
        ))
        .write_to(&mut out, ImageFormat::Png)
        .unwrap();
        out.into_inner()
    }
    #[test]
    fn deterministic_resource_only_pe() {
        let pe = build(&input()).unwrap();
        assert_eq!(pe, build(&input()).unwrap());
        assert_eq!(&pe[64..68], b"PE\0\0");
        assert_eq!(&pe[88 + 16..88 + 20], &[0; 4]); // no entrypoint
        assert_eq!(&pe[88 + 112 + 8..88 + 112 + 16], &[0; 8]); // no imports
        assert_eq!(
            u32::from_le_bytes(pe[364..368].try_into().unwrap()) & 0x2000_0000,
            0
        );
        assert!(build(b"bad").is_err());
    }
    #[test]
    fn windows_extracts_generated_large_and_small_icons() {
        use std::os::windows::ffi::OsStrExt;
        let dir = crate::workdir::new_scratch().unwrap();
        let path = dir.join("test-resource.dll");
        crate::safe_fs::atomic_write(&path, &build(&input()).unwrap()).unwrap();
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut large = std::ptr::null_mut();
        let mut small = std::ptr::null_mut();
        let count = unsafe { ExtractIconExW(wide.as_ptr(), 0, &mut large, &mut small, 1) };
        assert!(count > 0 && !large.is_null() && !small.is_null());
        unsafe {
            DestroyIcon(large);
            DestroyIcon(small);
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ExtractIconExW(
            path: *const u16,
            index: i32,
            large: *mut *mut std::ffi::c_void,
            small: *mut *mut std::ffi::c_void,
            count: u32,
        ) -> u32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn DestroyIcon(icon: *mut std::ffi::c_void) -> i32;
    }
}
