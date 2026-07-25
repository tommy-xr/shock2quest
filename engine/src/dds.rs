//! DDS (DirectDraw Surface) decoding.
//!
//! The 25th Anniversary Edition's upgraded textures ship as DDS, overwhelmingly
//! BC7. No GPU we target is guaranteed to accept BC7 natively - Quest in
//! particular does not - and the renderer only ever uploads uncompressed RGBA8
//! anyway, so we decode on the CPU at load time like every other texture format.
//!
//! Only mip level 0 is decoded; the engine builds its own mip chain.

use tracing::trace;

use crate::texture_format::{PixelFormat, RawTextureData, TextureFormat};

const MAGIC: &[u8; 4] = b"DDS ";
const HEADER_LEN: usize = 124; // excludes the 4-byte magic
const PF_FLAG_FOURCC: u32 = 0x4;
const PF_FLAG_RGB: u32 = 0x40;
/// Sanity bound on header-declared dimensions, so surface-size arithmetic can't
/// overflow on a malformed file.
const MAX_DIMENSION: u32 = 16384;

/// The subset of surface formats the 25AE assets actually use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockFormat {
    Bc1,
    Bc2,
    Bc3,
    Bc7,
    /// Uncompressed, described by channel masks rather than a block layout.
    /// Used by the environment cubemaps.
    Uncompressed {
        bits_per_pixel: u32,
        r_mask: u32,
        g_mask: u32,
        b_mask: u32,
        a_mask: u32,
    },
}

impl BlockFormat {
    /// Bytes needed for one mip-0 surface of `width` x `height`.
    fn surface_bytes(self, width: usize, height: usize) -> usize {
        match self {
            BlockFormat::Bc1 => width.div_ceil(4) * height.div_ceil(4) * 8,
            BlockFormat::Bc2 | BlockFormat::Bc3 | BlockFormat::Bc7 => {
                width.div_ceil(4) * height.div_ceil(4) * 16
            }
            BlockFormat::Uncompressed { bits_per_pixel, .. } => {
                width * height * (bits_per_pixel as usize / 8)
            }
        }
    }
}

/// Number of trailing zero bits, i.e. how far to shift a masked channel down.
fn mask_shift(mask: u32) -> u32 {
    if mask == 0 { 0 } else { mask.trailing_zeros() }
}

/// Scale a channel of `mask.count_ones()` bits up to a full 8 bits.
///
/// Widths come from a file-supplied mask, so the arithmetic is done in `u64`: a
/// 32-bit-wide mask would overflow `1u32 << 32` and `value * 255`.
fn scale_to_u8(value: u32, mask: u32) -> u8 {
    let bits = mask.count_ones();
    if bits == 0 {
        return 255;
    }
    let max = (1u64 << bits) - 1;
    ((value as u64 * 255 + max / 2) / max) as u8
}

struct DdsHeader {
    width: u32,
    height: u32,
    format: BlockFormat,
    data_offset: usize,
}

