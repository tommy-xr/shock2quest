// Render a playtest run into a self-contained, local HTML timeline.
//
//   node render-timeline.mjs <data.json>
//
// <data.json> lives in a directory alongside the run's screenshots (referenced
// by filename). Writes report.html next to it: a vertical timeline where each
// step shows its screenshot, what the agent observed (the note), the action it
// took, and any bug it flagged - plus a bug summary and verdict. Open the
// report.html directly (file://); images load by relative path, no server.
//
// data.json shape:
//   {
//     "mission": "medsci1", "goal": "...", "generated": "...",
//     "frontier": "medsci1 @ (x,y,z)",
//     "steps": [ { "index":1, "title":"...", "screenshot":"pt-01.png",
//                  "observation":"what the agent saw",
//                  "action":"what it did",
//                  "bug": { "severity":"Low", "class":"[visual]", "detail":"..." } | null } ],
//     "bugs":  [ { "title":"...", "severity":"High|Med|Low", "class":"[gameplay|visual|functionality]",
//                  "screenshot":"pt-02.png", "detail":"...", "issue":"#123", "fixPr":"#124" } ],
//     "verdict": "..."
//   }
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join, extname } from "node:path";

const dataPath = process.argv[2];
if (!dataPath) { console.error("usage: node render-timeline.mjs <data.json>"); process.exit(1); }
const data = JSON.parse(readFileSync(dataPath, "utf8"));
const dir = dirname(dataPath);

