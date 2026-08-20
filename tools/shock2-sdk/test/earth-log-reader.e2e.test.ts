import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { UiPanel } from "../src/types.js";
import { dataRoot } from "./helpers/crf.js";

// Regression for #1061: SCP overrides Earth log 1/24's deck icon from the
// base game's RickIcon to RamsIcon. The replacement layers supply the reader
// art as PNG rather than the PCX-era spelling implied by the string table, so
// blindly appending `.pcx` made both flat and VR panic in the shared UiCanvas
// renderer.
//
// This test deliberately requires the 25th Anniversary + SCP asset stack: a
// classic install cannot exercise the overriding string/art encodings. It
// collects the real Earth object through its production Frob script, invokes
// the production reader action, renders a frame, then verifies UI, audio and
// persistent read state in both presentations.
const e2eEnabled = process.env.SHOCK2_E2E === "1";
const assets = e2eEnabled ? dataRoot() : null;
const has25thScp =
  assets !== null &&
  existsSync(path.join(assets, "sshock2.kpf")) &&
  existsSync(path.join(assets, "mods", "scp.kpf"));

const EARTH_TRAINING_LOG = 301;

const panelText = (panel: UiPanel): string =>
  panel.elements
    .filter((element) => element.kind === "text" && element.text)
    .map((element) => element.text)
    .join(" ");

async function verifyEarthLogReader(
  presentation: "flat" | "vr",
  port: number,
): Promise<void> {
  await using game = await GameServer.launch({
    mission: "earth.mis",
    port,
    debugFlags: presentation === "vr" ? ["--vr"] : [],
  });
  await game.step({ frames: 5 });

  const [disc] = await game.entities.byTemplate(EARTH_TRAINING_LOG);
  assert.ok(disc, "earth.mis must contain training audio log object 301");

  await game.entities.sendMessage(disc.id, { type: "Frob" });
  await game.step({ frames: 5 });
  assert.deepEqual((await game.info()).player.collected_logs, [
    { deck: 1, log: 24, read: false },
  ]);

  await game.input.trigger("ReadLastUnreadLog");
  await game.step({ frames: 5 });

  const panel = (await game.ui.state()).active_panel;
  assert.ok(panel, `${presentation}: the Earth log reader must render`);
  const transcript = panelText(panel);
  assert.ok(
    transcript.includes("message is coming from the audio log"),
    `${presentation}: reader must expose log 1/24's transcript: ${transcript}`,
  );
  assert.ok(
    transcript.toUpperCase().includes("TRAINER"),
    `${presentation}: reader must expose log 1/24's header: ${transcript}`,
  );

  const textures = panel.elements
    .filter((element) => element.kind === "image")
    .map((element) => element.texture?.toLowerCase());
  assert.ok(textures.includes("iface/log.pcx"));
  assert.ok(textures.includes("bayliss.png"));
  assert.ok(
    textures.includes("ramsicon.png"),
    `${presentation}: SCP's RamsIcon PNG should render (images: ${textures.join(", ")})`,
  );

  assert.ok(
    (await game.audio.recent()).sounds.some(
      (sound) => sound.sample.toLowerCase() === "log0124",
    ),
    `${presentation}: reading must play LOG0124`,
  );
  assert.deepEqual((await game.info()).player.collected_logs, [
    { deck: 1, log: 24, read: true },
  ]);

  const shot = await game.screenshot(`issue-1061-earth-reader-${presentation}.png`);
  assert.ok(shot.size_bytes > 0, `${presentation}: reader screenshot must contain pixels`);
}

test(
  "25AE Earth log 1/24 collects, reads and renders in flat and VR",
  { skip: !has25thScp, timeout: 600_000 },
  async () => {
    await verifyEarthLogReader(
      "flat",
      Number(process.env.SHOCK2_E2E_PORT ?? 8861),
    );
    await verifyEarthLogReader(
      "vr",
      Number(process.env.SHOCK2_E2E_PORT ?? 8862),
    );
  },
);
