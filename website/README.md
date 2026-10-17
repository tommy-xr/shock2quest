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
  A control mode's buttons glow and animate through their press range - or, when its clip
  has a `<clip>.inputs.json` (recorded with the SDK's `recordClipInputs`), replay the
  clip's real inputs in sync with the video (`shared/inputs.js`).
- Tests: `node --test website/test/*.test.js`.
- `shared/feedback.js` — review overlay, loaded only with `?feedback=1`
  (`/website/?feedback=1`). Press **A** (or Annotate), then drag a box or click an
  element, and write a note; add overall notes, then **Export JSON**. Work in progress
  is kept in `localStorage` per page. Hand the file to an agent with the
  `website-feedback` skill (`.claude/skills/website-feedback`). Export shape
  (`shared/feedback-core.js`):

  ```json
  { "format": "shock2quest-website-feedback/1", "url": "…/website/?feedback=1",
    "created": "2026-10-17T12:34:56.789Z", "overall": "…",
    "annotations": [{ "shape": "box", "rect": { "x": 59, "y": 264, "w": 650, "h": 136 },
      "viewport": { "w": 1280, "h": 800 }, "devicePixelRatio": 2,
      "selector": "body > header.hero > div > h1", "tag": "h1",
      "text": "System Shock 2.In your hands.", "note": "…" }] }
  ```

  `rect` is in document CSS pixels; `selector`/`tag`/`text` name the element that best
  matches the box (for a click, the element clicked; `text` is its first 80 characters).
- Media comes from `screenshots/hero/` (regenerate with `tools/shock2-sdk/scripts/hero-shots.mjs`).
  Control modes with `clip: null` show "footage pending" until a clip is recorded.
  The MFD clip is re-recorded, with its inputs, by `tools/shock2-sdk/scripts/hero-mfd.mjs`;
  the Rickenbacker ladder clip (`rick1`) by `tools/shock2-sdk/scripts/hero-ladder-draw.mjs`.
- `shared/equipment.js` / `equipment.css` — equipment tour, backed by runtime captures
  in `screenshots/equipment/`. Regenerate with
  `tools/shock2-sdk/scripts/equipment-shots.mjs`; its `callouts.json` contains normalized
  camera-projected tracking points. The tour loads on approach, pauses offscreen,
  and starts paused for reduced-motion preferences. Item selection and stills also
  work when playback is paused or video is unavailable.