// Inline each screenshot as a base64 data URI so report.html is a single,
// portable, self-contained file (images render even if moved/shared).
const mime = { ".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".gif": "image/gif" };
const embedCache = new Map();
function embed(filename) {
  if (!filename) return null;
  if (embedCache.has(filename)) return embedCache.get(filename);
  const p = join(dir, filename);
  let uri = null;
  if (existsSync(p)) {
    const b64 = readFileSync(p).toString("base64");
    uri = `data:${mime[extname(filename).toLowerCase()] || "image/png"};base64,${b64}`;
  }
  embedCache.set(filename, uri);
  return uri;
}

const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

// Inline an mp4 session recording (if `data.video` is set) as a base64 data URI.
function embedVideo(filename) {
  if (!filename) return "";
  const p = join(dir, filename);
  if (!existsSync(p)) return `<p class="missing">⚠ missing video: ${esc(filename)}</p>`;
  const b64 = readFileSync(p).toString("base64");
  return `<video controls preload="metadata" src="data:video/mp4;base64,${b64}"></video>`;
}

const sevColor = { high: "#e5484d", med: "#f5a524", medium: "#f5a524", low: "#8b8d98" };
const badge = (sev) => `<span class="sev" style="background:${sevColor[String(sev).toLowerCase()] || "#8b8d98"}">${esc(sev)}</span>`;

const stepHtml = (s) => `
  <div class="step${s.bug ? " has-bug" : ""}">
    <div class="dot"></div>
    <div class="card">
      <div class="hd"><span class="ix">${esc(s.index)}</span><h3>${esc(s.title)}</h3></div>
      ${embed(s.screenshot) ? `<img loading="lazy" src="${embed(s.screenshot)}" alt="${esc(s.title)}">` : s.screenshot ? `<p class="missing">⚠ missing screenshot: ${esc(s.screenshot)}</p>` : ""}
      ${s.observation ? `<p class="obs"><b>Saw:</b> ${esc(s.observation)}</p>` : ""}
      ${s.action ? `<p class="act"><b>Did:</b> ${esc(s.action)}</p>` : ""}
      ${s.bug ? `<p class="bug">${badge(s.bug.severity)} <b>${esc(s.bug.class)}</b> ${esc(s.bug.detail)}</p>` : ""}
    </div>
  </div>`;

const bugRow = (b) => `
  <tr>
    <td>${badge(b.severity)}</td><td>${esc(b.class)}</td>
    <td><b>${esc(b.title)}</b><br><span class="muted">${esc(b.detail)}</span></td>
    <td>${embed(b.screenshot) ? `<img class="thumb" src="${embed(b.screenshot)}" alt="${esc(b.title)}">` : "-"}</td>
    <td>${b.issue ? esc(b.issue) : "-"}${b.fixPr ? ` → ${esc(b.fixPr)}` : ""}</td>
  </tr>`;

const html = `<!doctype html><html><head><meta charset="utf-8">
<title>Playtest — ${esc(data.mission)}</title>
<style>
  :root { color-scheme: dark; }
  body { margin:0; background:#0f1012; color:#e8e8ea; font:15px/1.5 system-ui,-apple-system,Segoe UI,Roboto,sans-serif; }
  .wrap { max-width: 880px; margin: 0 auto; padding: 28px 20px 80px; }
  h1 { font-size: 24px; margin: 0 0 4px; }
  .sub { color:#9fa0a8; margin: 0 0 20px; }
  .sub b { color:#e8e8ea; }
  .sev { color:#0f1012; font-weight:700; font-size:11px; padding:1px 7px; border-radius:10px; }
  table { width:100%; border-collapse:collapse; margin: 8px 0 28px; }
  th,td { text-align:left; padding:8px 10px; border-bottom:1px solid #24262c; vertical-align:top; font-size:13.5px; }
  th { color:#9fa0a8; font-weight:600; }
  .muted { color:#9fa0a8; }
  .timeline { position:relative; margin-left: 10px; padding-left: 26px; border-left: 2px solid #2a2c33; }
  .step { position:relative; margin: 0 0 22px; }
  .dot { position:absolute; left:-35px; top:6px; width:12px; height:12px; border-radius:50%; background:#4c78ff; border:2px solid #0f1012; }
  .step.has-bug .dot { background:#e5484d; }
  .card { background:#17181c; border:1px solid #24262c; border-radius:10px; padding:12px 14px; }
  .hd { display:flex; align-items:center; gap:9px; margin-bottom:6px; }
  .hd h3 { margin:0; font-size:16px; }
  .ix { background:#2a2c33; color:#c8c9cf; font-size:12px; font-weight:700; width:22px; height:22px; border-radius:50%; display:grid; place-items:center; }
  .card img { display:block; width:100%; border-radius:7px; margin:6px 0 8px; border:1px solid #24262c; }
  .obs, .act, .bug { margin:4px 0; font-size:14px; }
  .obs b, .act b { color:#9fa0a8; font-weight:600; }
  .bug { background:#241416; border:1px solid #4a2327; border-radius:7px; padding:7px 9px; }
  .missing { color:#f5a524; font-size:13px; }
  video { width:100%; border-radius:10px; border:1px solid #24262c; margin:8px 0 20px; background:#000; }
</style></head><body><div class="wrap">
  <h1>Playtest — ${esc(data.mission)}</h1>
  <p class="sub">goal: <b>${esc(data.goal || "explore & QA")}</b> · frontier: <b>${esc(data.frontier || "-")}</b> · ${esc(data.generated || "")}</p>
  ${data.verdict ? `<p class="sub">${esc(data.verdict)}</p>` : ""}
  ${embedVideo(data.video)}

  <h2>Issues found (${(data.bugs || []).length})</h2>
  <table><thead><tr><th>Sev</th><th>Class</th><th>Issue</th><th>Shot</th><th>Filed → Fix</th></tr></thead>
  <tbody>${(data.bugs || []).map(bugRow).join("") || `<tr><td colspan="5" class="muted">none</td></tr>`}</tbody></table>

  <h2>Timeline (${(data.steps || []).length} steps)</h2>
  <div class="timeline">${(data.steps || []).map(stepHtml).join("") || `<p class="muted">no steps</p>`}</div>
</div></body></html>`;

const out = join(dir, "report.html");
writeFileSync(out, html);
console.log(`wrote ${out}`);
