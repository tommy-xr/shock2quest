import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";

for (const vr of [false, true]) {
  test(`PMNM named blends remain per instance while skinned in ${vr ? "VR" : "flat"}`, {
    skip: process.env.SHOCK2_E2E !== "1", timeout: 600_000,
  }, async () => {
    await using game = await GameServer.launch({ mission: "debug_ragdoll", debugFlags: vr ? ["--vr"] : [] });
    await game.step({ frames: 1 });
    await game.input.trigger("SpawnDebugMonster");
    await game.step({ frames: 1 });
    const hybrids = (await game.entities.list({ filter: "OG-Pipe", limit: 100 })).entities
      .filter((e) => e.template_id === -397);
    assert.equal(hybrids.length, 2);
    await game.camera.set({ position: [-5, 5, 12], lookAt: [-5, 4, 0] });
    await game.step({ frames: 1 });
    const shapes = async (id: number) => (await game.scene.objects({ entityId: id })).objects
      .filter((o) => o.model?.toLowerCase() === "grunt_p");
    for (const hybrid of hybrids) {
      const objects = await shapes(hybrid.id);
      assert.ok(objects.length > 0);
      assert.ok(objects.every((o) => o.morph_names?.includes("MouthShapeH") && o.morph_names.includes("worm")),
        "retain shipped named targets on every material pass");
    }
    const set = (id: number, weights: Record<string, number>) => fetch(`${game.baseUrl}/v1/scene/morphs`, {
      method: "POST", body: JSON.stringify({ entity_id: id, weights }),
    });
    const first = hybrids[0]!.id, second = hybrids[1]!.id;
    assert.equal((await set(first, { nonexistent: 1 })).status, 400);
    assert.equal((await set(first, { worm: 2 })).status, 400);
    assert.equal((await set(first, { MouthShapeA: 0.5, MouthShapeH: 0.5 })).status, 200);
    assert.equal((await set(second, { worm: 1 })).status, 200);
    await game.input.trigger("DebugHitboxCyclePose");
    await game.step({ frames: 8 });
    for (const [id, expected] of [[first, { MouthShapeA: 0.5, MouthShapeH: 0.5 }], [second, { worm: 1 }]] as const) {
      for (const object of await shapes(id)) {
        assert.deepEqual(object.morph_weights,
          object.morph_names!.map((name) => (expected as Record<string, number>)[name] ?? 0));
      }
      const animation = await (await fetch(`${game.baseUrl}/v1/entities/${id}/animation`)).json() as { clip?: string; queue?: unknown[] } | null;
      assert.ok(animation?.clip || animation?.queue?.length, "skeletal animation continues with morph weights");
    }
    // Released channels are neutral, except mouths a speaking hybrid's
    // authored lip sync drives (lip-sync.e2e covers those). Lip sync never
    // drives MouthShapeA (the closed mouth has no shape), so it stays strict.
    const neutral = async (id: number) => {
      const speaking = (await game.audio.recent({ playing: true })).sounds
        .some((s) => s.tags.some(([k, v]) => k === "kind" && v === "speech"));
      return (await shapes(id)).every((o) => o.morph_weights?.every((w, i) => {
        const name = o.morph_names?.[i];
        return name === "worm" || (speaking && name?.startsWith("MouthShape") && name !== "MouthShapeA") || w === 0;
      }));
    };
    assert.equal((await set(first, {})).status, 200);
    await game.step({ frames: 1 });
    assert.ok(await neutral(first));
    assert.ok((await shapes(second)).every((o) => o.morph_weights?.[o.morph_names!.indexOf("worm")] === 1));
    assert.equal((await set(first, { MouthShapeA: 1 })).status, 200);
    await game.input.trigger("DebugReloadLevel");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).mission, "debug_ragdoll");
    const reloaded = (await game.entities.list({ filter: "OG-Pipe", limit: 100 })).entities;
    assert.ok(reloaded.length > 0);
    for (const entity of reloaded) {
      assert.ok(await neutral(entity.id),
        "reloading the same scene clears old entity preview weights");
    }
  });
}
