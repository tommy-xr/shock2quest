import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

// A Face archetype (ND-lipsync.dml: OG-Pipe) moves its mouth from the authored
// facepos.str sequence while its speech plays, then returns to neutral. Needs
// the 25AE assets (PMNM mouth targets and mods/sshock2ee.kpf).
for (const vr of [false, true]) {
  test(`hybrid speech drives authored mouth shapes in ${vr ? "VR" : "flat"}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_minimal", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 1 });
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 1 });
    const hybrids = (await game.entities.list({ filter: "OG-Pipe", limit: 100 })).entities
      .filter((e) => e.template_id === -397);
    assert.equal(hybrids.length, 1);
    const at = hybrids[0]!.position;
    await game.camera.set({ position: [at[0], at[1] + 1, at[2] + 3], lookAt: [at[0], at[1] + 1, at[2]] });
    // Peak mouth-shape weight on the hybrid, failing if no part has mouth
    // targets to read (a neutral reading must come from a drawn face).
    const mouth = async () => {
      let peak = 0;
      let channels = 0;
      for (const o of (await game.scene.objects({ entityId: hybrids[0]!.id })).objects) {
        o.morph_names?.forEach((name, i) => {
          if (!name.startsWith("MouthShape")) return;
          channels++;
          peak = Math.max(peak, Math.abs(o.morph_weights?.[i] ?? 0));
        });
      }
      assert.ok(channels > 0, "the hybrid's face is drawn with mouth targets");
      return peak;
    };
    const speaking = async () => (await game.audio.recent({ playing: true })).sounds
      .some((s) => s.tags.some(([k, v]) => k === "kind" && v === "speech"));
    await game.step({ frames: 1 });
    assert.equal(await speaking(), false);
    assert.equal(await mouth(), 0, "a silent hybrid keeps a neutral face");

    // Each hit bark picks a random sample, and some are authored all-closed
    // (og1hhi_3 is "0000000000000000"), so hit until a moving one plays.
    const heard: string[] = [];
    let peak = 0;
    for (let round = 0; round < 8 && peak <= 0.2; round++) {
      const before = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
      await game.entities.sendMessage(hybrids[0]!.id, { type: "Damage", amount: 1 });
      let speech;
      for (let i = 0; i < 40; i++) {
        await game.step({ frames: 3 });
        speech ??= (await game.audio.recent()).sounds.find((s) => s.sequence > before
          && s.source_entity?.template_id === -397 && s.tags.some(([k, v]) => k === "kind" && v === "speech"));
        if (speech) peak = Math.max(peak, await mouth());
      }
      if (speech) heard.push(speech.sample);
    }
    assert.ok(heard.length > 0 && heard.every((sample) => sample.startsWith("og1")), `hybrid barks: ${heard}`);
    assert.ok(peak > 0.2, `mouth moves while speaking (${heard}, peak ${peak})`);

    // An alerted hybrid barks almost continuously, so silence it for good:
    // its death cry plays, then the corpse's face settles to neutral.
    await game.entities.sendMessage(hybrids[0]!.id, { type: "Damage", amount: 10_000 });
    for (let i = 0; i < 60 && await speaking(); i++) await game.step({ frames: 15 });
    for (let i = 0; i < 20 && !await speaking(); i++) await game.step({ frames: 3 });
    for (let i = 0; i < 60 && await speaking(); i++) await game.step({ frames: 15 });
    assert.equal(await speaking(), false, "speech finishes");
    await game.step({ frames: 1 });
    assert.equal(await mouth(), 0, "neutral once speech ends");
  });
}
