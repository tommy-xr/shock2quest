# Particle System Colour Handling Parity

This note documents how the original Dark Engine handles particle colours and the steps required to mirror that behaviour in our renderer. It focuses on the palette-driven colour ramps used for effects like blood splatter and the ENG1 mission emitters.

## Status & hand-off (2026-06-19)

**Single-colour rendering is DONE (PR #308).** A particle now draws in its
authored palette colour (no more grey blobs). What shipped, mapped to the
porting strategy below:
- Step 1 (palette): **done** — `shock2vr/src/palette.rs` loads the SS2 **game
  master palette** `res/pal/SHOCKPAL.PCX` once and exposes `index_to_rgb`.
  ⚠️ Note: this is the *game master* palette, **not** the per-mission `RENDPARAMS`
  palette the strategy below assumes — for SS2 particles the master palette is the
  correct source. If a mission ever needs its own palette, revisit.
- Step 6 (shader) + 7 (cache): **done for a single colour** — `BillboardMaterial`
  has a `color`, the `inColor` uniform is fetched/uploaded, the hardcoded grey is
  gone, and the colour is resolved once per `ParticleSystem` (`with_color`). The
  emissive term is tinted by `inColor` too (so it glows in-colour, not white).

**Remaining (ranked) — the lifetime fade is what the rest of this doc covers:**
1. **Lifetime `cr → cg → cb` colour ramp (steps 3–4, 6).** This is the main
   unfinished piece. Needs per-particle `life_remaining/life_total` (step 3) and
   the multi-colour shader uniforms (step 6). The single-colour shader is in place;
   extend it to the ramp.
2. **`render_type`** — pixel/square vs. disk, then `PRT_SCALED_BITMAP` via
   `model_name`.
3. **Motion fidelity** — `motion_type` (esp. immobile / no-gravity), `is_worldspace`,
   `spin`.

**Gotchas that cost real time (read before starting the fade):**
- **Authored params live on the placed `.mis` *instance*, not the leaf gamesys
  template.** Spawning a particle template directly yields a *zeroed* runtime-state
  `PropParticleGroup` (`a:0`, denormal `size`) → renders blank. The `debug_particles`
  scene (`shock2vr/src/scenes/debug_particles.rs`) sets params directly to sidestep
  this; real placed emitters parse fine.
- **`cg == 0` / `cb == 0` is the "no 2nd/3rd colour" sentinel** (index 0 is the
  magenta colour key, not a real entry). The `colour_stage` sketch below already
  keys off this — don't fade toward palette entry 0.
- **Tint the *emissive* term by the ramp colour**, like the base colour, or it
  washes to white in dark scenes.
- **Verify with the fixed debug-runtime control (PR #307):** `cargo dbgr --mission
  debug_particles`, then a plain `/v1/step` + `/v1/screenshot` is deterministic (no
  `Content-Type` header, no sleeps). Time-based systems (particles) **only advance
  via `/v1/step`** — the runtime is paused otherwise.

---


## Reference Behaviour Summary

- `ParticleGroup` stores palette indices, not raw channels:
  - `cr`, `cg`, `cb` pick indices in the active 256-colour palette (`grd_pal`).
  - `ca` is the maximum opacity (0–255).
- At render time the engine builds translucency lookup tables for every palette index it needs (`get_tluc_color` in `pgroup.c`). Each table contains eight alpha steps (approx. 12.5% increments), so translucent draws become simple table lookups.
- Lifetime is tracked per particle. Remaining lifetime (`tm`) is compared with the total launch time to select which palette colour to use:
  - With three colours: early → `cr`, middle third → `cg`, final third → `cb`.
  - With two colours: first half → `cr`, second half → `cg`.
- Opacity stays at `ca / 255` until the remaining lifetime drops below `fade_time`, then it decays linearly to zero.

## Porting Strategy

1. **Expose the mission palette**
   - The `RENDPARAMS` chunk names the palette file. Load the 256×RGB table and keep it accessible so any system can convert palette indices to linear RGB:  
     `rgb = palette[index] / 255.0`.

2. **Clarify property naming**
   - Treat the `PropParticleGroup` fields as palette indices (`color0`, `color1`, `color2`) plus `max_alpha`. This helps distinguish them from literal colour components.

3. **Track per-particle lifespans**
   - When launching particles, store both `life_total` and `life_remaining`. This allows a direct translation of the reference lifetime checks regardless of fixed-point units.

4. **Replicate the colour ramp**
   ```rust
   fn colour_stage(pg: &PropParticleGroup, particle: &Particle) -> Stage {
       let ratio = particle.life_remaining / particle.life_total;
       if pg.color1 != 0 && pg.color2 != 0 {
           if ratio <= 1.0 / 3.0 { Stage::Late }
           else if ratio <= 2.0 / 3.0 { Stage::Mid }
           else { Stage::Early }
       } else if pg.color1 != 0 {
           if ratio <= 0.5 { Stage::Mid } else { Stage::Early }
       } else {
           Stage::Early
       }
   }
   ```
   - Convert the chosen palette index to RGB using the table from step 1.

5. **Mirror the fade curve**
   ```rust
   fn particle_alpha(pg: &PropParticleGroup, particle: &Particle) -> f32 {
       let peak = pg.max_alpha as f32 / 255.0;
       if pg.fade_time <= 0.0 { return peak; }
       let fade_ratio = (particle.life_remaining / pg.fade_time).min(1.0);
       peak * fade_ratio
   }
   ```

6. **Update the shader**
   - Replace the current fragment shader code that hardcodes `vec4(0.5)` with uniforms for the palette-derived colours and alpha. A direct GLSL sketch:
     ```glsl
     uniform vec3 uColorEarly;
     uniform vec3 uColorMid;
     uniform vec3 uColorLate;
     uniform float uHasMid;
     uniform float uHasLate;
     uniform float uLifeRatio; // remaining / total
     uniform float uAlpha;

     vec3 ramp = uColorEarly;
     if (uHasLate > 0.5) {
         if (uLifeRatio <= 1.0/3.0) ramp = uColorLate;
         else if (uLifeRatio <= 2.0/3.0) ramp = uColorMid;
     } else if (uHasMid > 0.5) {
         if (uLifeRatio <= 0.5) ramp = uColorMid;
     }

     vec4 tex = texture(texture1, texCoord);
     if (tex.a < 0.1) discard;
     fragColor.rgb = tex.rgb * ramp;
     fragColor.a   = tex.a * uAlpha;
     ```
   - Material instances already exist per particle, so the CPU can set these uniforms without additional instancing work. If batching becomes necessary later, promote these to per-instance buffers or precompute the final `vec4`.

7. **Cache palette colours per system**
   - Resolve `color0/1/2` once per `ParticleSystem` rather than per particle update. Combine with the shader changes so only the life ratio varies per particle.

8. **Validate with mission content**
   - Compare blood splatter and ENG1 emitters between the original engine and our port (screenshots or captures). The colour progression and fade timing should now align.

Implementing these steps will unblock the shader work and allow the remaining particle fixes (spawn shapes, motion modes, attachment handling) to reuse the same palette infrastructure.

