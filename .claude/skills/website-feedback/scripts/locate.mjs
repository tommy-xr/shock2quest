#!/usr/bin/env node
// Map each annotation in a website feedback export to candidate source lines.
// Usage: node .claude/skills/website-feedback/scripts/locate.mjs <feedback.json>
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const collapse = (s) => s.replace(/\s+/g, " ").trim();

// Search terms, most specific first: the id the selector is anchored at, data-*
// attributes on the marked element, then its text.
// e.g. '#install > p[data-d="fan"]' -> ['id="install"', 'data-d="fan"', <text>].
export function searchTerms({ selector, text }) {
  const steps = selector.split(" > ");
  const terms = [];
  const id = /^#([\w-]+)$/.exec(steps[0]);
  if (id) terms.push(`id="${id[1]}"`);
  for (const [, attr] of steps.at(-1).matchAll(/\[(data-[\w-]+="(?:[^"\\]|\\.)*")\]/g)) terms.push(attr.replace(/\\(.)/g, "$1"));
  if (text) terms.push(text);
  return terms;
}

// Source lines matching the terms, deduplicated, in term order. Text is compared
// with markup stripped (textContent drops tags: "Shock 2.<br><span>In" reads
// "Shock 2.In"); text with no hit falls back to its first words (at most four,
// never all), for copy that wraps across lines or is joined from separate strings.
export function locate(annotation, files) {
  const hits = [], seen = new Set();
  const search = (term, asText) => {
    let found = false;
    for (const { path: p, text } of files) {
      text.split("\n").forEach((raw, i) => {
        const line = asText ? collapse(raw.replace(/<[^>]*>/g, "")) : raw;
        if (!line.includes(term)) return;
        found = true;
        const key = `${p}:${i + 1}`;
        if (seen.has(key)) return;
        seen.add(key);
        hits.push({ path: p, line: i + 1, term, snippet: collapse(raw).slice(0, 120) });
      });
    }
    return found;
  };
  for (const term of searchTerms(annotation)) {
    if (term !== annotation.text) { search(term, false); continue; }
    const words = collapse(term).split(" ");
    if (!search(collapse(term), true) && words.length > 2) search(words.slice(0, Math.min(4, words.length - 1)).join(" "), true);
  }
  return hits;
}

// The page's own sources; the overlay itself is never what feedback is about.
function websiteFiles(root) {
  const shared = readdirSync(path.join(root, "website/shared"))
    .filter((f) => /\.(js|css)$/.test(f) && !f.startsWith("feedback"))
    .map((f) => `website/shared/${f}`);
  return ["website/index.html", ...shared].map((p) => ({ path: p, text: readFileSync(path.join(root, p), "utf8") }));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const file = process.argv[2];
  if (!file) { console.error("usage: locate.mjs <feedback.json>"); process.exit(2); }
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../..");
  const fb = JSON.parse(readFileSync(file, "utf8"));
  const files = websiteFiles(root);
  console.log(`${fb.url}  (${fb.created})`);
  if (fb.overall) console.log(`Overall: ${fb.overall}`);
  fb.annotations.forEach((a, i) => {
    const { w, h } = a.rect, vp = a.viewport;
    console.log(`\n${i + 1}. ${a.note}\n   ${a.selector}  <${a.tag}> "${a.text}"  box ${w}x${h} at (${a.rect.x},${a.rect.y}), viewport ${vp.w}x${vp.h}`);
    const hits = locate(a, files);
    if (!hits.length) console.log("   (no source match: locate by selector in the running page)");
    for (const hit of hits.slice(0, 6)) console.log(`   ${hit.path}:${hit.line}  ${hit.snippet}`);
  });
}
