//! Bounded decoding and small, square toolbar avatars. Never embeds artwork.
use image::{imageops, DynamicImage, ImageFormat, ImageReader, Limits, RgbaImage};
use std::io::Cursor;

pub const MAX_UPLOAD_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SIDE: u32 = 4096;
pub const OUTPUT_SIDE: u32 = 256;
pub const MAX_STORED_BYTES: usize = 512 * 1024;

pub fn square_pad_png(input: &[u8]) -> Result<Vec<u8>, String> {
    if input.len() > MAX_UPLOAD_BYTES {
        return Err("图片太大（上限 8MB）".into());
    }
    let mut reader = ImageReader::new(Cursor::new(input))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    if !matches!(
        reader.format(),
        Some(
            ImageFormat::Png
                | ImageFormat::Jpeg
                | ImageFormat::WebP
                | ImageFormat::Gif
                | ImageFormat::Bmp
        )
    ) {
        return Err("请选择 PNG、JPEG、WebP、GIF 或 BMP 图片".into());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|e| format!("图片无法解码，边长上限 4096 像素：{e}"))?;
    if img.width() == 0 || img.height() == 0 {
        return Err("图片没有像素".into());
    }
    let img = img.thumbnail(OUTPUT_SIDE, OUTPUT_SIDE).to_rgba8();
    let side = img.width().max(img.height());
    let mut canvas = RgbaImage::new(side, side);
    imageops::overlay(
        &mut canvas,
        &img,
        i64::from((side - img.width()) / 2),
        i64::from((side - img.height()) / 2),
    );
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(canvas)
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|e| format!("写成 PNG 失败：{e}"))?;
    Ok(out.into_inner())
}

pub fn validate_stored(input: &[u8]) -> Result<(), String> {
    if input.len() > MAX_STORED_BYTES || !input.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("保存的头像无效，请重新选择图片".into());
    }
    let reader = ImageReader::new(Cursor::new(input))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || w != h || w > OUTPUT_SIDE {
        return Err("头像需要重新导入（使用不大于 256 像素的方形 PNG）".into());
    }
    square_pad_png(input).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 255])))
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }
    #[test]
    fn pads_and_bounds_output() {
        let out = square_pad_png(&png(400, 800)).unwrap();
        validate_stored(&out).unwrap();
        let img = image::load_from_memory(&out).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (256, 256));
        assert_eq!(img.get_pixel(0, 128)[3], 0);
        assert_eq!(img.get_pixel(128, 128)[3], 255);
    }
    #[test]
    fn rejects_dimensions_before_full_decode() {
        assert!(square_pad_png(&png(MAX_SIDE + 1, 1)).is_err());
        assert!(square_pad_png(b"not an image").is_err());
        assert!(validate_stored(&png(257, 257)).is_err());
    }
}
