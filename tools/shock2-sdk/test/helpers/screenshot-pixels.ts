import { readFileSync } from "node:fs";
import { inflateSync } from "node:zlib";

// Minimal PNG reader for assertions about what a capture actually contains -
// "is this frame black?" is otherwise only answerable by a human looking at it,
// which is how a VR capture that rendered nothing went unnoticed.
//
// Only the shape the debug runtime writes is supported (8-bit RGB/RGBA,
// non-interlaced); anything else throws rather than guessing.

interface DecodedPng {
  width: number;
  height: number;
  /** Row-major RGB triples, alpha dropped. */
  pixels: Uint8Array;
}

function decodePng(path: string): DecodedPng {
  const file = readFileSync(path);
  const signature = [137, 80, 78, 71, 13, 10, 26, 10];
  for (const [i, byte] of signature.entries()) {
    if (file[i] !== byte) throw new Error(`${path} is not a PNG`);
  }

  let width = 0;
  let height = 0;
  let channels = 0;
  const idat: Buffer[] = [];
  for (let offset = 8; offset + 8 <= file.length; ) {
    const length = file.readUInt32BE(offset);
    const type = file.toString("ascii", offset + 4, offset + 8);
    const body = file.subarray(offset + 8, offset + 8 + length);
    if (type === "IHDR") {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      const bitDepth = body[8];
      const colorType = body[9];
      const interlace = body[12];
      if (bitDepth !== 8 || interlace !== 0 || (colorType !== 2 && colorType !== 6)) {
        throw new Error(
          `${path}: unsupported PNG (depth ${bitDepth}, color type ${colorType}, interlace ${interlace})`,
        );
      }
      channels = colorType === 6 ? 4 : 3;
    } else if (type === "IDAT") {
      idat.push(Buffer.from(body));
    } else if (type === "IEND") {
      break;
    }
    offset += 12 + length; // length + type + body + CRC
  }

  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * channels;
  const pixels = new Uint8Array(width * height * 3);
  const previous = new Uint8Array(stride);
  const current = new Uint8Array(stride);
  for (let y = 0; y < height; y++) {
    const rowStart = y * (stride + 1);
    const filter = raw[rowStart];
    for (let x = 0; x < stride; x++) {
      const value = raw[rowStart + 1 + x];
      const left = x >= channels ? current[x - channels] : 0;
      const up = previous[x];
      const upLeft = x >= channels ? previous[x - channels] : 0;
      // PNG per-row filters (RFC 2083 section 6).
      let reconstructed: number;
      switch (filter) {
        case 0:
          reconstructed = value;
          break;
        case 1:
          reconstructed = value + left;
          break;
        case 2:
          reconstructed = value + up;
          break;
        case 3:
          reconstructed = value + ((left + up) >> 1);
          break;
        case 4: {
          const p = left + up - upLeft;
          const dLeft = Math.abs(p - left);
          const dUp = Math.abs(p - up);
          const dUpLeft = Math.abs(p - upLeft);
          const predictor =
            dLeft <= dUp && dLeft <= dUpLeft ? left : dUp <= dUpLeft ? up : upLeft;
          reconstructed = value + predictor;
          break;
        }
        default:
          throw new Error(`${path}: unknown PNG row filter ${filter}`);
      }
      current[x] = reconstructed & 0xff;
    }
    for (let x = 0; x < width; x++) {
      const from = x * channels;
      const to = (y * width + x) * 3;
      pixels[to] = current[from];
      pixels[to + 1] = current[from + 1];
      pixels[to + 2] = current[from + 2];
    }
    previous.set(current);
  }

  return { width, height, pixels };
}

/**
 * Fraction of pixels brighter than `threshold` (0-255) on any channel - i.e.
 * how much of the frame is not black. On a scene whose only content is the
 * thing under test, this is a direct "did it actually draw?" assertion.
 */
export function litFraction(path: string, threshold = 12): number {
  const { width, height, pixels } = decodePng(path);
  let lit = 0;
  for (let i = 0; i < pixels.length; i += 3) {
    if (pixels[i] > threshold || pixels[i + 1] > threshold || pixels[i + 2] > threshold) {
      lit++;
    }
  }
  return lit / (width * height);
}
