import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// The campaign-produced save contains copyrighted mission state, so it remains
// out of tree. It has all three HRM computers genuinely hacked, every authored
// shield removed, and the live SHODAN head ready for ordinary Damage.
const endingSave = process.env.SHOCK2_SHODAN_ENDING_SAVE;
const e2eEnabled = process.env.SHOCK2_E2E === "1" && Boolean(endingSave);

const SHODAN_HEAD = 298;
const SHODAN_SHIELDS = [270, 272, 274, 275, 277, 278, 279, 280] as const;
const SHODAN_COMPUTERS = [268, 264, 262] as const;
const HACKED_SHODAN_COMPUTER = 1269;
const DIE_SHODAN_DIE = 743;

test(
  "SHODAN finale: destroying the exposed head plays the ending after the authored delay",
  { skip: !e2eEnabled, timeout: 180_000 },
  async () => {
    assert.ok(endingSave);
    await using game = await GameServer.launch({
      mission: "shodan.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8251),
      debugFlags: ["--save-file", endingSave],
      rustLog: "debug_runtime=info,shock2vr=debug",
    });
    await game.step({ frames: 5 });

    const [head] = await game.entities.byTemplate(SHODAN_HEAD);
    assert.ok(head, "the authentic save should retain SHODAN's live head");
    for (const computerTemplate of SHODAN_COMPUTERS) {
      assert.equal(
        (await game.entities.byTemplate(computerTemplate)).length,
        0,
        `authentic HRM success should have replaced computer ${computerTemplate}`,
      );
    }
    assert.ok(
      (await game.entities.byTemplate(HACKED_SHODAN_COMPUTER)).length >= 3,
      "all three authentic HRM successes should retain hacked replacements",
    );
    for (const shieldTemplate of SHODAN_SHIELDS) {
      assert.equal(
        (await game.entities.byTemplate(shieldTemplate)).length,
        0,
        `authentic interlock progress should have removed shield ${shieldTemplate}`,
      );
    }
    assert.equal(
      (await game.entities.byTemplate(DIE_SHODAN_DIE)).length,
      1,
      "the authored terminal DIE-SHODAN-DIE object should exist",
    );
    assert.equal(
      await game.quests.get("SOLUS"),
      "unknown",
      "the authentic save must exercise the authored unset-SOLUS negative filter",
    );

    // Damage enters the same health/script queue as a projectile hit. Do not
    // inject Slay or activate any finale trap directly: the head's actual
    // TriggerDestroy -> 5s TrapDelay -> SOLUS TrapQBNegFilter chain must run.
    await game.entities.sendMessage(head.id, {
      type: "Damage",
      amount: 10_000,
    });
    await game.step({ frames: 5 });
    assert.equal(
      (await game.entities.byTemplate(SHODAN_HEAD)).length,
      0,
      "ordinary lethal Damage should destroy SHODAN's head",
    );

    await game.step({ frames: 240 });
    let info = await game.info();
    assert.equal(
      info.campaign_completed,
      false,
      "the ending must wait for the authored five-second DeathDelay",
    );
    assert.equal(info.mission, "shodan.mis");

    await game.step({ frames: 120 });
    info = await game.info();
    assert.equal(info.campaign_completed, true);
    assert.equal(
      info.mission,
      "enhanced/cs3.ogv",
      "the retail ending should become the active cutscene scene",
    );
  },
);
