// node --test website/test/*.test.js
import assert from "node:assert/strict";
import { test } from "node:test";

import { FORMAT, buildExport, elementText, exportFilename, iou, normalizeRect, selectorFor } from "../shared/feedback-core.js";

// Minimal element chain: just what selectorFor reads.
function el(tagName, { id = "", className = "", dataset = {} } = {}, parent = null) {
  const e = { tagName: tagName.toUpperCase(), id, className, dataset, parentElement: parent, children: [] };
  parent?.children.push(e);
  return e;
}

test("an id short-circuits the path", () => {
  const body = el("body");
  const main = el("main", { id: "controls" }, body);
  const p = el("p", {}, main);
  assert.equal(selectorFor(main), "#controls");
  assert.equal(selectorFor(p), "#controls > p");
});

test("classes beat data-* (often runtime state), nth-of-type disambiguates same-tag siblings", () => {
  const body = el("body");
  const sec = el("section", { className: "hero dark" }, body);
  el("p", {}, sec);
  const p2 = el("p", { dataset: { d: "pre" } }, sec);
  const div = el("div", { className: "chip" }, sec);
  const view = el("span", { className: "view", dataset: { tracked: "true" } }, sec);
  assert.equal(selectorFor(p2), 'body > section.hero.dark > p[data-d="pre"]:nth-of-type(2)');
  assert.equal(selectorFor(div), "body > section.hero.dark > div.chip");
  assert.equal(selectorFor(view), "body > section.hero.dark > span.view");
});

test("camelCase dataset keys become kebab attributes; unsafe ids are skipped", () => {
  const body = el("body");
  const a = el("a", { id: "1bad id", dataset: { fooBar: 'x"y' } }, body);
  assert.equal(selectorFor(a), 'body > a[data-foo-bar="x\\"y"]');
});

test("elementText trims, collapses whitespace and caps at 80 chars", () => {
  assert.equal(elementText({ textContent: "\n  Get   it\n on Quest  " }), "Get it on Quest");
  assert.equal(elementText({ textContent: "x".repeat(100) }).length, 80);
});

test("normalizeRect handles drags in any direction", () => {
  const want = { x: 10, y: 20, w: 30, h: 40 };
  assert.deepEqual(normalizeRect({ x: 10, y: 20 }, { x: 40, y: 60 }), want);
  assert.deepEqual(normalizeRect({ x: 40, y: 60 }, { x: 10, y: 20 }), want);
  assert.deepEqual(normalizeRect({ x: 40, y: 20 }, { x: 10, y: 60 }), want);
  assert.deepEqual(normalizeRect({ x: 10, y: 60 }, { x: 40, y: 20 }), want);
});

test("export has the documented shape and a filesystem-safe name", () => {
  const created = new Date("2026-10-17T12:34:56.789Z");
  const ann = { shape: "box", rect: { x: 1, y: 2, w: 3, h: 4 }, viewport: { w: 800, h: 600 }, devicePixelRatio: 2, selector: "#hero", tag: "section", text: "hi", note: "bigger" };
  const out = buildExport({ url: "http://x/website/?feedback=1", created, overall: " tone ", annotations: [ann] });
  assert.deepEqual(Object.keys(out), ["format", "url", "created", "overall", "annotations"]);
  assert.equal(out.format, FORMAT);
  assert.equal(FORMAT, "shock2quest-website-feedback/1");
  assert.equal(out.created, "2026-10-17T12:34:56.789Z");
  assert.equal(out.overall, "tone");
  assert.deepEqual(out.annotations, [ann]);
  assert.equal(exportFilename(created), "feedback-2026-10-17T12-34-56-789Z.json");
});

test("iou scores how well an element's rect matches a drawn box", () => {
  const box = { x: 0, y: 0, w: 10, h: 10 };
  assert.equal(iou(box, box), 1);
  assert.equal(iou(box, { x: 20, y: 0, w: 10, h: 10 }), 0);
  assert.equal(iou(box, { x: 5, y: 0, w: 10, h: 10 }), 50 / 150);
  assert.equal(iou(box, { x: 0, y: 0, w: 0, h: 0 }), 0);
});
