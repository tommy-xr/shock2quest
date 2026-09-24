import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntitySummary, PlayedSound } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";

/// The entity named `name` authored at (x, z) - runtime ids change every launch.
async function at(
  game: GameServer,
  name: string,
  x: number,
  z: number,
): Promise<EntitySummary> {
  const found = (await game.entities.list({ filter: name, limit: 500 })).entities.find(
    (entity) => Math.abs(entity.position[0] - x) < 0.5 && Math.abs(entity.position[2] - z) < 0.5,
  );
  assert.ok(found, `no ${name} at (${x}, ${z})`);
  return found;
}

/// The one play of `sample` - a trap must not play its line twice.
async function playedOnce(game: GameServer, sample: string): Promise<PlayedSound> {
  const sounds = (await game.audio.recent({ sample })).sounds;
  assert.equal(sounds.length, 1, `expected one ${sample}, got ${JSON.stringify(sounds)}`);
  return sounds[0]!;
}

/// Listener-relative plays record the origin as their position.
function atEars(sound: PlayedSound): boolean {
  return sound.position.every((axis) => axis === 0);
}

// medsci1 authors both flavours of sound trap: TrapSoundAmb plays at the
// listener (a Xerxes message a floor away from the button that fires it),
// TrapSound at the trap.
test(
  "medsci1 sound traps: TrapSoundAmb plays at the listener, TrapSound at the trap",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({ mission: "medsci1.mis" });
    await game.step({ frames: 10 });

    // Button -> TrapSoundAmb "Xxrmsg12" (this is Xerxes...).
    const button = await at(game, "Button", 2.73, -8.48);
    await game.entities.sendMessage(button.id, { type: "Frob" });
    await game.step({ frames: 30 });
    // The button authors two SwitchLinks to this trap, so count plays loosely;
    // what matters is that none of them is positional.
    const ambient = (await game.audio.recent({ sample: "xxrmsg12" })).sounds;
    assert.ok(ambient.length > 0, "the button should fire its Xerxes message");
    assert.ok(
      ambient.every(atEars),
      `TrapSoundAmb must not be positional, got ${JSON.stringify(ambient.map((s) => s.position))}`,
    );

    // Tripwire -> email -> delay chain -> TrapSound "trg0202" (decompression).
    const tripwire = await at(game, "Tripwire", -40.49, 17.74);
    await game.player.teleport({
      x: tripwire.position[0],
      y: tripwire.position[1] + 0.8,
      z: tripwire.position[2],
    });
    await game.step({ frames: 20 * 60 });
    const spatial = await playedOnce(game, "trg0202");
    assert.ok(!atEars(spatial), "TrapSound plays at the trap");
  },
);
