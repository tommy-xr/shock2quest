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
/// DDSCAPS2_CUBEMAP, in the legacy header's `caps2`.
const CAPS2_CUBEMAP: u32 = 0x200;
/// DDSCAPS2_CUBEMAP plus all six DDSCAPS2_CUBEMAP_POSITIVEX.. face bits.
const CAPS2_FULL_CUBEMAP: u32 = CAPS2_CUBEMAP | 0xFC00;
/// DDS_RESOURCE_MISC_TEXTURECUBE, in the DX10 header's `miscFlag`.
const DX10_MISC_TEXTURECUBE: u32 = 0x4;

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
    /// Mip levels stored per surface; at least 1.
    mip_count: u32,
    /// Six faces follow one another, each with its own mip chain.
    cube: bool,
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
    let mip_count = u32_at(buf, 28)?.max(1);
    let legacy_cube = u32_at(buf, 112)? & CAPS2_FULL_CUBEMAP == CAPS2_FULL_CUBEMAP;

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
            mip_count,
            cube: legacy_cube,
        });
    }

    let (format, data_offset, cube) = match four_cc {
        b"DXT1" => (BlockFormat::Bc1, 128, legacy_cube),
        b"DXT3" => (BlockFormat::Bc2, 128, legacy_cube),
        b"DXT5" => (BlockFormat::Bc3, 128, legacy_cube),
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
            let cube = u32_at(buf, 136)? & DX10_MISC_TEXTURECUBE != 0;
            (format, 148, cube)
        }
        _ => return None,
    };

    Some(DdsHeader {
        width,
        height,
        format,
        data_offset,
        mip_count,
        cube,
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
    let pixels = decode_surface(header.format, data, w, h)?;
    Some((pixels, header.width, header.height))
}

/// A cubemap's six mip-0 faces as RGBA8, in DDS order: +X, -X, +Y, -Y, +Z, -Z
/// (the same order GL numbers `TEXTURE_CUBE_MAP_POSITIVE_X + i`).
pub struct CubeFaces {
    pub size: u32,
    pub faces: [Vec<u8>; 6],
}

/// Decode a DDS cubemap. `None` if the buffer is not a square cubemap we
/// understand, or is truncated. Each face keeps only mip 0, capped like any
/// other DDS; the renderer builds its own mips.
pub fn decode_cube_rgba8(buffer: &[u8]) -> Option<CubeFaces> {
    let header = parse_header(buffer)?;
    if !header.cube || header.width != header.height {
        return None;
    }
    let size = header.width as usize;
    // A mip chain ends at 1x1; a larger count is a malformed header.
    let mip_count = header
        .mip_count
        .min(u32::BITS - header.width.leading_zeros());
    let face_stride: usize = (0..mip_count)
        .map(|mip| {
            let edge = (size >> mip).max(1);
            header.format.surface_bytes(edge, edge)
        })
        .sum();
    let top = header.format.surface_bytes(size, size);
    let mut faces: [Vec<u8>; 6] = Default::default();
    let mut edge = header.width;
    for (index, face) in faces.iter_mut().enumerate() {
        let start = header.data_offset + index * face_stride;
        let pixels = decode_surface(header.format, buffer.get(start..start + top)?, size, size)?;
        (*face, edge, _) = cap_edge(pixels, header.width, header.height);
    }
    Some(CubeFaces { size: edge, faces })
}

/// Decode one `w` x `h` surface of `format` to RGBA8.
fn decode_surface(format: BlockFormat, data: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    // Uncompressed surfaces are repacked directly; no block decode involved.
    if let BlockFormat::Uncompressed {
        bits_per_pixel,
        r_mask,
        g_mask,
        b_mask,
        a_mask,
    } = format
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
        return Some(out);
    }

    let mut pixels = vec![0u32; w * h];
    let result = match format {
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
        format,
        w,
        h,
        data.len()
    );

    Some(argb_u32_to_rgba8(&pixels))
}

/// Largest edge a decoded DDS keeps, per platform.
///
/// A decoded DDS is uncompressed RGBA8 (the renderer uploads nothing else), so
/// the 25AE texture set costs roughly 12x the original's memory at full
/// resolution - about 1.5 GB if it were all resident, against 681 MB capped at
/// 256 px. Desktop can afford the full size; Quest cannot, and 256 px is already
/// the modal size in the upgraded set, so the cap costs little visually.
/// See `projects/25th-anniversary-assets.md` for the measurements.
#[cfg(target_os = "android")]
const MAX_EDGE: Option<u32> = Some(256);
#[cfg(not(target_os = "android"))]
const MAX_EDGE: Option<u32> = None;

/// Halve an RGBA8 image `(w, h)` repeatedly until both fit `max_edge`,
/// box-filtering each step.
///
/// Successive halving rather than a single resample: it is a few lines, needs no
/// filter kernel, and each step averages exactly 4 source texels.
pub fn downscale_to_fit(
    mut data: Vec<u8>,
    mut w: u32,
    mut h: u32,
    max_edge: u32,
) -> (Vec<u8>, u32, u32) {
    while w.max(h) > max_edge && w > 1 && h > 1 {
        let (nw, nh) = (w / 2, h / 2);
        let mut out = Vec::with_capacity((nw * nh * 4) as usize);
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let at = |sx: u32, sy: u32| data[(((sy * w) + sx) * 4 + c) as usize] as u32;
                    let (sx, sy) = (x * 2, y * 2);
                    let sum = at(sx, sy) + at(sx + 1, sy) + at(sx, sy + 1) + at(sx + 1, sy + 1);
                    out.push((sum / 4) as u8);
                }
            }
        }
        data = out;
        w = nw;
        h = nh;
    }
    (data, w, h)
}

