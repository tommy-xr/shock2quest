// Pcx — 8-bit palettized PCX images (SS2's world textures), decoded in
// Functor to RGB8 pixels for `Texture.fromPixels`, row 0 first.
//
//   let image = File.bytes("ss2/fam/medsci_1/wall.pcx") |> Pcx.decode |> Binary.require

type image = { width: float, height: float, pixels: Bytes.t }

type header = { width: float, height: float, stride: float }

let header =
  Decode.map5((magic, bits, box, planes, stride) => (magic, bits, box, planes, stride),
    Decode.u8, Decode.skipThen(2.0, Decode.u8), Decode.list(4.0, Decode.u16le),
    Decode.skipThen(53.0, Decode.u8), Decode.u16le)
    |> Decode.andThen((h) => match h with
      | (magic, bits, box, planes, stride) =>
        if magic != 10.0 || bits != 8.0 || planes != 1.0
        then Decode.fail("not an 8-bit, one-plane PCX")
        else Decode.succeed({ width: Affine.n(2.0, box) - Affine.n(0.0, box) + 1.0,
                              height: Affine.n(3.0, box) - Affine.n(1.0, box) + 1.0, stride: stride }))

// The run-length bytes after the 128-byte header, as palette indices: a byte
// ≥ 0xC0 repeats the next one (its low 6 bits) times. Each RGB triple is a
// 3-byte slice of the palette, so a pixel costs one lookup, not three.
let decode = (data: Bytes.t): Result.t<image, Binary.error> =>
  Decode.run(header, data) |> Result.bind((h: header) =>
    let end = Bytes.length(data) - 769.0 in
    if Bytes.u8(end, data) != Result.Ok(12.0) then Result.Error(Decode.failure(end, "no 256-colour palette"))
    else
      let colours = List.range(256.0) |> List.map((i) => Bytes.slice(end + 1.0 + 3.0 * i, 3.0, data) |> Binary.require) in
      let colour = (i: float) => Option.defaultValue(Bytes.zeros(3.0), List.nth(i, colours)) in
      List.range(end - 128.0)
        |> List.tryMapAccum((run, i) =>
             Bytes.u8(128.0 + i, data) |> Result.map((b) =>
               if run > 0.0 then (0.0, List.range(run) |> List.map((k) => colour(b)))
               else if b >= 192.0 then (b - 192.0, [])
               else (0.0, [colour(b)])),
           0.0)
        |> Result.bind((acc) => match acc with
          | (_, runs) =>
            let indexed = List.flatten(runs) in
            if h.stride < h.width || List.length(indexed) < h.stride * h.height
            then Result.Error(Decode.failure(128.0, "PCX pixel data is shorter than its rows"))
            else
              // Rows are `stride` bytes; keep `width` of each.
              let rows = if h.stride == h.width then List.take(h.width * h.height, indexed)
                         else List.range(h.height) |> List.concatMap((y) => List.take(h.width, List.drop(y * h.stride, indexed))) in
              Result.Ok({ width: h.width, height: h.height, pixels: Bytes.concat(rows) })))
