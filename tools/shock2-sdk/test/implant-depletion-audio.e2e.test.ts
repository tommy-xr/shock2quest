import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const vr of [false, true]) {
  test(`implant depletion announces once and recharge restores its bonus (${vr ? "VR" : "flat"})`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_weapons", debugFlags: vr ? ["--vr"] : [] });
    let implant = (await game.player.spawnItem(-101)).entity_id;
    const property = async (name: string) => (await game.entities.detail(implant)).properties.find(p => p.name === name)?.value;
    const energy = async () => Number(await property("Energy"));
    const notices = async () => (await game.audio.recent({ sample: "bb07" })).sounds;
    const use = async () => {
      await game.entities.sendMessage(implant, { type: "Frob" });
      await game.step({ frames: 2 });
    };
    const strength = (await game.info()).player.effective_stats!.strength;
    await use();
    assert.equal((await game.info()).player.effective_stats!.strength, strength + 1);
    // Exercise the authored drain, not an injected energy/property change.
    while (await energy() > 1) {
      const ticks = Math.min(10, await energy() - 1);
      await game.step({ frames: ticks * 600 });
    }
    assert.equal(await energy(), 1);
    assert.equal((await notices()).length, 0, "no premature depletion warning");
    await game.step({ frames: 600 });
    assert.equal(await energy(), 0);
    assert.equal((await notices()).length, 1, "the zero-charge transition announces once");
    assert.equal((await notices())[0].source_entity?.template_id, -101);
    assert.equal((await game.info()).player.effective_stats!.strength, strength);
    assert.equal(await property("ImplantSlot"), "0", "depletion does not eject the implant");
    await game.step({ frames: 1200 });
    assert.equal((await notices()).length, 1, "remaining depleted is silent");
    await game.transitionLevel("earth.mis");
    const [carried] = await game.entities.byTemplate(-101);
    assert.ok(carried);
    implant = carried.id;
    const beforeRecharge = (await notices()).length;
    const [station] = await game.entities.byTemplate(258);
    assert.ok(station);
    await game.entities.sendMessage(station.id, { type: "Frob" });
    await game.step({ frames: 2 });
    assert.ok(await energy() > 0);
    assert.equal((await game.info()).player.effective_stats!.strength, strength + 1);
    assert.equal((await notices()).length, beforeRecharge, "recharging is not a depletion transition");
    await use();
    const storedEnergy = await energy();
    await game.step({ frames: 1200 });
    assert.equal(await energy(), storedEnergy, "removed implants stop draining");
    assert.equal((await notices()).length, beforeRecharge, "removal is silent");
    assert.equal((await game.info()).player.effective_stats!.strength, strength);
  });
}
