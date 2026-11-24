//! PNG encoding/decoding (rented: the `png` crate) and image comparison.

use std::io::Cursor;

/// An RGBA8 image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Decode any 8-bit-or-less PNG into RGBA8.
pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut dec = png::Decoder::new(Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let size = reader.output_buffer_size().ok_or("image too large")?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let (w, h) = (info.width, info.height);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => {
            buf.as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
        }
        png::ColorType::Indexed => return Err("indexed PNG was not expanded".into()),
    };
    Ok(Image { width: w, height: h, rgba })
}

/// Result of comparing an image with a reference.
#[derive(Clone, Debug)]
pub struct Diff {
    pub same_size: bool,
    pub diff_pixels: u64,
    pub total_pixels: u64,
    /// Largest per-channel difference.
    pub max_delta: u8,
    /// Visualization: differing pixels red, the rest a dim copy of `actual`.
    pub image: Option<Image>,
}

pub fn diff(actual: &Image, reference: &Image) -> Diff {
    if actual.width != reference.width || actual.height != reference.height {
        return Diff {
            same_size: false,
            diff_pixels: actual.width as u64 * actual.height as u64,
            total_pixels: actual.width as u64 * actual.height as u64,
            max_delta: 255,
            image: None,
        };
    }
    let mut diff_pixels = 0;
    let mut max_delta = 0u8;
    let mut vis = Vec::with_capacity(actual.rgba.len());
    for (a, r) in actual.rgba.as_chunks::<4>().0.iter().zip(reference.rgba.as_chunks::<4>().0.iter()) {
        let d = a.iter().zip(r).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
        if d > 0 {
            diff_pixels += 1;
            max_delta = max_delta.max(d);
            vis.extend_from_slice(&[255, 0, 0, 255]);
        } else {
            let g = ((a[0] as u32 * 3 + a[1] as u32 * 6 + a[2] as u32) / 10 / 3) as u8;
            vis.extend_from_slice(&[g, g, g, 255]);
        }
    }
    Diff {
        same_size: true,
        diff_pixels,
        total_pixels: actual.width as u64 * actual.height as u64,
        max_delta,
        image: Some(Image { width: actual.width, height: actual.height, rgba: vis }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trip_and_diff() {
        let img = Image { width: 2, height: 1, rgba: vec![1, 2, 3, 255, 9, 8, 7, 255] };
        let bytes = encode_png(img.width, img.height, &img.rgba).unwrap();
        let back = decode_png(&bytes).unwrap();
        assert_eq!(back, img);
        let mut other = img.clone();
        other.rgba[4] = 19;
        let d = diff(&other, &img);
        assert_eq!((d.diff_pixels, d.max_delta), (1, 10));
    }
}
