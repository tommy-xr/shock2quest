import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { selectPsiPower } from "./helpers/psi.js";
import { pullTrigger } from "./helpers/weapon.js";

test("psi casts use authored activation sounds and sustained expiry plays deactivation once", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 10 });
  await selectPsiPower(game, "Quickness");
  await pullTrigger(game);
  const activation = (await game.audio.recent({ sample: "psiagiac" })).sounds;
  assert.equal(activation.length, 1, "Agility must play its own activation sample");
  assert.ok(activation[0]!.tags.some(([tag, value]) => tag === "event" && value === "activate"));
  assert.equal((await game.audio.recent({ sample: "psiagide" })).sounds.length, 0);
  const remaining = (await game.info()).player.stats!.modifiers.find(m => m.source === "psi:quickness")!.remaining;
  await game.step({ frames: (remaining.secs + 2) * 60 });
  assert.ok(!(await game.info()).player.active_psi_powers.includes("Quickness"));
  assert.equal((await game.audio.recent({ sample: "psiagide" })).sounds.length, 1);
  await game.step({ frames: 60 });
  assert.equal((await game.audio.recent({ sample: "psiagide" })).sounds.length, 1);

  // Death ends a newly active power once, even while the death sequence ticks.
  await pullTrigger(game);
  const player = (await game.info()).player;
  assert.ok(player.active_psi_powers.includes("Quickness"));
  await game.entities.sendMessage(player.entity_id!, { type: "Damage", amount: player.hit_points! + 100 });
  await game.step({ frames: 60 });
  assert.equal((await game.audio.recent({ sample: "psiagide" })).sounds.length, 2);
});

test("an attack cancels Invisibility with its deactivation schema only once", {
  skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
}, async () => {
  await using game = await GameServer.launch({ mission: "debug_psi" });
  await game.step({ frames: 10 });
  await selectPsiPower(game, "Inviso");
  await pullTrigger(game);
  assert.ok((await game.info()).player.active_psi_powers.includes("Inviso"));
  await game.player.spawnItem("Pistol");
  await game.input.trigger("EquipPistol");
  await game.step({ frames: 10 });
  const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
  await pullTrigger(game);
  await game.step({ frames: 60 });
  const ending = (await game.audio.recent()).sounds.filter(sound => sound.sequence > before
    && sound.tags.some(([tag, value]) => tag === "event" && value === "deactivate"));
  assert.equal(ending.length, 1);
  assert.ok(!(await game.info()).player.active_psi_powers.includes("Inviso"));
  await pullTrigger(game);
  await game.step({ frames: 60 });
  assert.equal((await game.audio.recent()).sounds.filter(sound => sound.sequence > before
    && sound.tags.some(([tag, value]) => tag === "event" && value === "deactivate")).length, 1);
});
