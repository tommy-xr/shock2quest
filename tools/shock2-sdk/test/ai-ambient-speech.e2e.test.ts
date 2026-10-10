import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, type PlayedSound } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";
const concept = (sound: PlayedSound) => sound.tags.find(([key]) => key === "concept")?.[1];

test("spiders: ambient voices repeat with authored gaps and stop after death", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_melee" });
  await game.devParams.set("cheat", 1);
  await game.step({ frames: 1 });
  const [spider] = await game.entities.byTemplate(-1439);
  assert.ok(spider, "discover the adult spider each run");
  const collected = new Map<number, PlayedSound>();
  for (let i = 0; i < 8; i++) {
    await game.step({ frames: 300 });
    for (const sound of (await game.audio.recent()).sounds) {
      if (sound.source_entity?.template_id === -1439 && concept(sound) === "atlevelzero") {
        collected.set(sound.sequence, sound);
      }
    }
  }
  const calls = [...collected.values()].sort((a, b) => a.sequence - b.sequence);
  assert.ok(calls.length >= 2, `expected recurring calm calls, got ${JSON.stringify(calls)}`);
  for (const sound of calls) {
    assert.match(sound.sample, /^sp1at0_[1-4]$/);
    assert.ok(sound.duration_secs && sound.duration_secs > 0);
  }
  for (let i = 1; i < calls.length; i++) {
    const gap = calls[i].sim_time - calls[i - 1].sim_time - calls[i - 1].duration_secs!;
    assert.ok(gap >= 0.09 && gap <= 15.1, `authored spider gap is 0.1–15s, got ${gap}`);
  }
  await game.entities.sendMessage(spider.id, { type: "Damage", amount: 10000 });
  await game.step({ frames: 5 });
  const last = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
  await game.step({ frames: 1200 });
  const afterDeath = (await game.audio.recent()).sounds.filter(sound =>
    sound.sequence > last && sound.source_entity?.template_id === -1439
      && concept(sound)?.startsWith("atlevel"));
  assert.equal(afterDeath.length, 0, "dead spiders must not request ambient speech");
});

test("protocol droid: shared AI resolves its own ambient voice", {
  skip: !enabled, timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_protocol_droid" });
  await game.devParams.set("cheat", 1);
  // Behind the initial facing, still within hearing range.
  await game.player.teleport({ x: 0, y: 1.25, z: 30 });
  await game.step({ frames: 180 });
  const [droid] = await game.entities.byTemplate(-174);
  assert.ok(droid);
  const calls = (await game.audio.recent()).sounds.filter(sound =>
    sound.source_entity?.template_id === -174 && concept(sound)?.startsWith("atlevel"));
  assert.ok(calls.length > 0, `expected droid ambient speech; ${JSON.stringify((await game.audio.recent()).sounds)}`);
  assert.ok(calls.every(sound => sound.sample.startsWith("pd1")));
});
