// Requires Node 22+, Python 3 + Pillow (lossless WebP), and a built SDK.
// npm run build, then node scripts/astra-grip-gallery.mjs [--templates=-52,-2949]
// A diagnostic capture tool: successful acquisition never implies visual approval.
import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs, promisify } from 'node:util';
import { execFile } from 'node:child_process';
import { GameServer } from '../dist/src/index.js';
import { aimVrHandAt, quatConjugate, quatRotate } from '../dist/test/helpers/vr-hand.js';

const { values } = parseArgs({ options: {
  output: { type: 'string', default: '/tmp/astra-grip-gallery' },
  templates: { type: 'string' },
  reviews: { type: 'string' },
  'render-only': { type: 'boolean', default: false },
} });
const repoRoot = fileURLToPath(new URL('../../../', import.meta.url));
const output = resolve(values.output);
const manifestPath = join(output, 'data.json');
const viewNames = ['front', 'back', 'top', 'oblique', 'palm'];
const weaponTemplates = new Set([-928, -17, -19, -26, -27, -247]);
const credentialTemplates = new Set([-2998, -2594]);
const reviewStatuses = new Set(['reviewed', 'adjust', 'uncertain']);
const reviews = values.reviews ? JSON.parse(await readFile(resolve(values.reviews), 'utf8')) : {};
for (const [template, review] of Object.entries(reviews)) {
  assert.ok(reviewStatuses.has(review.status) && typeof review.note === 'string', `Invalid review for ${template}`);
}

// The rack is the canonical catalog. Preserve every template, including implants
// sharing geometry; explicit --templates can narrow an iteration without duplication.
const sceneSource = await readFile(join(repoRoot, 'shock2vr/src/scenes/debug_interactions.rs'), 'utf8');
const constants = Object.fromEntries([...sceneSource.matchAll(/const (\w+): i32 = (-?\d+);/g)]
  .map((m) => [m[1], Number(m[2])]));
