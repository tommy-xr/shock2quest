# Website

Static landing page, no build step. It uses ES modules and the captures in
`screenshots/hero/`, so serve the repo root over HTTP and open `/website/`:

```sh
python3 -m http.server 4180   # http://127.0.0.1:4180/website/
```

- `index.html` — the page.
- `shared/controls.js` — copy, links, control-scheme data, latest-version lookup (GitHub API).
- `shared/controller3d.js` — Meta Quest Touch Plus pair, rendered with three.js from the
  [WebXR Input Profiles](https://github.com/immersive-web/webxr-input-profiles) models (MIT).
  A control mode's buttons glow and animate through their press range.
- Media comes from `screenshots/hero/` (regenerate with `tools/shock2-sdk/scripts/hero-shots.mjs`).
  Control modes with `clip: null` show "footage pending" until a clip is recorded.

The MFD control clip has a deterministic production-input capture:

```sh
cd tools/shock2-sdk
npm ci && npm run build
DARK_ASSET_PATH=/path/to/25AE node scripts/hero-mfd.mjs
```

It stages a small inventory in Med/Sci, taps the raw Menu action, then grips,
points and releases a wrench into a different cell. The script asserts the
placement ghost and final item identity, writes `mfd.mp4` / `.png` / `.gif`, and
records its asset root, runtime hash, revision, inputs and checks in `mfd.json`.
This is first-person VR harness footage, not a headset tracking recording.
