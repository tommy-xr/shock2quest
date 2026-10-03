import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer, PLAYER_EYE_HEIGHT_WORLD } from "../src/index.js";

const enabled = process.env.SHOCK2_E2E === "1";

for (const vr of [false, true]) {
  test(`loading fades into and out of the ${vr ? "VR" : "flat"} presentation`,
    { skip: !enabled, timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({
        mission: "medsci1.mis",
        debugFlags: ["--defer-transitions", ...(vr ? ["--vr"] : [])],
      });
      const loadingObjects = async () => (await game.scene.objects()).objects
        .filter(object => object.source === "loading_screen");
      await game.step({ frames: 30 });
      await game.input.trigger("DebugReloadLevel");
      await game.step({ frames: 3 });
      assert.equal((await game.info()).mission, "loading");
      const entering = await loadingObjects();
      assert.equal(entering.length, 3);
      // Flat image alpha lives in its screen shader, outside the scene
      // summary. The bar and VR materials expose it; matched screenshots
      // verify the flat backdrop/disc alongside this shared-canvas contract.
      for (const object of entering.filter(object => object.transparency !== null)) {
        assert.ok(object.transparency! > 0.5 && object.transparency! < 1,
          `all three shared canvas elements should be fading in together: ${JSON.stringify(entering)}`);
      }
      await game.step({ frames: 10 });
      assert.equal((await game.info()).mission, "loading");
      for (const object of (await loadingObjects()).filter(object => object.transparency !== null)) {
        assert.equal(object.transparency, 0, "entry fade should reach full opacity");
      }
      // Parsing runs on a worker; synchronize by the real scene boundary,
      // never assume that it finishes on a particular simulation frame.
      for (let frame = 0; frame < 600 && (await game.info()).mission === "loading"; frame++) {
        await game.step({ frames: 1 });
      }
      assert.equal((await game.info()).mission, "medsci1.mis");
      const exiting = await loadingObjects();
      assert.equal(exiting.length, 3, "the completed panel must survive the scene swap");
      const player = await game.player.position();
      for (const object of exiting) {
        assert.equal(object.render_layer, "system_overlay");
        if (object.transparency !== null) assert.ok(object.transparency > 0 && object.transparency < 0.1);
        if (vr) {
          const [x, y, z] = object.position;
          const distance = Math.hypot(x - player.x, y - player.y - PLAYER_EYE_HEIGHT_WORLD, z - player.z);
          assert.ok(distance > 1 && distance < 5,
            `released VR panel should follow the new pawn coordinate frame, distance ${distance}`);
        } else {
          assert.deepEqual(object.position, [0, 0, 0]);
        }
      }
      // Movement is deliberately suspended while the exit panel still covers
      // the destination; it resumes once the fade finishes.
      await game.input.set("right_hand.thumbstick", [0, 1]);
      await game.step({ frames: 5 });
      assert.deepEqual(await game.player.position(), player);
      const middle = await loadingObjects();
      assert.equal(middle.length, 3);
      assert.ok(middle[2].transparency! > exiting[2].transparency!);
      await game.step({ frames: 40 });
      assert.notDeepEqual(await game.player.position(), player,
        "held locomotion should resume when the completed panel releases");
      await game.input.set("right_hand.thumbstick", [0, 0]);
      assert.equal((await loadingObjects()).length, 0, "release must remove the panel completely");
    });
}
