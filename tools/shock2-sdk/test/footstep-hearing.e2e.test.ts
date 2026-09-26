import assert from "node:assert/strict";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { tagValue } from "./helpers/audio.js";

const enabled = process.env.SHOCK2_E2E === "1";
for (const distance of [2, 4.5]) {
  test(`player footsteps notify AI at ${distance} units with Agility 6`,
    { skip: !enabled, timeout: 600_000 }, async () => {
      await using game = await GameServer.launch({ mission: "debug_interactions" });
      await game.step({ frames: 60 });
      await game.input.trigger("SpawnDebugMonster");
      await game.step({ frames: 2 });
      const monster = (await game.entities.list({ filter: "OG-Pipe" })).entities.find(e => e.template_id === -397);
      assert.ok(monster);
      assert.equal((await game.info()).player.stats?.agility, 6);
      const pawn = (await game.info()).player.position;
      await game.player.teleport({ x: monster.position[0] + 2, y: pawn[1], z: monster.position[2] + distance });
      await game.step({ frames: 3 });
      await game.entities.sendMessage(monster.id, { type: "SetAlertness", level: "Lowest" });
      await game.step({ frames: 1 });
      const beforeAudio = (await game.audio.recent()).sounds.at(-1)?.sequence ?? 0;
      const beforeMessages = (await game.messages.recent()).messages.at(-1)?.sequence ?? 0;
      // Walk across the listener's front. Stop at the first real paced step,
      // well before vision could escalate the calm AI to Moderate.
      await game.input.set("right_hand.thumbstick", [0, 1]);
      let step;
      for (let frame = 0; frame < 60 && !step; frame++) {
        await game.step({ frames: 1 });
        step = (await game.audio.recent()).sounds.find(s => s.sequence > beforeAudio
          && tagValue(s, "event") === "footstep" && tagValue(s, "creaturetype") === "player");
      }
      await game.input.set("right_hand.thumbstick", [0, 0]);
      assert.ok(step, "walking must play an actual player footstep");
      await game.step({ frames: 2 });
      const heard = (await game.messages.recent()).messages.filter(m => m.sequence > beforeMessages
        && m.to.entity_id === monster.id && m.payload === "HeardNoise");
      assert.equal(heard.length > 0, distance === 2,
        `Agility 6 gives a 3-unit footstep range; source=${step.position}, heard=${JSON.stringify(heard)}`);
    });
}