fn u32_at(buf: &[u8], offset: usize) -> Option<u32> {
    let bytes = buf.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn parse_header(buf: &[u8]) -> Option<DdsHeader> {
    if buf.get(0..4)? != MAGIC {
        return None;
    }
    let size = u32_at(buf, 4)?;
    if size as usize != HEADER_LEN {
        return None;
    }
    let height = u32_at(buf, 12)?;
    let width = u32_at(buf, 16)?;

    // DDS_PIXELFORMAT starts at offset 76 (4 magic + 72 into the header).
    let pf_flags = u32_at(buf, 80)?;
    let four_cc = buf.get(84..88)?;

    // Guard the dimensions before any caller multiplies them out: they come
    // straight from the file, and a bogus header would otherwise overflow the
    // surface-size arithmetic (panicking in debug, wrapping to 0 in release).
    // The largest surface the 25AE assets ship is 3840x2160.
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return None;
    }

    if pf_flags & PF_FLAG_FOURCC == 0 {
        // Uncompressed surface: channel layout comes from explicit bit masks.
        // Only straight RGB is understood; luminance/YUV would decode to garbage.
        if pf_flags & PF_FLAG_RGB == 0 {
            return None;
        }
        let bits_per_pixel = u32_at(buf, 88)?;
        if bits_per_pixel != 32 && bits_per_pixel != 24 {
            return None;
        }
        return Some(DdsHeader {
            width,
            height,
            format: BlockFormat::Uncompressed {
                bits_per_pixel,
                r_mask: u32_at(buf, 92)?,
                g_mask: u32_at(buf, 96)?,
                b_mask: u32_at(buf, 100)?,
                a_mask: u32_at(buf, 104)?,
            },
            data_offset: 128,
        });
    }

    let (format, data_offset) = match four_cc {
        b"DXT1" => (BlockFormat::Bc1, 128),
        b"DXT3" => (BlockFormat::Bc2, 128),
        b"DXT5" => (BlockFormat::Bc3, 128),
        b"DX10" => {
            // DDS_HEADER_DXT10 follows the base header; dxgiFormat is its first field.
            let dxgi = u32_at(buf, 128)?;
            let format = match dxgi {
                70..=72 => BlockFormat::Bc1, // BC1_TYPELESS/UNORM/UNORM_SRGB
                73..=75 => BlockFormat::Bc2, // BC2_*
                76..=78 => BlockFormat::Bc3, // BC3_*
                97..=99 => BlockFormat::Bc7, // BC7_TYPELESS/UNORM/UNORM_SRGB
                _ => return None,
            };
            (format, 148)
        }
        _ => return None,
    };

    Some(DdsHeader {
        width,
        height,
        format,
        data_offset,
    })
}

/// `texture2ddecoder` writes each pixel as a packed `0xAARRGGBB` `u32`; the
/// renderer wants tightly-packed RGBA bytes.
fn argb_u32_to_rgba8(pixels: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for px in pixels {
        out.push(((px >> 16) & 0xFF) as u8); // R
        out.push(((px >> 8) & 0xFF) as u8); // G
        out.push((px & 0xFF) as u8); // B
        out.push(((px >> 24) & 0xFF) as u8); // A
    }
    out
}

/// Decode mip level 0 of a DDS buffer to RGBA8. `None` if the buffer is not a
/// DDS we understand, or is truncated.
pub fn decode_rgba8(buffer: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let header = parse_header(buffer)?;
    let (w, h) = (header.width as usize, header.height as usize);

    let expected = header.format.surface_bytes(w, h);
    let data = buffer.get(header.data_offset..header.data_offset + expected)?;

    // Uncompressed surfaces are repacked directly; no block decode involved.
    if let BlockFormat::Uncompressed {
        bits_per_pixel,
        r_mask,
        g_mask,
        b_mask,
        a_mask,
    } = header.format
    {
        let stride = bits_per_pixel as usize / 8;
        let mut out = Vec::with_capacity(w * h * 4);
        for px in data.chunks_exact(stride) {
            let mut raw = 0u32;
            for (i, byte) in px.iter().enumerate() {
                raw |= (*byte as u32) << (8 * i);
            }
            out.push(scale_to_u8((raw & r_mask) >> mask_shift(r_mask), r_mask));
            out.push(scale_to_u8((raw & g_mask) >> mask_shift(g_mask), g_mask));
            out.push(scale_to_u8((raw & b_mask) >> mask_shift(b_mask), b_mask));
            out.push(if a_mask == 0 {
                255
            } else {
                scale_to_u8((raw & a_mask) >> mask_shift(a_mask), a_mask)
            });
        }
        return Some((out, header.width, header.height));
    }

    let mut pixels = vec![0u32; w * h];
    let result = match header.format {
        BlockFormat::Bc1 => texture2ddecoder::decode_bc1(data, w, h, &mut pixels),
        BlockFormat::Bc2 => texture2ddecoder::decode_bc2(data, w, h, &mut pixels),
        BlockFormat::Bc3 => texture2ddecoder::decode_bc3(data, w, h, &mut pixels),
        BlockFormat::Bc7 => texture2ddecoder::decode_bc7(data, w, h, &mut pixels),
        BlockFormat::Uncompressed { .. } => unreachable!("handled above"),
    };
    if result.is_err() {
        return None;
    }

    trace!(
        "decoded DDS {:?} {}x{} ({} bytes compressed)",
        header.format, w, h, expected
    );

    Some((argb_u32_to_rgba8(&pixels), header.width, header.height))
}

pub struct DdsFormat {}

