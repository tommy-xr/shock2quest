import assert from "node:assert/strict";
import childProcess from "node:child_process";
import { readFileSync } from "node:fs";
import { syncBuiltinESMExports } from "node:module";
import { beforeEach, test } from "node:test";

import {
  MISSION_CONFIG,
  missionSelection,
  restoreMissionSelection,
  selectMission,
} from "./quest-device.mjs";

let device;
beforeEach((t) => {
  device = { exists: false, contents: Buffer.alloc(0), failure: null };
  t.mock.method(childProcess, "execFileSync", (command, args, options) => {
    assert.equal(command, "adb");
    assert.deepEqual(args.slice(0, 2), ["-s", "test-device"]);
    if (device.failure === "offline") throw new Error("device offline");
    const [operation, ...values] = args.slice(2);
    let output = "";
    if (operation === "exec-out") {
      // Reproduce adb exec-out's successful host exit on a remote cat error.
      assert.deepEqual(values, ["cat", MISSION_CONFIG]);
      if (device.failure === "read") throw new Error("read failed");
      output = device.exists
        ? device.contents
        : `cat: ${MISSION_CONFIG}: No such file or directory\n`;
    } else if (operation === "shell" && values[0].startsWith("if [ -e ")) {
      output = device.failure === "probe"
        ? "unexpected output"
        : device.exists ? "present\n" : "absent\n";
    } else if (operation === "shell" && values[0] === "-T") {
      assert.deepEqual(values, ["-T", "cat", MISSION_CONFIG]);
      if (device.failure === "read") throw new Error("read failed");
      assert.ok(device.exists);
      output = device.contents;
    } else if (operation === "push") {
      assert.equal(values[1], MISSION_CONFIG);
      device.contents = readFileSync(values[0]);
      device.exists = true;
    } else if (operation === "shell" && values[0] === "rm") {
      assert.deepEqual(values, ["rm", "-f", MISSION_CONFIG]);
      device.exists = false;
    } else {
      assert.equal(operation, "shell");
      assert.equal(values[0], "mkdir");
    }
    return options.encoding === null ? Buffer.from(output) : output.toString();
  });
  syncBuiltinESMExports();
  t.after(() => {
    t.mock.restoreAll();
    syncBuiltinESMExports();
  });
});

test("a missing selector stays missing after a temporary mission", () => {
  const previous = missionSelection("test-device");
  assert.deepEqual(previous, { exists: false, contents: Buffer.alloc(0) });
  selectMission("test-device", "rec1.mis");
  assert.ok(device.exists);
  restoreMissionSelection("test-device", previous);
  assert.equal(device.exists, false);
});

test("restores existing selector bytes without trimming or newline conversion", () => {
  device.exists = true;
  const original = Buffer.from("medsci1.mis\r\n\n");
  device.contents = original;
  const previous = missionSelection("test-device");
  selectMission("test-device", "rec1.mis");
  restoreMissionSelection("test-device", previous);
  assert.ok(device.exists);
  assert.deepEqual(device.contents, original);
});

test("an empty selector is distinct from an absent selector", () => {
  device.exists = true;
  const previous = missionSelection("test-device");
  selectMission("test-device", "rec1.mis");
  restoreMissionSelection("test-device", previous);
  assert.ok(device.exists);
  assert.deepEqual(device.contents, Buffer.alloc(0));
});

test("connection errors abort the snapshot instead of reporting absence", () => {
  device.failure = "offline";
  assert.throws(() => missionSelection("test-device"), /device offline/);
});

test("read errors abort the snapshot instead of reporting absence", () => {
  device.exists = true;
  device.failure = "read";
  assert.throws(() => missionSelection("test-device"), /read failed/);
});

test("unexpected probe output cannot be treated as an absent selector", () => {
  device.failure = "probe";
  assert.throws(() => missionSelection("test-device"), /mission selector/);
});
