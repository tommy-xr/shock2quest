import assert from "node:assert/strict";
import { test } from "node:test";
import { e2ePort } from "./helpers/e2e-port.js";

test("fixture port defaults and explicit zero remain ephemeral at every offset", () => {
  for (const offset of [0, 1, 5]) {
    assert.equal(e2ePort(offset, "PORT", {}), 0);
    assert.equal(e2ePort(offset, "PORT", { PORT: "0" }), 0);
  }
});

test("fixture port preserves exact developer overrides and offsets", () => {
  assert.equal(e2ePort(2, "PORT", { PORT: "9100" }), 9102);
});

test("fixture port rejects invalid explicit ports instead of drifting", () => {
  for (const value of ["", "oops", "-1", "1.5", "65536"]) {
    assert.throws(() => e2ePort(0, "PORT", { PORT: value }), /PORT/);
  }
  assert.throws(() => e2ePort(1, "PORT", { PORT: "65535" }), /PORT/);
});