impl TextureFormat for DdsFormat {
    fn load(&self, buffer: &[u8]) -> RawTextureData {
        let (bytes, width, height) = decode_rgba8(buffer).expect("Failed to decode DDS texture");
        RawTextureData {
            bytes,
            width,
            height,
            format: PixelFormat::RGBA,
        }
    }
}

pub const DDS: DdsFormat = DdsFormat {};

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal DX10/BC7 DDS around a single block of `payload`.
    fn dx10_bc7(width: u32, height: u32, payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        let mut header = [0u8; HEADER_LEN];
        header[0..4].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
        header[8..12].copy_from_slice(&height.to_le_bytes());
        header[12..16].copy_from_slice(&width.to_le_bytes());
        header[76..80].copy_from_slice(&PF_FLAG_FOURCC.to_le_bytes()); // pf flags
        header[80..84].copy_from_slice(b"DX10");
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&98u32.to_le_bytes()); // dxgiFormat = BC7_UNORM
        buf.extend_from_slice(&[0u8; 16]); // rest of DXT10 header
        buf.extend_from_slice(payload);
        buf
    }

    #[test]
    fn parses_dx10_bc7_header() {
        let dds = dx10_bc7(4, 4, &[0u8; 16]);
        let h = parse_header(&dds).expect("should parse");
        assert_eq!((h.width, h.height), (4, 4));
        assert_eq!(h.format, BlockFormat::Bc7);
        assert_eq!(h.data_offset, 148);
    }

    #[test]
    fn decodes_a_single_bc7_block_to_rgba8() {
        let dds = dx10_bc7(4, 4, &[0u8; 16]);
        let (bytes, w, h) = decode_rgba8(&dds).expect("should decode");
        assert_eq!((w, h), (4, 4));
        assert_eq!(bytes.len(), 4 * 4 * 4);
    }

    #[test]
    fn rejects_non_dds_and_truncated_input() {
        assert!(decode_rgba8(b"NOPE").is_none());
        assert!(decode_rgba8(&[]).is_none());
        // Header claims 4x4 BC7 (16 bytes of data) but supplies only 8.
        let truncated = dx10_bc7(4, 4, &[0u8; 8]);
        assert!(decode_rgba8(&truncated).is_none());
    }

    #[test]
    fn decodes_uncompressed_bgra32() {
        // DDPF_RGB | DDPF_ALPHAPIXELS, 32bpp, standard B8G8R8A8 masks.
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        let mut header = [0u8; HEADER_LEN];
        header[0..4].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
        header[8..12].copy_from_slice(&1u32.to_le_bytes()); // height
        header[12..16].copy_from_slice(&1u32.to_le_bytes()); // width
        header[76..80].copy_from_slice(&(0x40u32 | 0x1).to_le_bytes());
        header[84..88].copy_from_slice(&32u32.to_le_bytes()); // rgbBitCount
        header[88..92].copy_from_slice(&0x00FF0000u32.to_le_bytes()); // R
        header[92..96].copy_from_slice(&0x0000FF00u32.to_le_bytes()); // G
        header[96..100].copy_from_slice(&0x000000FFu32.to_le_bytes()); // B
        header[100..104].copy_from_slice(&0xFF000000u32.to_le_bytes()); // A
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&[0x33, 0x22, 0x11, 0x80]); // BGRA bytes
        let (bytes, w, h) = decode_rgba8(&buf).expect("should decode");
        assert_eq!((w, h), (1, 1));
        assert_eq!(bytes, vec![0x11, 0x22, 0x33, 0x80]);
    }

    #[test]
    fn rejects_absurd_dimensions_instead_of_overflowing() {
        // A malformed header must not reach the surface-size multiply.
        let mut dds = dx10_bc7(4, 4, &[0u8; 16]);
        dds[16..20].copy_from_slice(&u32::MAX.to_le_bytes()); // width
        assert!(decode_rgba8(&dds).is_none());
        let zero = dx10_bc7(0, 4, &[0u8; 16]);
        assert!(decode_rgba8(&zero).is_none());
    }

    #[test]
    fn argb_repacks_channels_in_rgba_order() {
        assert_eq!(
            argb_u32_to_rgba8(&[0x80_11_22_33]),
            vec![0x11, 0x22, 0x33, 0x80]
        );
    }
}
