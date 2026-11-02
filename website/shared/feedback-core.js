// Pure helpers for feedback.js: no DOM globals, so node tests can import them.

export const FORMAT = "shock2quest-website-feedback/1";

// jsDelivr mirrors the repo with a JS content type; raw.githubusercontent.com serves
// text/plain + nosniff, which browsers refuse to run as a module.
export const FALLBACK_SRC = "https://cdn.jsdelivr.net/gh/tommy-xr/shock2quest@main/website/shared/feedback.js";

// Bookmark URL that opens the overlay on any copy of the page: loads the overlay
// deployed beside the page, else FALLBACK_SRC; does nothing if already open.
export const bookmarklet = () =>
  "javascript:(()=>{if(document.getElementById(\"fb-panel\"))return;" +
  "const add=(src,onerror)=>{const s=document.createElement(\"script\");s.type=\"module\";s.src=src;s.onerror=onerror;document.head.append(s)};" +
  `add(new URL("shared/feedback.js",location.href).href,()=>add("${FALLBACK_SRC}"))})()`;

const SAFE_ID = /^[A-Za-z][\w-]*$/;
const SAFE_CLASS = /^-?[_A-Za-z][\w-]*$/;

// One path step: tag, then up to two classes or else the first data-* attribute
// (classes first: data-* can be runtime state, e.g. data-tracked), then
// :nth-of-type when same-tag siblings make the step ambiguous.
function step(e) {
  let s = e.tagName.toLowerCase();
  const classes = typeof e.className === "string" // SVG elements carry an SVGAnimatedString
    ? e.className.split(/\s+/).filter((c) => SAFE_CLASS.test(c)).slice(0, 2) : [];
  const [key, value] = Object.entries(e.dataset ?? {})[0] ?? [];
  if (classes.length) s += classes.map((c) => "." + c).join("");
  else if (key !== undefined) {
    const attr = key.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
    s += `[data-${attr}="${String(value).replace(/["\\]/g, "\\$&")}"]`;
  }
  const same = [...(e.parentElement?.children ?? [])].filter((c) => c.tagName === e.tagName);
  if (same.length > 1) s += `:nth-of-type(${same.indexOf(e) + 1})`;
  return s;
}

// Unique by construction: anchored at the nearest id (assumed unique) or at body,
// with nth-of-type wherever siblings share a tag. e.g. "#controls > div.chip:nth-of-type(2)".
export function selectorFor(el) {
  const parts = [];
  for (let e = el; e; e = e.parentElement) {
    if (SAFE_ID.test(e.id)) { parts.unshift("#" + e.id); return parts.join(" > "); }
    if (e.tagName === "BODY" || e.tagName === "HTML") break;
    parts.unshift(step(e));
  }
  return ["body", ...parts].join(" > ");
}

// First 80 chars of the element's text, whitespace collapsed.
export const elementText = (el) => (el.textContent ?? "").trim().replace(/\s+/g, " ").slice(0, 80);

// Drag start/end points (any direction) to a positive-size rect.
export function normalizeRect(a, b) {
  const x = Math.round(Math.min(a.x, b.x)), y = Math.round(Math.min(a.y, b.y));
  return { x, y, w: Math.round(Math.max(a.x, b.x)) - x, h: Math.round(Math.max(a.y, b.y)) - y };
}

// Intersection over union of two rects: 1 for identical, 0 for disjoint or empty.
export function iou(a, b) {
  const w = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
  const h = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
  const inter = w > 0 && h > 0 ? w * h : 0;
  return inter && inter / (a.w * a.h + b.w * b.h - inter);
}

export const buildExport = ({ url, created, overall, annotations }) =>
  ({ format: FORMAT, url, created: created.toISOString(), overall: overall.trim(), annotations });

export const exportFilename = (created) => `feedback-${created.toISOString().replace(/[:.]/g, "-")}.json`;
