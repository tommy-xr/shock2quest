// Review overlay, loaded by index.html on ?feedback=1: draw boxes on the page, note
// each one, export JSON for an agent to act on (see .claude/skills/website-feedback).
import { buildExport, elementText, exportFilename, iou, normalizeRect, selectorFor } from "./feedback-core.js";

// Work in progress survives reloads; storage can be unavailable, so it is best-effort.
const KEY = "s2q-feedback:" + location.pathname;
let state = { overall: "", annotations: [] };
try { state = JSON.parse(localStorage.getItem(KEY)) ?? state; } catch {}
const save = () => { try { localStorage.setItem(KEY, JSON.stringify(state)); } catch {} };

const css = `
#fb-panel,#fb-panel *{box-sizing:border-box;font:13px/1.4 system-ui,sans-serif}
#fb-panel{position:fixed;right:12px;bottom:12px;z-index:10001;width:min(320px,calc(100vw - 24px));max-height:70vh;overflow:auto;background:#111;color:#eee;border:2px solid #ff3ea5;padding:10px;display:grid;gap:8px}
#fb-panel b{font-weight:700;color:#ff3ea5}
#fb-panel textarea{width:100%;min-height:52px;background:#000;color:#eee;border:1px solid #555;padding:4px}
#fb-panel .row{display:flex;gap:6px;flex-wrap:wrap}
#fb-panel button{background:#333;color:#eee;border:1px solid #666;padding:3px 8px;cursor:pointer}
#fb-panel button[aria-pressed=true]{background:#ff3ea5;color:#000}
#fb-panel ol{margin:0;padding-left:20px}
#fb-panel li{margin-bottom:4px}
#fb-panel li small{display:block;color:#999;word-break:break-all}
#fb-layer{position:fixed;inset:0;z-index:10000;cursor:crosshair;display:none}
#fb-layer.on{display:block}
.fb-box{position:absolute;border:2px dashed #ff3ea5;background:rgba(255,62,165,.08);pointer-events:none;z-index:9999}
.fb-box span{position:absolute;right:-2px;top:-20px;background:#ff3ea5;color:#000;font:700 12px system-ui;padding:0 5px}
#fb-draft{position:fixed}`;
document.head.append(Object.assign(document.createElement("style"), { textContent: css }));

const layer = Object.assign(document.createElement("div"), { id: "fb-layer" });
const panel = Object.assign(document.createElement("div"), { id: "fb-panel" });
panel.innerHTML = `
<b>Feedback</b>
<div class="row"><button data-act="annotate" aria-pressed="false">Annotate (A)</button><button data-act="export">Export JSON</button><button data-act="clear">Clear</button></div>
<div data-pending hidden><textarea placeholder="Note for this box (Ctrl+Enter saves, Esc cancels)"></textarea><div class="row"><button data-act="save">Save note</button><button data-act="cancel">Cancel</button></div></div>
<label>Overall<textarea data-overall placeholder="General feedback on the page"></textarea></label>
<ol></ol>`;
document.body.append(layer, panel);

const $ = (s) => panel.querySelector(s);
const pendingBox = $("[data-pending]"), pendingNote = pendingBox.querySelector("textarea");
const annotateBtn = $('[data-act="annotate"]'), list = $("ol"), overall = $("[data-overall]");
overall.value = state.overall;
overall.oninput = () => { state.overall = overall.value; save(); };

// Boxes live in document coordinates so they scroll with the content they mark;
// the draft box is viewport-fixed while the drag is in progress.
function boxEl(rect, label, draft = false) {
  const d = Object.assign(document.createElement("div"), { className: "fb-box" });
  if (draft) d.id = "fb-draft";
  place(d, rect);
  if (label) d.append(Object.assign(document.createElement("span"), { textContent: label }));
  return d;
}
const place = (d, r) => Object.assign(d.style, { left: r.x + "px", top: r.y + "px", width: r.w + "px", height: r.h + "px" });

let marks = [];
function render() {
  marks.forEach((m) => m.remove());
  marks = state.annotations.map((a, i) => boxEl(a.rect, String(i + 1)));
  document.body.append(...marks);
  list.replaceChildren(...state.annotations.map((a, i) => {
    const li = document.createElement("li");
    const del = Object.assign(document.createElement("button"), { textContent: "×", title: "Delete" });
    del.onclick = () => { state.annotations.splice(i, 1); save(); render(); };
    li.append(a.note + " ", del, Object.assign(document.createElement("small"), { textContent: a.selector }));
    return li;
  }));
}

