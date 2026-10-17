// node --test .claude/skills/website-feedback/scripts/locate.test.mjs
import assert from "node:assert/strict";
import test from "node:test";

import { locate, searchTerms } from "./locate.mjs";

const files = [
  {
    path: "website/index.html",
    text: [
      '<section id="install">',
      '  <h2 class="sec">Sideload in three steps.</h2>',
      "    <h1>System Shock 2.<br><span>In your hands.</span></h1>",
      '    <p data-d="fan"></p>',
    ].join("\n"),
  },
  { path: "website/shared/controls.js", text: 'export const X = {\n  fan: "Unofficial fan project, not affiliated with anyone",\n  rows: [["Right stick ← →", "Turn"]],\n};' },
];

test("terms come from the selector's attributes and the element text", () => {
  const terms = searchTerms({ selector: 'body > section#x > p[data-d="fan"]:nth-of-type(2)', text: "Hi there" });
  assert.deepEqual(terms, ['data-d="fan"', "Hi there"]);
  assert.deepEqual(searchTerms({ selector: "#install > h2.sec", text: "" }), ['id="install"']);
});

test("text matches source lines with markup stripped, so <br>-split headings resolve", () => {
  const hits = locate({ selector: "body > header.hero > div > h1", text: "System Shock 2.In your hands." }, files);
  assert.deepEqual(hits.map((h) => `${h.path}:${h.line}`), ["website/index.html:3"]);
});

test("attribute matches come before text matches, without duplicates", () => {
  const hits = locate({ selector: "#install > h2.sec", text: "Sideload in three steps." }, files);
  assert.deepEqual(hits.map((h) => [h.line, h.term]), [[1, 'id="install"'], [2, "Sideload in three steps."]]);
});

test("long text that spans markup falls back to its opening words", () => {
  const hits = locate({ selector: 'body > p[data-d="fan"]', text: "Unofficial fan project, not affiliated with anyone at all ever" }, files);
  assert.deepEqual(hits.map((h) => `${h.path}:${h.line}`), ["website/index.html:4", "website/shared/controls.js:2"]);
});

test("text joined from separate strings falls back to a shorter prefix", () => {
  // A chip renders <b>${k}</b>${v}: textContent runs the two strings together.
  const hits = locate({ selector: "#rows > div.chip:nth-of-type(2)", text: "Right stick ← →Turn" }, files);
  assert.deepEqual(hits.map((h) => `${h.path}:${h.line}`), ["website/shared/controls.js:3"]);
});
