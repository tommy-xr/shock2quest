import assert from "node:assert/strict";
import { test } from "node:test";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { GameServer, findRepoRoot } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) {
  test(`HUD use: authored corpse hint survives save/load (${vr ? "VR" : "flat"})`,
    { skip: !enabled, timeout: 600_000 }, async (t) => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
      await game.step({ frames: 2 });
      const corpse = (await game.entities.list()).entities
        .filter(entity => entity.name === "MS Male Corpse")
        .sort((a, b) => a.distance - b.distance)[0];
      assert.ok(corpse, "retail mission must have its authored corpse");
      const hint = async () => {
        // Concrete runtime IDs may change on load. Mission template IDs persist.
        const [entity] = await game.entities.byTemplate(corpse.template_id);
        assert.ok(entity);
        return (await game.entities.detail(entity.id)).properties
          .find(property => property.name === "HUDUse")?.value;
      };
      assert.equal(await hint(), 'human_corpses: "Search corpse"');
      const save = `hud-use-${vr ? "vr" : "flat"}-${Date.now()}`;
      t.after(() => {
        const root = findRepoRoot(process.cwd()) ?? process.cwd();
        for (const data of [process.env.DARK_ASSET_PATH, join(root, "Data"), join(root, "..", "Data")]) {
          if (data) rmSync(join(data, "saves", `${save}.sav`), { force: true });
        }
      });
      assert.equal((await game.save(save)).success, true);
      assert.equal((await game.load(save)).success, true);
      await game.step({ frames: 2 });
      assert.equal(await hint(), 'human_corpses: "Search corpse"');
    });
}