const setAnnotating = (on) => { layer.classList.toggle("on", on); annotateBtn.setAttribute("aria-pressed", on); };

let pending = null; // { box, annotation } awaiting its note
function finishPending(keep) {
  const note = pendingNote.value.trim();
  if (keep && pending && note) state.annotations.push({ ...pending.annotation, note });
  pending?.box.remove();
  pending = null;
  pendingBox.hidden = true;
  pendingNote.value = "";
  save();
  render();
}

// Element under a viewport point, looking through the capture layer.
function hit(x, y) {
  layer.style.pointerEvents = "none";
  const el = document.elementFromPoint(x, y);
  layer.style.pointerEvents = "";
  return el && !panel.contains(el) ? el : document.body;
}

// For a drag: of the element under the box centre and its ancestors, the one whose
// rect best matches the box - a box drawn around a heading names the heading, not a
// span inside it.
function enclosing(r) {
  let best = document.body, score = -1;
  for (let e = hit(r.x + r.w / 2, r.y + r.h / 2); e && e !== document.documentElement; e = e.parentElement) {
    const b = e.getBoundingClientRect(), s = iou(r, { x: b.left, y: b.top, w: b.width, h: b.height });
    if (s > score) { best = e; score = s; }
  }
  return best;
}

// A drag marks a region; a click (under 4px of travel) marks the element clicked.
let start = null, draft = null;
layer.addEventListener("pointerdown", (e) => {
  if (pending) finishPending(false);
  start = { x: e.clientX, y: e.clientY };
  draft = boxEl({ ...start, w: 0, h: 0 }, "", true);
  document.body.append(draft);
  layer.setPointerCapture(e.pointerId);
});
layer.addEventListener("pointermove", (e) => {
  if (start) place(draft, normalizeRect(start, { x: e.clientX, y: e.clientY }));
});
layer.addEventListener("pointerup", (e) => {
  if (!start) return;
  let r = normalizeRect(start, { x: e.clientX, y: e.clientY });
  start = null;
  draft.remove();
  const click = r.w < 4 && r.h < 4;
  const target = click ? hit(e.clientX, e.clientY) : enclosing(r);
  if (click) { const b = target.getBoundingClientRect(); r = normalizeRect({ x: b.left, y: b.top }, { x: b.right, y: b.bottom }); }
  const rect = { ...r, x: r.x + Math.round(scrollX), y: r.y + Math.round(scrollY) };
  const box = boxEl(rect, String(state.annotations.length + 1));
  document.body.append(box);
  pending = {
    box,
    annotation: {
      shape: "box", rect, viewport: { w: innerWidth, h: innerHeight }, devicePixelRatio,
      selector: selectorFor(target), tag: target.tagName.toLowerCase(), text: elementText(target),
    },
  };
  pendingBox.hidden = false;
  pendingNote.focus();
});

function exportJson() {
  const created = new Date();
  const json = JSON.stringify(buildExport({ url: location.href, created, overall: state.overall, annotations: state.annotations }), null, 2);
  const a = Object.assign(document.createElement("a"), {
    href: URL.createObjectURL(new Blob([json], { type: "application/json" })), download: exportFilename(created),
  });
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

panel.addEventListener("click", (e) => {
  const act = e.target.dataset?.act;
  if (act === "annotate") setAnnotating(!layer.classList.contains("on"));
  else if (act === "export") exportJson();
  else if (act === "save" || act === "cancel") finishPending(act === "save");
  else if (act === "clear" && confirm("Delete all feedback on this page?")) {
    finishPending(false);
    state = { overall: "", annotations: [] };
    overall.value = "";
    save();
    render();
  }
});
pendingNote.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) finishPending(true);
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") { if (pending) finishPending(false); else setAnnotating(false); }
  else if (e.key.toLowerCase() === "a" && !e.ctrlKey && !e.metaKey && !e.altKey && !/^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName)) {
    setAnnotating(!layer.classList.contains("on"));
  }
});
render();
