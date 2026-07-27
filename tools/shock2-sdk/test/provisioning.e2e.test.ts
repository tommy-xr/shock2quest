import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import { clickUiElement } from "./helpers/ui.js";
import { fireOnce } from "./helpers/weapon.js";

// End-to-end test for the debug provisioning levers (POST /v1/player/spawn-item
// and POST /v1/player/stats) - what an automated playtest needs to establish a
// starting loadout for a class tweak (e.g. "Marine: acquire and fire pistol,
// shotgun, assault rifle") in a scenario that starts mid-game with a fresh
// character.
//
// Negative-first: before these endpoints existed there was no way to get a
// weapon the level does not contain, and no way at all to set skills - the
// spawn lever was one hardcoded template (Pistol) and /v1/control/command was a
// stub. Both calls below 404 on the base commit.
//
// Opt-in (compiles the runtime + needs Data/ assets):
//   npm run test:e2e        (or SHOCK2_E2E=1 node --test dist/test/)
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const SHOTGUN_TEMPLATE = -19;
const ASSAULT_RIFLE_TEMPLATE = -18;
const GRUNT_TEMPLATE = -397; // og-pipe grunt: a creature, not a pickup item

function ammoOf(detail: { properties: { name: string; value: string }[] }): number {
  const p = detail.properties.find((x) => x.name === "Ammo");
  assert.ok(p, "a provisioned gun should expose an Ammo property");
  return Number(p.value);
}

test(
  "provisioning: spawn weapons by template and train the skill that uses them",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    // command1.mis is a mid-game deck: the campaign scenario where the start
    // state is a fresh character with nothing.
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8348),
    });
    await game.step({ frames: 5 });

    // --- A fresh character: untrained, unarmed ---
    const before = (await game.info()).player.stats;
    assert.ok(before, "player should have a character sheet");
    assert.equal(before.skills.standard_weapons, 0, "fresh character is untrained");
    const carriedBefore = await game.player.inventory();
    assert.ok(
      !carriedBefore.items.some((i) => i.name === "Shotgun"),
      "command1 start state carries no shotgun",
    );

    // --- Provision the character sheet (a Marine loadout) ---
    const trained = await game.player.setStats({
      strength: 3,
      skills: { standard_weapons: 4, maintenance: 2 },
      cyber_modules: 20,
    });
    assert.equal(trained.skills.standard_weapons, 4);
    assert.equal(trained.skills.maintenance, 2);
    assert.equal(trained.strength, 3);
    assert.equal(trained.cyber_modules, 20);
    // Untouched fields keep their values.
    assert.equal(trained.skills.hack, before.skills.hack, "omitted skills are untouched");
    assert.equal(trained.endurance, before.endurance, "omitted stats are untouched");
    // The write is visible through the read side (same PlayerStats).
    const liveStats = (await game.info()).player.stats;
    assert.deepEqual(liveStats, trained, "/v1/info should report the provisioned sheet");

    // --- Provision the weapons: by name and by stable template id ---
    const shotgun = await game.player.spawnItem("Shotgun");
    assert.equal(shotgun.template_id, SHOTGUN_TEMPLATE, "name resolves to the stable template");
    assert.equal(shotgun.name, "Shotgun");
    const rifle = await game.player.spawnItem(ASSAULT_RIFLE_TEMPLATE);
    assert.equal(rifle.name, "Assault Rifle");

    const carried = await game.player.inventory();
    for (const item of [shotgun, rifle]) {
      const row = carried.items.find((i) => i.entity_id === item.entity_id);
      assert.ok(row, `${item.name} should be carried (got ${JSON.stringify(carried.items)})`);
      assert.equal(row.location, "inventory", "a provisioned item lands in the backpack");
    }
    // A backpack item is out of the physical world, like any real pickup.
    assert.equal(
      (await game.physics.bodies({ entityId: shotgun.entity_id })).bodies.length,
      0,
      "a provisioned item must not keep a world physics body",
    );

    // --- Wield the provisioned shotgun through the real flat UI (Tab into use
    // mode, double-click the strip item), exactly as a player would ---
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const stripShotgun = (await game.ui.state()).strip?.elements.find(
      (e) => e.kind === "button" && e.label === "Shotgun",
    );
    assert.ok(stripShotgun, "the provisioned shotgun should appear in the inventory strip");
    await clickUiElement(game, stripShotgun); // lift...
    await clickUiElement(game, stripShotgun); // ...double-click wields
    await game.input.trigger("ToggleUseMode"); // back to shooter mode
    await game.step({ frames: 5 });
    assert.equal(
      (await game.info()).player.wielded_entity_id,
      shotgun.entity_id,
      "a provisioned weapon should be wieldable",
    );

    // --- Fire it: a provisioned weapon is a real weapon (ammo decrements) ---
    const startAmmo = ammoOf(await game.entities.detail(shotgun.entity_id));
    assert.ok(startAmmo > 0, `the shotgun should spawn loaded (got ${startAmmo})`);
    await fireOnce(game);
    assert.equal(
      ammoOf(await game.entities.detail(shotgun.entity_id)),
      startAmmo - 1,
      "firing a provisioned shotgun consumes a round",
    );

    // --- Refusals ---
    await assert.rejects(
      game.player.spawnItem("Definitely Not A Template"),
      /status 400|no template named/,
      "an unknown template name is rejected",
    );
    // Count by name, not by a truncated listing: `limit` truncates server-side,
    // so a total-count comparison would pass no matter what leaked.
    const gruntCount = async () =>
      (await game.entities.list({ filter: "*OG-Pipe*", limit: 500 })).entities.length;
    const gruntsBefore = await gruntCount();
    await assert.rejects(
      game.player.spawnItem(GRUNT_TEMPLATE),
      /status 400|not a pickup/,
      "a creature template is not provisionable",
    );
    assert.equal(
      await gruntCount(),
      gruntsBefore,
      "a refused spawn must leave nothing behind in the world",
    );

    // Mission objects (positive ids) are refused: duplicating one would hand
    // the player a second copy of a unique quest item.
    const missionObject = (await game.entities.list({ limit: 50 })).entities.find(
      (e) => e.template_id > 0,
    );
    assert.ok(missionObject, "expected a mission object in command1");
    await assert.rejects(
      game.player.spawnItem(missionObject.template_id),
      /status 400|mission object/,
      "provisioning takes gamesys templates, not mission objects",
    );

    // A misspelled field is a 400, not a silent no-op that would make the
    // playtest think the character was provisioned when it wasn't.
    await assert.rejects(
      // @ts-expect-error - deliberately misspelled field
      game.player.setStats({ stength: 5 }),
      /status 400|unknown field/,
      "an unknown stat field is rejected",
    );

    await assert.rejects(
      game.player.setStats({ skills: { standard_weapons: 1 } }),
      /status 400|only raises/,
      "provisioning must not lower a trained skill",
    );
    await assert.rejects(
      game.player.setStats({ strength: 9 }),
      /status 400|maxes out/,
      "provisioning must respect the stat cap",
    );
    // A rejected request applies nothing: the sheet is exactly as provisioned.
    assert.deepEqual(
      (await game.info()).player.stats,
      trained,
      "refused provisioning must not partially apply",
    );

    // The runtime stays live and the loadout intact after the refusals.
    assert.equal(
      ammoOf(await game.entities.detail(shotgun.entity_id)),
      startAmmo - 1,
      "the wielded shotgun survives the refused requests",
    );
  },
);