/// Apply the platform's `MAX_EDGE` to a decoded image.
fn cap_edge(bytes: Vec<u8>, width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    match MAX_EDGE {
        Some(max) => downscale_to_fit(bytes, width, height, max),
        None => (bytes, width, height),
    }
}

pub struct DdsFormat {}

impl TextureFormat for DdsFormat {
    fn load(&self, buffer: &[u8]) -> RawTextureData {
        let (bytes, width, height) = decode_rgba8(buffer).expect("Failed to decode DDS texture");
        let (bytes, width, height) = cap_edge(bytes, width, height);
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

    /// A 2x2 BGRA32 cubemap with two mips per face. Face `i` is filled with
    /// red `i * 10` at mip 0 and red 255 at mip 1, so reading the wrong
    /// face or into a mip chain shows up as a wrong colour.
    fn uncompressed_cube() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        let mut header = [0u8; HEADER_LEN];
        header[0..4].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
        header[8..12].copy_from_slice(&2u32.to_le_bytes()); // height
        header[12..16].copy_from_slice(&2u32.to_le_bytes()); // width
        header[24..28].copy_from_slice(&2u32.to_le_bytes()); // mip count
        header[76..80].copy_from_slice(&(0x40u32 | 0x1).to_le_bytes());
        header[84..88].copy_from_slice(&32u32.to_le_bytes());
        header[88..92].copy_from_slice(&0x00FF0000u32.to_le_bytes());
        header[92..96].copy_from_slice(&0x0000FF00u32.to_le_bytes());
        header[96..100].copy_from_slice(&0x000000FFu32.to_le_bytes());
        header[100..104].copy_from_slice(&0xFF000000u32.to_le_bytes());
        header[108..112].copy_from_slice(&(CAPS2_CUBEMAP | 0xFC00).to_le_bytes());
        buf.extend_from_slice(&header);
        for face in 0..6u8 {
            for _ in 0..4 {
                buf.extend_from_slice(&[0, 0, face * 10, 255]); // BGRA
            }
            buf.extend_from_slice(&[0, 0, 255, 255]); // 1x1 mip
        }
        buf
    }

    #[test]
    fn decodes_each_cube_face_past_the_previous_faces_mips() {
        let cube = decode_cube_rgba8(&uncompressed_cube()).expect("should decode");
        assert_eq!(cube.size, 2);
        for (index, face) in cube.faces.iter().enumerate() {
            assert_eq!(face.len(), 2 * 2 * 4);
            assert_eq!(&face[0..4], &[index as u8 * 10, 0, 0, 255], "face {index}");
        }
    }

    /// A header's mip count is file data: an absurd one must neither panic
    /// nor loop, and cannot stretch past the 1x1 level.
    #[test]
    fn an_absurd_mip_count_stops_at_the_chain_end() {
        let mut dds = uncompressed_cube();
        dds[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        let cube = decode_cube_rgba8(&dds).expect("should decode");
        assert_eq!(&cube.faces[5][0..4], &[50, 0, 0, 255]);
    }

    #[test]
    fn a_cubemap_missing_faces_is_rejected() {
        let mut dds = uncompressed_cube();
        dds[112..116].copy_from_slice(&(CAPS2_CUBEMAP | 0x400).to_le_bytes());
        assert!(decode_cube_rgba8(&dds).is_none());
    }

    #[test]
    fn a_plain_texture_is_not_a_cubemap() {
        assert!(decode_cube_rgba8(&dx10_bc7(4, 4, &[0u8; 16])).is_none());
    }

    #[test]
    fn reads_the_dx10_cube_flag() {
        let mut dds = dx10_bc7(4, 4, &[0u8; 16 * 6]);
        dds[136..140].copy_from_slice(&DX10_MISC_TEXTURECUBE.to_le_bytes());
        assert!(parse_header(&dds).expect("should parse").cube);
        assert_eq!(
            decode_cube_rgba8(&dds).expect("should decode").faces.len(),
            6
        );
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
    fn downscale_halves_until_it_fits_and_averages_texels() {
        // 4x4, every texel of the top-left 2x2 block set to 100/200/40/80 so the
        // averaged result is predictable.
        let mut data = vec![0u8; 4 * 4 * 4];
        for (y, v) in [(0u32, 100u8), (1, 200)] {
            for x in 0..2u32 {
                let at = (((y * 4) + x) * 4) as usize;
                data[at] = v;
            }
        }
        let (out, w, h) = downscale_to_fit(data, 4, 4, 2);
        assert_eq!((w, h), (2, 2));
        assert_eq!(out.len(), 2 * 2 * 4);
        // Top-left output texel averages 100, 100, 200, 200.
        assert_eq!(out[0], 150);
    }

    #[test]
    fn downscale_is_a_no_op_when_already_small_enough() {
        let data = vec![7u8; 2 * 2 * 4];
        let (out, w, h) = downscale_to_fit(data.clone(), 2, 2, 256);
        assert_eq!((w, h, out), (2, 2, data));
    }

    #[test]
    fn argb_repacks_channels_in_rgba_order() {
        assert_eq!(
            argb_u32_to_rgba8(&[0x80_11_22_33]),
            vec![0x11, 0x22, 0x33, 0x80]
        );
    }
}
