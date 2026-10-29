//! Packed DIBs, the image format of the Windows clipboard (`CF_DIB`, `CF_DIBV5`): a bitmap
//! header, maybe color masks, then the pixel rows. Plain byte work, so it's tested on Linux too.

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;
/// `BITMAPINFOHEADER`; the V4 and V5 headers extend it.
const HEADER: usize = 40;

/// `(width, height, RGBA rows top to bottom)` of a 24 or 32 bit DIB, uncompressed or with
/// color masks. Palettes and compressed DIBs give `None`.
pub fn to_rgba(dib: &[u8]) -> Option<(i32, i32, Vec<u8>)> {
    let u32_at = |i: usize| Some(u32::from_le_bytes(dib.get(i..i + 4)?.try_into().ok()?));
    let size = u32_at(0)? as usize;
    let width = u32_at(4)? as i32;
    let height = u32_at(8)? as i32;
    let bits = u16::from_le_bytes(dib.get(14..16)?.try_into().ok()?);
    let compression = u32_at(16)?;
    let colors = u32_at(32)? as usize;
    if size < HEADER || width <= 0 || height == 0 || !matches!(bits, 24 | 32) {
        return None;
    }
    let (masks, start) = match compression {
        BI_RGB => ([0x00ff_0000, 0x0000_ff00, 0x0000_00ff, if bits == 32 { 0xff00_0000 } else { 0 }], size + colors * 4),
        // V2 headers and up carry the masks (V3 and up with alpha), a plain header is followed by them
        BI_BITFIELDS if bits == 32 && size >= 52 => {
            let alpha = if size >= 56 { u32_at(52)? } else { 0 };
            ([u32_at(40)?, u32_at(44)?, u32_at(48)?, alpha], size)
        }
        BI_BITFIELDS if bits == 32 => ([u32_at(HEADER)?, u32_at(HEADER + 4)?, u32_at(HEADER + 8)?, 0], size + 12),
        _ => return None,
    };
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    let step = usize::from(bits / 8);
    let stride = (w * step).div_ceil(4) * 4;
    let pixels = dib.get(start..start.checked_add(stride.checked_mul(h)?)?)?;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        // positive height: the bottom row comes first
        let row = if height > 0 { h - 1 - y } else { y };
        for px in pixels[row * stride..][..w * step].chunks_exact(step) {
            let value = px.iter().rev().fold(0u32, |v, b| v << 8 | u32::from(*b));
            rgba.extend(masks.map(|m| channel(value, m)));
        }
    }
    // most programs leave the fourth byte at 0: then it isn't alpha
    if rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
        rgba.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p[3] = 255);
    }
    Some((width, h as i32, rgba))
}

/// One color channel scaled to 0–255.
fn channel(value: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = u64::from(mask >> shift);
    (u64::from((value & mask) >> shift) * 255 / max) as u8
}

/// A 32 bit bottom-up DIB of RGBA rows (top to bottom), the layout every program reads.
pub fn from_rgba(width: i32, height: i32, rgba: &[u8]) -> Vec<u8> {
    let row = width as usize * 4;
    let mut dib = Vec::with_capacity(HEADER + rgba.len());
    for field in [HEADER as u32, width as u32, height as u32] {
        dib.extend(field.to_le_bytes());
    }
    dib.extend(1u16.to_le_bytes()); // planes
    dib.extend(32u16.to_le_bytes());
    dib.extend(BI_RGB.to_le_bytes());
    dib.extend((rgba.len() as u32).to_le_bytes());
    dib.resize(HEADER, 0); // resolution and palette: unused
    for line in rgba.chunks_exact(row).rev() {
        for [r, g, b, a] in line.as_chunks::<4>().0 {
            dib.extend([b, g, r, a]);
        }
    }
    dib
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain header for a `width`×`height` DIB.
    fn header(width: i32, height: i32, bits: u16, compression: u32) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend(40u32.to_le_bytes());
        h.extend(width.to_le_bytes());
        h.extend(height.to_le_bytes());
        h.extend(1u16.to_le_bytes());
        h.extend(bits.to_le_bytes());
        h.extend(compression.to_le_bytes());
        h.resize(40, 0);
        h
    }

    #[test]
    fn round_trip() {
        let rgba = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24];
        let dib = from_rgba(3, 2, &rgba);
        assert_eq!(dib.len(), 40 + 24);
        assert_eq!(to_rgba(&dib), Some((3, 2, rgba.to_vec())));
    }

    #[test]
    fn bottom_up_24_bit_rows_are_padded() {
        // 1×2, BGR rows padded to 4 bytes; the first row in the data is the bottom one
        let mut dib = header(1, 2, 24, BI_RGB);
        dib.extend([0, 0, 255, 0, 255, 0, 0, 0]);
        assert_eq!(to_rgba(&dib), Some((1, 2, vec![0, 0, 255, 255, 255, 0, 0, 255])));
    }

    #[test]
    fn top_down() {
        let mut dib = header(1, -2, 32, BI_RGB);
        dib.extend([0, 0, 255, 128, 255, 0, 0, 0]);
        assert_eq!(to_rgba(&dib), Some((1, 2, vec![255, 0, 0, 128, 0, 0, 255, 0])));
    }

    #[test]
    fn zero_alpha_means_opaque() {
        let mut dib = header(1, 1, 32, BI_RGB);
        dib.extend([10, 20, 30, 0]);
        assert_eq!(to_rgba(&dib), Some((1, 1, vec![30, 20, 10, 255])));
    }

    #[test]
    fn masks_after_a_plain_header() {
        // RGBX instead of the usual BGRX
        let mut dib = header(1, 1, 32, BI_BITFIELDS);
        for mask in [0x0000_00ffu32, 0x0000_ff00, 0x00ff_0000] {
            dib.extend(mask.to_le_bytes());
        }
        dib.extend([10, 20, 30, 0]);
        assert_eq!(to_rgba(&dib), Some((1, 1, vec![10, 20, 30, 255])));
    }

    #[test]
    fn masks_in_a_v5_header() {
        let mut dib = header(1, 1, 32, BI_BITFIELDS);
        dib[..4].copy_from_slice(&124u32.to_le_bytes());
        for mask in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff, 0xff00_0000] {
            dib.extend(mask.to_le_bytes());
        }
        dib.resize(124, 0);
        dib.extend([10, 20, 30, 40]);
        assert_eq!(to_rgba(&dib), Some((1, 1, vec![30, 20, 10, 40])));
    }

    #[test]
    fn rejects_what_it_cant_read() {
        assert_eq!(to_rgba(&[]), None);
        assert_eq!(to_rgba(&header(1, 1, 8, BI_RGB)), None);
        // too short for its pixels
        assert_eq!(to_rgba(&header(2, 2, 32, BI_RGB)), None);
    }
}