const catalog = [...sceneSource.matchAll(/InteractionFixture\s*\{\s*label:\s*"([^"]+)",\s*template_id:\s*(-?\d+|[A-Z_]+),\s*model:\s*"([^"]+)"/g)]
  .map((m) => ({ label: m[1], template: Number.isNaN(Number(m[2])) ? constants[m[2]] : Number(m[2]), model: m[3] }));
assert.ok(catalog.length > 0 && catalog.every((f) => Number.isInteger(f.template)), 'Unable to read the canonical fixture catalog');
const requested = values.templates ? new Set(values.templates.split(',').map(Number)) : null;
if (requested) for (const template of requested) assert.ok(catalog.some((f) => f.template === template), `Unknown fixture ${template}`);
const fixtures = catalog.filter((f) => requested ? requested.has(f.template) : !weaponTemplates.has(f.template));
// Produce the newly requested samples first so partial galleries are useful
// while a full capture is still running. Variants remain distinct entries.
fixtures.sort((a, b) => Number(/implant|worm|gamepig|ice/i.test(b.label)) - Number(/implant|worm|gamepig|ice/i.test(a.label)));

function reviewFor(fixture) {
  const supplied = reviews[String(fixture.template)];
  if (supplied) {
    assert.ok(reviewStatuses.has(supplied.status) && typeof supplied.note === 'string', `Invalid review for ${fixture.label}`);
    return supplied;
  }
  if (fixture.label.toLowerCase().includes('hypo') || fixture.model === 'techt') {
    return { status: 'adjust', note: 'Existing review requests a better grasp. Inspect palm, thumb, and finger contact in the enlarged views.' };
  }
  if (fixture.model === 'scipass') return { status: 'adjust', note: 'Lower placement requested; pending visual review. Real credential cards are shown at their station, not held.' };
  if (fixture.model === 'magci') return { status: 'adjust', note: 'Magazine tilt remains under review; side-center placement alone is not approval.' };
  return { status: 'uncertain', note: 'Not yet visually reviewed in this gallery. Prepared lookup is diagnostic data, not a quality verdict.' };
}

const escapeHtml = (value) => String(value).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

async function renderGripGallery(manifest) {
  const originals = manifest.items.flatMap((item) => Object.values(item.hands)
    .flatMap((capture) => Object.values(capture.views).map((shot) => shot.file)));
  // Preserve native PNG evidence; lossless WebP makes the self-contained HTML
  // small enough to download. Cache by mtime so progressive renders stay cheap.
  await promisify(execFile)('python3', ['-c', `
from pathlib import Path
from PIL import Image
import sys
root=Path(sys.argv[1])
for name in sys.argv[2:]:
    source=root/name
    target=source.with_suffix('.webp')
    if not target.exists() or target.stat().st_mtime < source.stat().st_mtime:
        with Image.open(source) as image:
            image.save(target,format='WEBP',lossless=True,method=4)
`, output, ...originals]);
  const embedded = await Promise.all(manifest.items.map(async (item) => {
    const images = {};
    for (const [hand, capture] of Object.entries(item.hands)) {
      images[hand] = {};
      for (const [view, shot] of Object.entries(capture.views)) {
        const portableFile = shot.file.replace(/\.png$/, '.webp');
        images[hand][view] = { ...shot, image: `data:image/webp;base64,${(await readFile(join(output, portableFile))).toString('base64')}` };
      }
    }
    const unavailable = item.error || Object.entries(item.hands).some(([hand, capture]) => hand !== 'station' && capture.diagnostic.grip?.source !== 'prepared');
    const review = unavailable
      ? { status: 'uncertain', note: item.error ? 'Capture failed; inspect the error before evaluating this item.' : 'Prepared fit unavailable; these images show fallback behavior, not an approved fit.' }
      : reviews[String(item.template)] ?? item.review;
    return { ...item, review, images };
  }));
  const payload = JSON.stringify(embedded).replaceAll('<', '\\u003c');
  const html = `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Astra VR grip inspection</title><style>
*{box-sizing:border-box}[hidden]{display:none!important}body{margin:0;background:#14181e;color:#edf1f5;font:16px system-ui,sans-serif}header{padding:18px 24px;border-bottom:1px solid #3d4856}h1{font-size:24px;margin:0 0 8px}p{margin:8px 0;line-height:1.5;color:#bdc8d4}.layout{display:grid;grid-template-columns:260px minmax(0,1fr)}aside{padding:16px;border-right:1px solid #3d4856;max-height:calc(100vh - 130px);overflow:auto;position:sticky;top:0}main{padding:20px;min-width:0}button,select,input{font:inherit;color:inherit;background:#273240;border:1px solid #536477;border-radius:6px;padding:8px;cursor:pointer}button.active{background:#34657c;border-color:#8fd1ed}nav button{display:block;width:100%;text-align:left;margin:8px 0}.toolbar{display:flex;gap:8px;flex-wrap:wrap;margin:12px 0}figure{margin:0;background:#07090c;border:1px solid #536477}#shot{display:block;width:100%;height:clamp(300px,calc(100vh - 390px),800px);object-fit:contain}figcaption{padding:10px;color:#bdc8d4}h2{margin:0;font-size:24px}.status{display:inline-block;padding:4px 8px;border-radius:4px;background:#455466}.adjust{background:#80431d}.reviewed{background:#245b48}.uncertain{background:#455466}pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:13px}details{margin:16px 0}a{color:#a0d9ed}#search{width:100%;cursor:text}.small{font-size:13px}#note{max-width:1000px}#error{color:#ffc6ab}.empty{padding:50px} @media(max-width:800px){.layout{grid-template-columns:1fr}aside{position:static;max-height:230px;border-right:0;border-bottom:1px solid #3d4856}main{padding:12px}header{padding:14px}#shot{height:auto;max-height:none}}
</style><header><h1>Astra VR grip inspection</h1><p>Diagnostic gallery — not headset approval. Inspect both hands and multiple angles before accepting a fit.</p><p class="small">${escapeHtml(manifest.created)} · ${embedded.length} item variants · normal prepared lookup · original glove calibration</p></header>
<div class="layout"><aside><label for="search">Find an item</label><input id="search" type="search" placeholder="Name or model"><nav id="items" aria-label="Items"></nav></aside><main><h2 id="name"></h2><p id="identity" class="small"></p><span id="status" class="status"></span><p id="note"></p><p id="error"></p><div class="toolbar" id="hands" aria-label="Hand"></div><div class="toolbar" id="views" aria-label="Camera view"></div><figure><img id="shot" alt=""><figcaption id="caption"></figcaption></figure><div class="toolbar"><button id="previous">Previous item</button><button id="next">Next item</button><a id="download" download>Download full-resolution image</a></div><details><summary>Capture and grip diagnostics</summary><pre id="diagnostics"></pre></details><details><summary>How to interpret review status</summary><p>Uncertain means these views still need inspection. Adjust records a known placement or contact concern. Reviewed means an explicit review note was supplied; it does not imply headset comfort or approval.</p><p>Palm close-up prioritizes hand contact and may crop the rest of a large item. Front/back/top/oblique include the full hand/object bounds. The same model may appear in several variants intentionally.</p></details></main></div>
<script>const items=${payload};let selected=0,hand='left',view='oblique';const el=id=>document.getElementById(id);const labels={front:'Front',back:'Back',top:'Top',oblique:'Oblique',palm:'Palm close-up',station:'Station (credential)'};
function buttons(id,values,current,select){const root=el(id);root.replaceChildren();for(const v of values){const b=document.createElement('button');b.textContent=labels[v]||v;b.className=v===current?'active':'';b.setAttribute('aria-pressed',String(v===current));b.onclick=()=>{select(v);render()};root.append(b)}}
function navigation(){const q=el('search').value.toLowerCase();const root=el('items');root.replaceChildren();items.forEach((item,i)=>{if(![item.label,item.model,String(item.template)].some(s=>s.toLowerCase().includes(q)))return;const b=document.createElement('button');b.textContent=item.label+' · '+item.review.status;b.className=i===selected?'active':'';b.onclick=()=>{selected=i;render()};root.append(b)})}
function render(){if(!items.length){el('name').textContent='No captures yet';return}const item=items[selected];const hands=Object.keys(item.images);if(!hands.includes(hand))hand=hands[0];const shots=item.images[hand]||{};const views=Object.keys(shots);if(!views.includes(view))view=views.includes('oblique')?'oblique':views[0];const shot=shots[view];el('name').textContent=item.label;el('identity').textContent=item.model+' · template '+item.template+(item.sharedModel?' · shared geometry: '+item.sharedModel:'');el('status').textContent=item.review.status;el('status').className='status '+item.review.status;el('note').textContent=item.review.note;el('error').textContent=item.error||'';buttons('hands',hands,hand,v=>hand=v);buttons('views',views,view,v=>view=v);el('shot').hidden=!shot;el('download').hidden=!shot;if(shot){el('shot').src=shot.image;el('shot').alt=item.label+', '+hand+', '+(labels[view]||view);el('caption').textContent=(labels[view]||view)+' · '+shot.framing;el('download').href=shot.image;el('download').download=item.model+'-'+hand+'-'+view+'.webp'}else {el('shot').removeAttribute('src');el('download').removeAttribute('href');el('caption').textContent='No successful image; inspect error and diagnostics.';}el('diagnostics').textContent=JSON.stringify({capture:item.hands[hand]?.diagnostic,camera:shot?{position:shot.position,lookAt:shot.lookAt}:null},null,2);el('previous').disabled=selected===0;el('next').disabled=selected===items.length-1;navigation()}
el('search').oninput=navigation;el('previous').onclick=()=>{selected=Math.max(0,selected-1);render()};el('next').onclick=()=>{selected=Math.min(items.length-1,selected+1);render()};render();</script></html>`;
  await writeFile(join(output, 'index.html'), html);
}

await mkdir(output, { recursive: true });
if (values['render-only']) {
  await renderGripGallery(JSON.parse(await readFile(manifestPath, 'utf8')));
} else {
  const manifest = { created: new Date().toISOString(), mission: 'debug_interactions', items: [] };
  const game = await GameServer.launch({ mission: 'debug_interactions', debugFlags: ['--vr'], repoRoot });
  try {
    for (const fixture of fixtures) {
      const aliases = catalog.filter((f) => f.model === fixture.model && f.template !== fixture.template).map((f) => f.label);
      const record = { ...fixture, sharedModel: aliases.join(', '), review: reviewFor(fixture), hands: {} };
      manifest.items.push(record);
      try {
        for (const hand of credentialTemplates.has(fixture.template) ? ['station'] : ['left', 'right']) {
          await game.input.set('left_hand.squeeze', 0);
          await game.input.set('right_hand.squeeze', 0);
          await game.camera.attach();
          await game.input.trigger('DebugReloadLevel');
          await game.step({ frames: 90 });
          const item = (await game.entities.list({ limit: 100 })).entities.find((e) => e.template_id === fixture.template);
          assert.ok(item, `Missing ${fixture.label}`);
          const capture = { views: {}, diagnostic: {} };
          record.hands[hand] = capture;
          if (hand === 'station') {
            const position = [item.position[0], item.position[1] + 0.8, 0];
            await game.input.set('left_hand.position', [0, -100, 0]);
            await game.input.set('right_hand.position', [0, -100, 0]);
            await game.camera.set({ position, lookAt: item.position });
            await game.step({ frames: 1 });
            const file = `${Math.abs(fixture.template)}-station.png`;
            await game.screenshot(join(output, file), 1600);
            capture.views.station = { file, position, lookAt: item.position, framing: 'Credential collectible; not a held grip sample.' };
            continue;
          }
          const looseDetail = await game.entities.detail(item.id);
          await game.player.teleport({ x: item.position[0], y: 1, z: 0 });
          await aimVrHandAt(game, item.position, 0.2, 0, 0, { hand });
          await game.input.set(`${hand}_hand.squeeze`, 1);
          await game.step({ frames: 3 });
          assert.equal((await game.info()).player[hand === 'left' ? 'wielded_entity_id' : 'right_hand_entity_id'], item.id, `${hand} acquisition failed`);
          await game.player.teleport({ x: 4, y: 1.25, z: 5 });
          await game.input.set(`${hand}_hand.position`, [0, 1, 0]);
          await game.input.set(`${hand}_hand.rotation`, [0, Math.SQRT1_2, 0, Math.SQRT1_2]);
          await game.input.set(`${hand === 'left' ? 'right' : 'left'}_hand.position`, [0, -100, 0]);
          await game.step({ frames: 3 });
          const info = await game.info();
          const grip = info.player.hand_grips?.find((g) => g.hand === hand);
          const pawn = info.player.position;
          const palm = [pawn[0] - 0.09, pawn[1] + 1, pawn[2]];
          const detail = await game.entities.detail(item.id);
          let bounds = detail.selection_bounds;
          // Holding removes the pickup collider. Reuse the loose collider's
          // eight corners, mapped through its old/new entity transforms.
          if (!bounds && looseDetail.selection_bounds) {
            const corners = [];
            for (const x of [0, 1]) for (const y of [0, 1]) for (const z of [0, 1]) {
              const point = [looseDetail.selection_bounds[x][0], looseDetail.selection_bounds[y][1], looseDetail.selection_bounds[z][2]];
              const local = quatRotate(quatConjugate(looseDetail.rotation), point.map((v, i) => v - looseDetail.position[i]));
              corners.push(quatRotate(detail.rotation, local).map((v, i) => v + detail.position[i]));
            }
            bounds = [0, 1].map((side) => [0, 1, 2].map((axis) => (side ? Math.max : Math.min)(...corners.map((v) => v[axis]))));
          }
          // Union the live object collider with the calibrated glove envelope.
          // A separate close-up prevents large models making the hand tiny.
          const min = palm.map((v, i) => Math.min(v - 0.14, bounds?.[0]?.[i] ?? v));
          const max = palm.map((v, i) => Math.max(v + 0.14, bounds?.[1]?.[i] ?? v));
          const center = min.map((v, i) => (v + max[i]) / 2);
          const radius = Math.hypot(...max.map((v, i) => (v - min[i]) / 2));
          const distance = Math.max(0.42, radius * 2.4);
          capture.diagnostic = { entity: item.id, model: fixture.model, grip, selection_bounds: bounds, pawn, handPosition: [0, 1, 0] };
          if (!grip?.grip || grip.source !== 'prepared') {
            record.review = { status: 'uncertain', note: `Prepared fit unavailable for ${hand}; captured fallback for diagnosis.` };
          }
          const directions = { front: [0, 0, 1], back: [0, 0, -1], top: [0, 1, 0.01], oblique: [0.8, 0.5, -0.8], palm: [0.5, 0.5, hand === 'right' ? 1 : -1] };
          for (const view of viewNames) {
            const close = view === 'palm';
            const lookAt = close ? palm : center;
            const direction = directions[view];
            const length = Math.hypot(...direction);
            const position = lookAt.map((v, i) => v + direction[i] / length * (close ? 0.48 : distance));
            await game.camera.set({ position, lookAt });
            await game.step({ frames: 1 });
            const file = `${Math.abs(fixture.template)}-${hand}-${view}.png`;
            await game.screenshot(join(output, file), 1600);
            capture.views[view] = { file, position, lookAt, framing: close ? 'Calibrated hand-contact close-up; the rest of a large object can extend outside the frame.' : bounds ? 'Full hand/object bounds.' : 'Calibrated framing; no object bounds available.' };
          }
          console.log(`${fixture.label} ${hand}: ${grip?.source ?? 'missing'}; ${grip?.solve_ms?.toFixed(3) ?? '?'}ms`);
          await writeFile(manifestPath, JSON.stringify(manifest, null, 2));
        }
      } catch (error) {
        record.error = String(error);
        record.review = { status: 'uncertain', note: 'Capture failed; inspect the error before evaluating this item.' };
        process.exitCode = 1;
        console.error(`${fixture.label}: ${error}`);
      }
      await writeFile(manifestPath, JSON.stringify(manifest, null, 2));
      await renderGripGallery(manifest);
      console.log(`Gallery updated: ${join(output, 'index.html')}`);
    }
  } finally {
    await game.shutdown();
    await writeFile(join(output, 'runtime.log'), game.logs().join('\n'));
  }
}
console.log(join(output, 'index.html'));
