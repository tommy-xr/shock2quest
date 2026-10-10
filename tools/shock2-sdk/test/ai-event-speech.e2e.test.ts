import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type PlayedSound } from "../src/index.js";
import { tagValue } from "./helpers/audio.js";

const enabled = process.env.SHOCK2_E2E === "1";

test("mobile AI plays committed ambush-pain speech from its own voice", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "medsci1.mis" });
  await game.step({ frames: 10 });
  const known = new Set((await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities.map(e => e.id));
  await game.input.trigger("SpawnDebugMonster");
  await game.step({ frames: 30 });
  const monster = (await game.entities.list({ filter: "OG-Pipe", limit: 50 })).entities.find(e => e.name === "OG-Pipe" && !known.has(e.id));
  assert.ok(monster);
  // Lowest alertness can still have visual contact before awareness rises.
  // Stage behind the spawned actor and verify sight, not just alertness.
  await game.player.teleport({ x: monster.position[0] - 10, y: monster.position[1], z: monster.position[2] });
  await game.step({ frames: 2 });
  assert.equal((await game.entities.detail(monster.id)).properties.find(p => p.name === "AITargetVisible")?.value, "false");
  assert.equal((await game.entities.detail(monster.id)).properties.find(p => p.name === "AIAlertness")?.value, "Lowest");

  await game.entities.sendMessage(monster.id, { type: "Damage", amount: 1 });
  await game.step({ frames: 2 });
  const pain = (await game.audio.recent()).sounds.filter(s => s.source_entity?.template_id === monster.template_id && tagValue(s, "concept") === "comhitamb");
  assert.equal(pain.length, 1, "an unaware hybrid must play one ambush reaction after losing HP");
  assert.match(pain[0].sample, /^og1/);
});

test("mobile AI plays separate alert-transition lines behind an occluding wall", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_melee" });
  // Behind the pen's back wall, on its floor: no attack line competes with
  // the lower-priority transition calls under test.
  await game.player.teleport({ x: -16, y: 1.244, z: 0 });
  await game.step({ frames: 61 });
  const monsters = await game.entities.byTemplate(-397);
  monsters.sort((a, b) => b.position[0] - a.position[0]);
  const monster = monsters[0];
  assert.ok(monster);
  for (const [level, concept] of [["Moderate", "toleveltwo"], ["Lowest", "backtozero"], ["Low", "tolevelone"], ["High", "tolevelthree"]] as const) {
    const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
    await game.entities.sendMessage(monster.id, { type: "SetAlertness", level });
    await game.step({ frames: 2 });
    const calls: PlayedSound[] = (await game.audio.recent()).sounds.filter(s => s.sequence > before && s.source_entity?.template_id === monster.template_id && tagValue(s, "concept") === concept);
    assert.equal(calls.length, 1, `${level} must play ${concept}`);
    assert.match(calls[0].sample, /^og1/);
    await game.step({ frames: Math.ceil((calls[0].duration_secs! + 0.2) * 60) });
  }
});
