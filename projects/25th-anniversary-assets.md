# 25th Anniversary Edition assets — compatibility spike

**Status:** spike. Parser/decoder fixes implemented and verified against a running
game; asset *mounting* (reading a stock 25AE install directly) is still outstanding.
**Asset source analysed:** `/Users/bryan/ss2-25th` (SystemShock2Remastered, KEX engine build, Jun 2025 binaries).
**Question:** can we switch to the 25th Anniversary Edition (25AE) assets, ideally supporting both?

## TL;DR

The base game data is **byte-for-byte the original**, so the 25AE install is a drop-in
replacement for `Data/` — every mission loads and renders today with zero code changes.

The *remaster* part is not new base data; it is a **layered mod stack of five KPF archives**
in ordinary Dark Engine formats. Consuming it needs three things, in increasing cost:
extension-agnostic texture lookup, a DDS/BC7 decoder, and LGMD v6 model support.

There is **no normal-mapping and no PBR** anywhere in the 25AE assets — worth knowing before
budgeting renderer work.

## 1. Packaging

`*.kpf` files are **stored-mode (uncompressed) ZIPs** — so `ZipAssetPath` can mount them
directly, and random access is essentially free.

| Archive | Size | Files | Contents |
| --- | --- | --- | --- |
| `sshock2.kpf` | 434 MB | 10 760 | The original game data (`data/*.mis`, `shock2.gam`, `data/res/**`) |
| `mods/sshock2ee.kpf` | 352 MB | 4 557 | **The Nightdive remaster layer** — models, textures, motions, materials |
| `mods/shtup.kpf` | 182 MB | 2 913 | SHTUP (community texture upgrade) |
| `mods/scp.kpf` | 168 MB | 1 351 | SCP (community patch) — models, DMLs, `.mis` fixes |
| `mods/400.kpf` | 149 MB | 2 173 | Terrain-family (`fam/`) texture upgrades |
| `mods/patch_ext.kpf` | 2 MB | 79 | Small compatibility fixes |
| `sshock2ee.kpf` (root) | 327 MB | 398 | Frontend only — menu DDS art, TTF fonts, loading spinner, haptics |
| `sshock2ee-vault.kpf` | 1.16 GB | 2 253 | Bonus gallery (concept art, scans, videos). Irrelevant to us. |

Load order is explicit in `base.kpf:defaults/cam_mod.ini` — highest priority first:

```
uber_mod_path ./res+kpf:/ubermod
mod_path      kpf:/mods/sshock2ee + kpf:/mods/400 + kpf:/mods/shtup + kpf:/mods/scp + kpf:/mods/patch_ext
resname_base  ./data/res + kpf:/data/res
load_path     ./data + kpf:/data
```

This maps cleanly onto our existing `AssetPath::combine`, which is already first-mount-wins.

One layout difference: the classic install packs resources as `res/<family>.crf`; 25AE ships
them **loose** under `data/res/<family>/`. Also `motiondb.bin` moves from the data root to
`data/res/mschema/motiondb.bin`, and `shock2vr/src/lib.rs` opens it (and `shock2.gam`) with a
direct `File::open`, not through the asset paths.

## 2. Base data is identical

CRC32 comparison of 25AE `data/` against a pristine classic install:

- **All 23 `.mis` files, `shock2.gam`, and all four `.res` files: byte-identical.**
- `data/res/**` vs the classic `.crf` archives: **10 588 of 10 594 files identical.**
  - 2 differ: `obj/COMDOOR.BIN`, `obj/ELECOM.BIN` (both still LGMD v4, both parse fine).
  - 141 additive files (95 `iface` PNGs, 39 `objicon` PNGs, `mschema/motiondb.bin`).
  - `motiondb.bin` itself is identical to the classic one.

So the entity/gamesys/geometry layer needs **no work at all**.

## 3. What the mod stack actually contains

All of it is in formats we already parse — no new container or archive format:

| Kind | Count | Format | Our status |
| --- | --- | --- | --- |
| Static models | 1 692 `.bin` | LGMD **v4** (and 13 × **v6**) | v4 fine; v6 misparses |
| Skinned meshes | 67 `.bin` | LGMM v1/v2 | fine |
| Motions | 362 `.mc` | same format as vanilla, re-authored | fine |
| Textures | 3 262 `.dds` | **BC7_UNORM** (DX10 header) | **no decoder** |
| Textures | ~1 200 `.png`/`.pcx`/`.gif` | standard | fine |
| Materials | 3 507 `.mtl` / `.inc` | KEX text material DSL | **unsupported** |
| Gamesys patches | 74 `.dml` | DML1 text patch scripts | **unsupported** |
| Game scripts | 61 `.nut` | Squirrel | not applicable (we reimplement scripts in Rust) |
| HUD text layouts | 109 `.itl` | KEX text layout | **unsupported** |

### The `.mtl` material system

