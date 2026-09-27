# Website

Static landing page, no build step. ES modules, so serve over HTTP:

```sh
python3 -m http.server -d website 4180
```

- `index.html` — the page.
- `shared/controls.js` — copy, links, control-scheme data, latest-version lookup (GitHub API).
- `shared/controller3d.js` — Meta Quest Touch Plus pair, rendered with three.js from the
  [WebXR Input Profiles](https://github.com/immersive-web/webxr-input-profiles) models (MIT).
  A control mode's buttons glow and animate through their press range.
- `media/` — captures from `tools/shock2-sdk/scripts/hero-shots.mjs`. Control modes with
  `clip: null` show "footage pending" until a clip is recorded.