`.mtl`/`.inc` are a small text DSL of render passes. The vocabulary is modest — roughly 25
directives, and 1 430 of 3 507 files are trivial (an `include` plus a `texture`). The
meaningful ones by frequency: `texture`, `shaded`, `render_material_only`, `ui_scale`,
`terrain_scale`, `blend`, `uv_clamp`, `uv_mod` (scale/scroll), `alpha`, `ani_frames`/`ani_rate`.

`terrain_scale` / `ui_scale` (748 / 825 uses) exist because the replacement textures are
higher-resolution than the originals — they carry the logical-vs-actual size relationship, so
they must be honoured or upgraded textures tile at the wrong scale.

### No normal maps, no PBR

Searched every layer: **zero** normal / bump / roughness / metalness / height maps. The only
extra map channels are:

- `illum_map` (16 uses) — emissive `_LUMA` maps; vanilla Dark already had this concept.
- `env_map` (7 uses) — one cube-map for frosted glass.

The ~30 `*_S.dds` / `*_spec` files in the Nightdive layer are **not** specular maps in the PBR
sense. They drive an additive Fresnel rim pass through a 1-D ramp lookup:

```
render_pass {
    blend SRC_ALPHA ONE
    texture $TEXTURE
    RGB 1.5 1.5 1.5
    alpha func INCIDENCE 1.0 1.0 MATERIALS/ND-IR_SPEC
    shaded 1
}
```

That is a fixed-function-era trick (view-incidence → ramp → additive). Cheap to replicate in a
shader and quite VR-friendly, but it is a stylistic rim-light, not physically based shading.
If we want real normal/specular mapping, we would be authoring it ourselves — the remaster
does not supply it.

## 3a. Bugs found and fixed

Working the mod stack up to "renders correctly" surfaced six defects. Three of them
are **pre-existing bugs against the original game data**, not 25AE-specific.

| # | Defect | Effect | Pre-existing? |
| --- | --- | --- | --- |
| 1 | `assert!(version > 3 && size_mat_extra >= 8)` in `read_extended_materials` | Hard crash on any LGMD v3 model (SCP's `SHOVEL.BIN`). The `if` on the next line already handled the case. | yes (latent) |
| 2 | `read_polygon` skipped its 1-byte tail only when `version == 4` | LGMD v6 polygon stream misaligned → out-of-range indices → `index out of bounds` crash. v6 uses the same 1-byte tail; `>= 4` is correct. | 25AE-only |
| 3 | `VhotType::from_u32(..).unwrap()` | Panic on vhot ids outside 0–8 (SCP's `escpod.bin` uses 10/11). Nothing reads the type, so unknown ids now degrade to `Unknown`. | yes (latent) |
| 4 | `read_extended_materials` treated `size_mat_extra` as a whole-chunk size | It is the stride of **one** material's record. With the 16-byte stride used by 270 SHTUP and 192 SCP models, every material after the first read garbage transparency. | yes (latent; 1 vanilla model affected) |
| 5 | PCX decoder assumed 8-bit paletted | Panic on 24-bit PCX. Also fixed **7 textures in the original game data** that never decoded. | **yes, real** |
| 6 | No TGA importer | 5 mod textures plus 1 in the original data (`obj/txt16/ricklogo.tga`) failed to load. | **yes, real** |

### The transparency convention (the interesting one)

Even with all of the above fixed, props still rendered invisible. Bisecting the layers
showed the Nightdive layer alone reproduced it, and dumping `GURNEY.bin` explained why:

```
VANILLA GURNEY.BIN   mats=5  mat_flags=2   transparency = 0.00 (x5)
ND      GURNEY.bin   mats=2  mat_flags=3   transparency = 1.00 (x2)   <- fully invisible
```

Dark stores **transparency** (0.0 = opaque). Models re-exported by the mod layers store
**opacity** (1.0 = opaque). Under Dark's reading, every one of those props is 100%
transparent.

Only the *opaque* value was remapped by the re-export — fractional transparencies survive
unchanged. So the rule is a per-material clamp of `1.0 → 0.0`, **not** a whole-model
inversion. Checked against the vanilla counterpart of every re-exported model that carries
a 1.0:

| rule reproduces the vanilla values | models |
| --- | --- |
| both rules agree | 509 |
| **clamp only** | **20** |
| whole-model inversion only | **0** |
| neither (mod genuinely retuned the material) | 75 |

Inversion is never better and corrupts the 20 models that mix 1.0 with a real
transparency — `shutscrn.bin` is `[1.0, 0.9, 0.4]` against vanilla's `0.9`/`0.4`, which
inversion would turn into `0.1`/`0.6`. (This was caught by the cross-engine review; the
first implementation inverted.)

The convention is cleanly separable from Dark's, which is what makes the fix safe rather
than a guess:

| Layer | Models with an extended-material chunk | …containing an exact `1.0` |
| --- | --- | --- |
| original game data | 1434 | **0** |
| `patch_ext` | 12 | 0 |
| `shtup` | 416 | 14 |
| `scp` | 397 | 17 |
| `sshock2ee` (Nightdive) | 863 | **850** |

No model shipped with the original game stores 1.0, so the clamp is provably a no-op
there. The layers' own mixed cases confirm the reading — `PRISM_CAS.bin = [0.5, 1.0]` is
half-transparent glass in an opaque frame; `rrails82.bin = [1.0, 0.98]` is an opaque
railing with a near-opaque second material. The SHTUP/SCP models carrying a 1.0 are
chairs, ladders, ducts, cans and consoles — under Dark's convention every chair in the
game would be invisible. The 13 Nightdive models that are *not* re-exports (`PILLOW.bin`
at 0.0, `glass1-4.bin` at 0.98) are untouched and keep Dark semantics.

Covered by unit tests in `dark/src/ss2_bin_obj_loader.rs`; the stride test fails against
the old code, confirming it is a real regression test.

Two silent drop paths in `to_scene_objects` (missing texture, and empty vertex list) now
log a warning — both made props vanish with no diagnostic at all, which is what made this
take as long as it did.

## 4. Spike results — what actually loads

A new probe tool (`tools/asset_probe`, added by this spike) runs the real `dark` / `engine`
importers over an asset tree, catching panics per file. It also validates polygon
vertex/normal/UV indices against the arrays they point into, because a misaligned read
otherwise "succeeds" and only explodes later at render time.

Vanilla is the control. **Before** the fixes in §3a:

| Layer | Models | Textures |
| --- | --- | --- |
| **vanilla (control)** | 1 518 / 1 518 ok | 5 105 ok, 8 fail (7 non-paletted PCX + 1 TGA — pre-existing) |
| `sshock2ee` (Nightdive) | 916 ok / **13 fail** | 833 ok / **829 fail** (all `.dds`) |
| `shtup` | 416 / 416 ok | 122 ok / **1 791 fail** (all `.dds`) |
| `400` | — | 0 ok / **642 fail** (all `.dds`) |
| `scp` | 397 ok / **2 fail** | 646 ok, 5 fail (1 PCX + 4 TGA) |
| `patch_ext` | 12 / 12 ok | 29 ok, 2 fail (PCX) |

**After**, every layer is clean — including 8 textures in the original game data that
never loaded before:

| Layer | Models | Textures |
| --- | --- | --- |
| vanilla (control) | 1 518 / 1 518 ok | **5 113 / 5 113 ok** |
| `sshock2ee` (Nightdive) | **929 / 929 ok** | **1 662 / 1 662 ok** |
| `shtup` | 416 / 416 ok | **1 913 / 1 913 ok** |
| `400` | — | **642 / 642 ok** |
| `scp` | **399 / 399 ok** | **651 / 651 ok** |
| `patch_ext` | 12 / 12 ok | **31 / 31 ok** |

Reproduce with:

```bash
cargo build -p asset_probe --release
./target/release/asset_probe <extracted-layer-dir>
```

### End-to-end runs

Repacked 25AE `data/` into the classic layout and pointed `DARK_ASSET_PATH` at it:

1. **25AE base data only** — `medsci1.mis` boots and renders correctly, no panics.
   All 23 missions load (see §6).
2. **25AE + full mod stack** — **crashed on load**, twice, exactly where the probe predicted:
   - `dark/src/ss2_bin_obj_loader.rs:611` — `assert!(version > 3 && header.size_mat_extra >= 8)`
     on SCP's `obj/shovel.bin`, which is **LGMD v3**. v3 legitimately has no extended-material
     chunk, and the `if` on the very next line already handles its absence, so the assert is
     simply wrong. (Spike removed it; needs review + a regression test before landing.)
   - `dark/src/ss2_bin_obj_loader.rs:387` — `index out of bounds: len is 400 but index is 15872`,
     from a misparsed **LGMD v6** model.
3. **25AE + mod stack, 13 v6 models swapped back to vanilla** — **boots clean, 0 panics.**

So the only two hard blockers for the mod stack are **LGMD v6** and (for visual benefit) **DDS**.

### Why v6 breaks

`read_polygon` skips a trailing byte only when `version == 4`:

```rust
if version == 4 {
    let _unknown = read_u8(reader);
}
```

v6 has a different per-polygon tail, so every subsequent polygon is read misaligned. The 13
affected files are the Many-infested organic props: `EGGOP`, `EGGOP_F`, `FLOWER`, `NERVEST`,
`NERVEU`, `ORIFICE`, `PLANT1`, `PLANTT`, `PLANTW`, `PLANTWA`, `TALPLANT`, `WPODOPEN`,
`ND-shoscreen`. 11 have vanilla v4 equivalents to fall back to; `EGGOP_F` and `ND-shoscreen`
do not.

### The texture-resolution problem

This is the crux of the visual upgrade, and it is **not** simply "add a DDS decoder".

`Model::from_obj_bin` requests the **exact filename** stored in the model's material list:

```rust
let mut tex_path = material.name.to_string();
let maybe_texture = asset_cache.get_opt(&TEXTURE_IMPORTER, &tex_path);
if maybe_texture.is_none() { return None; }   // <- mesh slot silently dropped
```

The mod layers replace `FOO.PCX` with `FOO.DDS` — a *different filename* — so the override is
never found, and where a replacement model references a name that no longer exists as-is, the
mesh slot is silently dropped and **the prop vanishes**. That is exactly what the modded
screenshot showed (missing gurney and ceiling lamp).

Measured over the merged obj set, with the vanilla build as control:

| Build | Names referenced | Resolve exactly today | Resolve only if extension ignored | No such stem |
| --- | --- | --- | --- | --- |
| vanilla (control) | 1 144 | 1 092 | 20 (0 loadable) | 32 |
| 25AE + mod stack | 957 | 726 | **203 (119 already loadable)** | 28 |

The ~30 "no such stem" entries appear in the control too, so they are pre-existing noise, not
a 25AE regression.

**Reading:** adding extension-agnostic lookup recovers those names. This also mirrors what
KEX itself does — `foo.pcx` → `foo.mtl` → `material/shtup/foo` → `foo.dds`.

### Mount-first resolution

A first pass tried the **requested name first** and only fell back to another extension on
a miss. That was safe but left the upgrades shadowed: an upgraded encoding lost whenever
the original file was still present, so 404 of 945 material names kept their original
`.pcx` with an unused `.dds` sitting right there.

The naive fix — preferring `.dds` globally — is **not** safe. `ZipAssetPath` registers
every archive entry under its bare basename as well as its full path, and 200 of these
names have candidates in more than one directory, so a global preference silently pulls a
terrain-family `black.dds` in place of a model's `obj/txt16/BLACK.PCX`.

Two changes make it correct:

1. **Mount-first ordering.** `AbstractAssetPath` gained
   `resolve_first(base, &[candidate])`. The default (a leaf mount) is first-match;
   `MultipleAssetPaths` overrides it so **mounts are the outer loop** — the highest-priority
   archive that has *any* candidate wins, and only then does candidate order break the tie.
   Testing each candidate independently with `exists` would instead let a low-priority
   mount's early-ordered candidate beat a high-priority mount's later-ordered one.
2. **Namespace qualification.** Model and mesh textures live in their family archive's
   `txt16/` subdirectory, so candidates are tried `txt16/`-qualified before the bare
   basename. That keeps the search inside the right family and defuses the collision above.

Measured on `medsci1` (`RUST_LOG=dark::util=trace`):

| | 25AE + mod stack | original data (control) |
| --- | --- | --- |
| resolved to `.dds` | **398** | 0 |
| resolved to `.pcx` | 53 | 569 |
| resolved to `.gif` / `.png` | 16 | 13 |

So the upgrades now land, and the original data is untouched — it has no `.dds` to prefer,
resolves entirely to `.pcx`/`.gif`, drops zero mesh slots, and its frame-1 screenshot
differs from the pre-change baseline by 12 bytes out of 1 440 600 (~4 pixels of render
nondeterminism). Covered by unit tests in `engine/src/assets/asset_paths.rs`.

## 4a. DDS/BC7 and Quest

The renderer only ever uploads uncompressed RGBA8 — every texture today, PCX included, is
CPU-decoded at load. So BC7 needed **no renderer change at all**, just a decoder
(`engine/src/dds.rs`, backed by `texture2ddecoder`): BC1/BC2/BC3/BC7 plus uncompressed
surfaces, mip 0 only, since the engine builds its own mip chain.

**Decoding on the fly on Quest is viable; memory is the binding constraint, not CPU.**

- **Throughput**: all 1 791 SHTUP DDS decode in **2.95 s single-threaded** on an M-series
  desktop — 147 Mpx at ~50 Mpx/s. A Quest CPU core is several times slower, but textures
  load per-level rather than all at once and decoding parallelises trivially. This is not
  the problem.
- **Memory is.** Upgraded textures are mostly 256×256 where the originals were 64×64 or
  128×128, so as RGBA8 they cost roughly **12×** the vanilla set (64.5 MB → 772.7 MB for
  the stems present in both), plus ~808 MB of textures with no vanilla counterpart.

Full-residency footprint, and what a decode-time downscale cap would cost:

| | RGBA8 footprint |
| --- | --- |
| full resolution | 1 540 MB |
| cap 512 px | 1 302 MB |
| cap 256 px | 681 MB |
| cap 128 px | 186 MB |

(Upper bounds — nothing loads every texture at once — but the ratios are what matter.)

Practical options for Quest, in order of effort:

1. **Decode + downscale on Android.** A max-dimension cap in the DDS path (256 px keeps
   most of the visual gain, since that is already the modal size). Bounded memory, small
   change, no new upload path. This is the obvious first move.
2. **Offline transcode to ASTC** at packaging time, plus a compressed-texture upload path
   in `engine` (which does not exist today). Best quality per byte and Quest-native, but
   it is real renderer work.
3. **Native BC7 on desktop** (`GL_ARB_texture_compression_bptc`) to save desktop memory.
   Doesn't help Quest, and desktop isn't under pressure — low priority.

## 4b. Two very different upgrade mechanisms: `obj/` vs `mesh/`

Weapons and world objects upgrade in a way we already consume. Creatures and first-person
arms do not — and the difference is structural, not cosmetic.

### `obj/*.bin` — upgraded in place, and already working

The Nightdive layer replaces every weapon model as a plain LGMD, upgraded in the file
itself. First-person viewmodels (`_h`) gain the most, and pick up extra sub-objects
(articulated slides/magazines, which is what reload animation needs):

| model | vanilla polys | ND polys | factor | ND sub-objects |
| --- | --- | --- | --- | --- |
| `atek_h` (pistol) | 79 | 1 739 | **22×** | 4 |
| `empgun_h` | 59 | 1 359 | **23×** | 1 |
| `sg_h` (shotgun) | 60 | 1 213 | **20×** | 2 |
| `ar15_h` (assault rifle) | 126 | 1 695 | **14×** | 4 |
| `gren_h` (grenade launcher) | 47 | 531 | 11× | 1 |
| `lasehand` | 185 | 1 869 | 10× | 2 |
| `amp_h` (psi amp) | 114 | 1 173 | 10× | 2 |
| world models (`_w`) | — | — | 3–7× | 1 |

`pipewrench_h` / `pipewrench_w` are **new** — vanilla has no such model.

**These load today**, through the existing LGMD path plus the mount-first resolver.

### `mesh/*.bin` — a second mesh appended in an unparsed format

Every one of ND's 66 `mesh/` files is **two meshes concatenated**: first a copy of the
vanilla LGMM (byte-identical geometry, same triangle count, same vanilla material names
like `RUMBLER.gif`), then a **second mesh tagged `PMNM`** whose materials are the upgraded
`ND-*.psd` names:

```
mesh/player.bin   file=265 214   vanilla LGMM ends ~15 364   then:
  "LGMM" v1 ... "PMNM" ... materials: ND-player.psd, ND-teeth.psd
```

| mesh | vanilla tris | vanilla KB | ND KB | size | PMNM verts / indices |
| --- | --- | --- | --- | --- | --- |
| `grunt_p` (pipe hybrid) | 297 | 14 | 585 | **41×** | 1 704 / 7 464 |
| `grunt_g` (shotgun hybrid) | 291 | 14 | 431 | 30× | 2 779 / 10 836 |
| `cruf_*` / `ghost_f` (crew) | 270 | 13 | 304 | 23× | 1 627 / 7 674 |
| `crum_*` (crew) | 266 | 12 | 250 | 19× | 1 576 / 7 404 |
| `rumbler` | 290 | 13 | 153 | 11× | 1 242 / 5 634 |
| `player` (first-person arms) | 288 | 14 | 258 | 17× | 1 544 / 6 906 |
| `psword_h` (psi sword + arm) | 98 | — | — | — | 3 141 |

Reading the second field as an index count (÷3) puts the upgraded creature meshes at
roughly **6–12× the triangles**. That interpretation is not yet confirmed — the `PMNM`
field layout has only been partially mapped, and the marker's offset relative to the `LGMM`
magic varies between files.

**Consequences, which are worth being blunt about:**

- We read the *first* chunk and ignore the tail, so **every ND creature and the
  first-person arms currently render at vanilla quality**. A rumbler/midwife/crew
  before-after would show **no difference at all** — the upgrade is entirely in the
  unparsed chunk.
- The upgraded `mesh/` textures are unreachable for the same reason. `ND-rumbler.dds` is
  only ever named by the PMNM chunk's material (`ND-rumbler.psd`); the vanilla chunk still
  says `RUMBLER.gif`, and there is no `RUMBLER.mtl` redirect. So no amount of extension
  fallback finds them — the material name itself has to come from the new chunk.
- Same for VR arms: `ND-melee_arm.dds` (699 KB), `ND-player.dds` and its `_s` shine map all
  hang off PMNM material names.

**Parsing the `PMNM` chunk is therefore the single highest-value follow-up** — it unlocks
the creature upgrade, the first-person/VR arm upgrade, and the entire `mesh/` texture layer
in one go.

### The `PMNM` format, as reverse-engineered

Mapped empirically and validated against **all 66 chunks**. Every stride below holds
66/66 unless noted. All offsets are relative to the `PMNM` marker.

**Header (60 bytes)**

| offset | type | meaning |
| --- | --- | --- |
| +0 | `char[4]` | `"PMNM"` |
| +4 | `u32` | 0 in every file |
| +8 | `u32` | `num_materials` |
| +12 | `u32` | `num_joints` |
| +16 | `u32` | `num_vertices` |
| +20 | `u32` | `num_indices` (always divisible by 3) |
| +24 | `u32` | `morph_vertex_count` (`C`) — 0 in 35 of 66 |
| +28 | `u32` | `morph_target_count` (`D`) — 0 when `C` is 0 |
| +32 | `u32[7]` | section offsets |

**Sections**

| section | start | stride | contents |
| --- | --- | --- | --- |
| materials | `offs[0]` (always 60) | **56** | 16-byte name (`ND-rumbler.psd`), then floats, then what looks like the material's index start/count (`5634` for single-material `rumbler`, matching `num_indices`) |
| joints | `offs[1]` | **12** | `3 × f32` — a joint pivot **in model space** |
| vertices | `offs[2]` | **40** | see below |
| morph targets | `offs[3]` | `16 × D` | only when `D > 0` |
| morph deltas | `offs[4]` | `32 × D × C` | only when `C > 0` |
| indices | `offs[5]` | **2** | `u16` triangle list, `num_indices` entries |
| morph vertex list | `offs[6]` | **2** | `u16 × C` |

**Vertex (40 bytes)** — a modern interleaved skinned vertex:

| offset | type | meaning | validation |
| --- | --- | --- | --- |
| +0 | `3 × f32` | position (model space) | joint pivots span the same range |
| +12 | `2 × f32` | UV | lands in 0..1 |
| +20 | `3 × f32` | normal | length is exactly 1.0000 |
| +32 | `4 × u8` | bone indices | always `< num_joints` |
| +36 | `4 × u8` | bone weights | **always sum to exactly 255** |

The weights-sum-to-255 and unit-normal invariants hold for every vertex of every chunk
except a single degenerate vertex in `protodmg.bin` (1 of 1 676, zero normal) — a data
artifact, not a layout error. Every index is `< num_vertices` in all 66.

Summed over all 66 chunks: **132 320 PMNM triangles against 18 289 vanilla — 7.2×.**

This maps almost directly onto what the renderer already has:
`VertexPositionTextureSkinnedNormal` is already position + UV + normal + `bone_indices[4]`
+ `bone_weights[4]`.

**The one genuine blocker left is skeleton binding.** The vanilla LGMM stores vertices in
**joint-local** space with its own joint count (24 for `rumbler`); PMNM stores them in
**model** space against its own **20** pivots, and the joint records carry only a position —
no parent index, no name. So how those 20 pivots correspond to the `.cal` skeleton the
motion system animates is still unknown; likely nearest-pivot matching or an implied
ordering. Static rendering (or a fixed-pose arm/weapon) needs nothing further; **animated
creatures need that mapping solved.**

### Static-pose path — implemented, opt-in

`dark/src/ss2_bin_pmnm.rs` parses the chunk and `ss2_bin_ai_loader::pmnm_to_scene_objects`
renders it unskinned, in its authored rest pose. Enable with `SS2_PMNM_MESHES=1` (an env var
rather than an `--experimental` flag because model loading lives in `dark`, which has no
access to `shock2vr`'s options — the same reason `SS2_DEBUG_NORMALS` works this way).

Deliberately conservative:

- **Opt-in.** Default behaviour is byte-for-byte what it was.
- **Hitboxes still come from the original mesh**, so damage locations and ragdoll fitting are
  untouched. The swap is purely what gets drawn.
- **`read` returns `None` on any structural mismatch**, so an unexpected chunk degrades to
  "no high-detail mesh" rather than to garbage geometry, and every index is range-checked
  before it reaches the renderer.

One bug worth recording, because it is the kind that looks like a parser failure and is not:
the first render came out **rotated 90°**. PMNM stores vectors in Dark's axis convention
(`(x, z, y)`, x negated), the same as every other Dark reader — `vec3_at` now applies the
same conversion as `ss2_common::read_vec3`, and the mesh lines up with its own skeleton.

Verified: all 66 chunks parse through the Rust parser (asset_probe reports them, and a
present-but-unparseable chunk is a probe failure); `debug_hitbox` renders the pipe hybrid's
2 488-triangle chunk with `ND-ogp.psd` resolving to `txt16/nd-ogp.dds`; and the mesh is
upright, correctly scaled, and aligned with the hitbox skeleton.

The visible trade-off today is exactly the expected one — high-detail geometry and the
upgraded texture, but a T-pose instead of the animated pose:

| `SS2_PMNM_MESHES` unset | `SS2_PMNM_MESHES=1` |
| --- | --- |
| original mesh, correctly posed, muddy texture | 8.6x the triangles + upgraded DDS, rest pose |

So this is a **proof step, not a shippable creature path** — an unskinned creature is worse
in play than a posed low-poly one. It de-risks the parser and the texture plumbing so the
remaining work is purely the joint mapping.

### What this means for the flat vs VR weapon strategy

Today the two paths diverge by necessity:

- **Flat** uses `PropPlayerGun.hand_model` (the `_h` viewmodel) or `PropLimbModel` for
  melee, camera-anchored at authored offsets.
- **VR** mostly shows the **world** model (`PropModelName`), because
  `internal_switch_held_model` only swaps in an `_h` viewmodel when it appears in
  `vr_config::is_allowed_hand_model` — a hand-tuned allowlist. The vanilla `_h` meshes were
  too crude and too screen-space-authored to hold in a tracked hand.

The 25AE assets weaken that reason: at 1 200–1 700 polys with articulated sub-objects, the
`_h` models are now genuinely good enough to hold, and they share a single orientation
(corrected by one base yaw) rather than the inconsistently-keyed table
`flat_player_controller` complains about. So a unified "`_h` everywhere, anchored to the
camera in flat and to the hand in VR" path becomes plausible.

Two real caveats before committing to that:

1. Some viewmodels **include an arm** — `psword_h`'s materials are `ND-melee_arm.psd` +
   `ND-psword.psd`. In VR that duplicates the player's own hand/glove, so arm-inclusive
   viewmodels need either arm-part suppression or a separate VR variant.
2. The `_h` models are framed for a single one-handed screen-space pose; two-handed VR grips
   still need their own handling.

Worth noting the asset quality is no longer the blocker — the blocker is that the melee/arm
half of this lives in the unparsed `PMNM` chunk (caveat 1 is invisible to us today).

## 5. What we would need to change

Ordered by value-per-unit-effort. Each step is independently shippable and verifiable.

**Done in this spike** (all verified against a running game):

| Change | Unlocks |
| --- | --- |
| Parser/decoder fixes 1–6 in §3a + the transparency convention | Mod stack loads and renders |
| Mount-first, `txt16/`-qualified texture resolution (`AbstractAssetPath::resolve_first` + `dark::util::resolve_texture_name`) | Upgraded encodings actually win; stops props vanishing |
| DDS decoder (BC1/2/3/7 + uncompressed) in `engine::dds` | The ~3 262 upgraded textures — the actual visual upgrade |

**Still outstanding:**

| # | Change | Unlocks | Effort |
| --- | --- | --- | --- |
| 1 | Mount `.kpf` archives + accept the 25AE layout (loose `data/res/**`, `motiondb.bin` under `res/mschema/`, `shock2.gam`/`motiondb.bin` via asset paths not `File::open`) | Point straight at an unmodified 25AE install; **both** installs supported. Removes the repack scaffolding this spike used. | S — `ZipAssetPath` already handles stored ZIPs, and mount-first resolution is now in place |
| 2 | **Solve PMNM skeleton binding** — map the chunk's joint pivots onto the `.cal` skeleton so the high-detail meshes animate (the parser and static path already landed, see §4b) | Turns the PMNM path from a proof into the shippable creature + VR-arm upgrade | M — the format is mapped; this is the one remaining unknown |
| 3 | Android max-dimension cap in the DDS decode path | Bounded texture memory on Quest (see §4a) | S |
| 4 | Minimal `.mtl` subset: `texture`, `terrain_scale`/`ui_scale`, `uv_clamp`, `uv_mod`, `ani_frames`/`ani_rate`, `blend` | Correct scale/tiling/animation for upgraded textures | M |
| 5 | Optional: `illum_map`, incidence rim pass | The Nightdive "shine" look | M |
| 6 | Optional: offline ASTC transcode + compressed upload path | Best quality/byte on Quest | L |
| 7 | Not recommended: `.dml`, `.itl`, `.nut` | SCP gamesys patches / KEX HUD / KEX scripts | L — and largely duplicates logic we implement in Rust |

### Nightdive string tables are localization stubs — do not let them override

Capturing the weapon visuals surfaced this: with the full mod stack the psi-amp HUD renders
the raw key `$PSI6` where the original data reads `PROJECTED CRYOKINESIS`.

ND's `strings/psihelp.str` is 2 668 bytes against vanilla's 7 741, and every entry is an
indirection token:

```
Psi6:"$Psi6"
Psi7:"$Psi7"
```

KEX resolves `$Psi6` through `localization/loc_english.txt` in `base.kpf` (428 KB, and it
does contain the real text). We have no localization layer, so we render the token.

This is systemic, not a one-off — and it separates cleanly by layer:

| layer | `.str` files | entries | files >80% `$`-token stubs |
| --- | --- | --- | --- |
| original game data | 80 | 7 091 | **0** |
| `sshock2ee` (Nightdive) | 42 | 3 636 | **41** |
| `scp` | 37 | 3 225 | 0 |

So: **the Nightdive layer's `strings/` must not override the original tables** until the KEX
localization file is supported. SCP's string tables are real text and are fine to take. The
staged build used for the screenshots does let them override, which is why `$PSI6` appears
in the psi-amp capture.

### Two engine-side gaps the weapon captures exposed

Neither is an asset problem, and neither is a 25AE regression — both were verified against
the original data too:

- **The flat viewmodel does not animate.** Firing decrements ammo and produces hit spangs,
  and `Reload` is a real action, but the viewmodel itself does not move on either — no
  recoil, no slide or magazine travel. So the extra sub-objects ND ships on `atek_h` and
  `ar15_h` (4 each) are loaded but have nothing driving them yet.
- **The four exotic viewmodels are oversized and clip the frame** (stasis generator, fusion
  cannon, worm launcher, viral proliferator). Equally broken on the original data, so this is
  a pre-existing flat-viewmodel framing bug in this port.

Known gaps carried forward (raised by the cross-engine review, deferred deliberately):

- **Terrain families still hardcode PCX.** `dark/src/mission/scene_builder.rs` and
  `dark/src/mission/mod.rs`'s `texture_dimensions` build `"{family}/{name}.PCX"` directly
  and call `read_pcx_dimensions`, so `400.kpf`'s fam DDS upgrades are unreachable and
  would fall back to 1x1 dimensions. Route these through the same resolver as part of
  step 1.
- **Animated texture frames** (`load_multiple_textures`) still search only the base
  texture's original extension, so a base resolved to DDS will not find `foo_1.dds`.
- **Uncompressed DDS ignores `dwPitchOrLinearSize`**, assuming tightly-packed rows. All 8
  uncompressed surfaces the 25AE ships are 32bpp where this is equivalent; a padded 24bpp
  surface would shear.

Notes on the tail end:

- **`.dml` (74 files)** are text patches against gamesys/mission properties. Some encode real
  bug fixes we may have already fixed natively in Rust; applying them wholesale risks
  double-fixing. Worth mining for *content* rather than implementing as a system.
- **`.nut` (61 files)** are KEX Squirrel scripts. We reimplement object scripts in Rust, so
  these are reference material, not something to run.
- **BC7 on Quest** is the open risk in step 4. Quest GPUs want ASTC/ETC2; BC7 is generally not
  supported, so mobile likely needs an offline transcode step. Desktop is fine.

### Supporting both installs

Cheap, because the difference is confined to asset mounting: detect an install by sentinel
(`sshock2.kpf` ⇒ 25AE, `res/obj.crf` ⇒ classic) and build the appropriate `AssetPath` chain.
`paths::data_root()` already has the `DARK_ASSET_PATH` hook, and `AssetPath::combine` already
does first-mount-wins layering, which is exactly KEX's `mod_path` semantics.

A reasonable stopping point is steps 1–3: run directly off a stock 25AE install, get the
model/motion upgrades and the PNG texture upgrades, and defer DDS until we decide about Quest.

## 6. Verification performed

- CRC32 manifest diff, 25AE vs pristine classic `.crf` archives (10 594 files).
- `tools/asset_probe` over all six layers, with vanilla as control and polygon-index
  validation. **After the fixes: 100% of models and 100% of textures load in every layer.**
- Unit tests in `dark` (extended-material stride, transparency convention) and `engine`
  (DDS header parsing, BC7 decode, uncompressed BGRA, channel order). The stride test
  fails against the old code, so it is a genuine regression test.
- Headless boot of `medsci1.mis` on 25AE base data — renders correctly.
- Headless boot on the **full mod stack** — renders correctly, with the upgraded models
  and textures visibly in place (remodelled gurney, higher-detail surgical lamp, high-res
  crate). Frame-1 A/B against the base build confirms the base is pixel-unchanged.
- **All 23 missions load on the full mod stack** — 23 PASS / 0 FAIL, and **zero** dropped
  mesh slots logged across all of them.
- **All 23 missions loaded on 25AE base data** via the debug runtime — 23 PASS / 0 FAIL, no
  panics. (This incidentally confirms `shodan.mis` loads; the stale "known issue" note in
  `AGENTS.md` referred to [#267](https://github.com/tommy-xr/shock2quest/issues/267), closed
  2026-06-12. Verified it also loads on classic data, so this is not a 25AE difference —
  `AGENTS.md` updated.)

### Reproducing the staging directories

The spike built these under a scratch dir (not committed):

```bash
# 1. extract layers
for m in sshock2ee 400 shtup scp patch_ext; do unzip -qq mods/$m.kpf -d layer-$m; done
unzip -qq sshock2.kpf 'data/*' -d layer-vanilla

# 2. repack 25AE data/ into the classic layout our engine expects today
#    (res/<family>.crf zips + root .mis/.gam + motiondb.bin at root)
# 3. point the runtime at it
DARK_ASSET_PATH=<staged-dir> cargo dbgr --mission medsci1.mis --port 8137
```

Step 2 is scaffolding that step 1 of §5 makes unnecessary.
