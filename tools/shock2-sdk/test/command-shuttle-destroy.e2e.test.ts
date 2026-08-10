import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary } from "../src/types.js";
import { clickUiElement } from "./helpers/ui.js";
import { fireOnce } from "./helpers/weapon.js";

// Command's Shuttle B is an authored destroyable prop with TriggerDestroy and
// nine SwitchLinks, but deliberately no HitPoints. The original simple-health
// path treats that absence as a one-hit Slay. Before #868, however, the entity
// creator only installed that path on props which *did* author HitPoints, so a
// real weapon impact had no script owner and the objective chain never fired.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const SHIELD_DESTROY_TRAP = 280;
const SHUTTLE_SHIELD = 278;
const SHUTTLE_B = 2392;
const EXPLOSION_TWEQS = [157, 460, 482, 632, 500, 524, 516, 499, 521] as const;
const DELAYED_SLAY_TARGET = 808;
const PISTOL_TEMPLATE = -17;
const SMALL_AP_CLIP_TEMPLATE = -1360;

async function only(game: GameServer, objectId: number): Promise<EntitySummary> {
  const found = await game.entities.byTemplate(objectId);
  assert.equal(
    found.length,
    1,
    `expected exactly one command1 object ${objectId}, got ${JSON.stringify(found)}`,
  );
  return found[0];
}

function ammoOf(detail: EntityDetailResult): number {
  const ammo = detail.properties.find((property) => property.name === "Ammo");
  assert.ok(ammo, "the provisioned pistol must expose its live magazine count");
  return Number(ammo.value);
}

test(
  "command1: one visible AP hit destroys no-HP Shuttle B and fires its objective chain",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    const saveName = `command_shuttle_b_destroyed_${Date.now()}`;
    await using game = await GameServer.launch({
      mission: "command1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8618),
    });
    await game.step({ frames: 5 });

    const shuttle = await only(game, SHUTTLE_B);
    const shuttleDetail = await game.entities.detail(shuttle.id);
    const switchLinks = shuttleDetail.outgoing_links.filter(
      (link) => link.link_type === "SwitchLink",
    );
    assert.equal(switchLinks.length, 9, "Shuttle B must retain all nine authored branches");
    assert.ok(
      shuttleDetail.properties.some(
        (property) => property.name === "Scripts" && property.value.includes("TriggerDestroy"),
      ),
      "mission object 2392 must exercise the authored TriggerDestroy path",
    );
    assert.ok(
      !shuttleDetail.properties.some((property) => property.name === "HitPoints"),
      "the regression requires Shuttle B's deliberate no-HP fallback",
    );

    assert.equal(await game.quests.get("ShuttleBBoom"), "unknown");
    assert.equal(await game.quests.get("HackEXP"), "unknown");
    assert.equal(await game.quests.get("Note_6_6"), "unknown");
    assert.equal(await game.quests.get("Note_6_7"), "unknown");
    const modulesBefore = (await game.info()).player.stats?.cyber_modules;
    assert.equal(modulesBefore, 0, "fresh Command character starts with no cyber modules");

    // Isolate the already-completed objective precondition by firing Command's
    // authored DestroyTrap280. It removes the real Shield278 entity; the weapon
    // shot below is therefore genuinely unshielded rather than bypassing a
    // collision volume or mutating Shuttle2392 itself.
    const shieldDestroyTrap = await only(game, SHIELD_DESTROY_TRAP);
    await game.entities.sendMessage(shieldDestroyTrap.id, { type: "TurnOn" });
    await game.step({ frames: 30 });
    assert.equal(
      (await game.entities.byTemplate(SHUTTLE_SHIELD)).length,
      0,
      "authored DestroyTrap280 setup must remove Shuttle Shield278",
    );

    // The QB filter is conditional on the player being in the objective room.
    // The later delayed filter likewise expects Shuttle A's preceding objective
    // to be done. Establish only those authored campaign preconditions through
    // the debug setup API; every action under test below is the production
    // weapon/impact/script path.
    await game.quests.set("InRoom", "complete");
    await game.quests.set("ShuttleABoom", "incomplete");

    const pistol = await game.player.spawnItem(PISTOL_TEMPLATE);
    await game.player.setStats({ skills: { standard_weapons: 6 } });
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    const stripPistol = (await game.ui.state()).strip?.elements.find(
      (element) => element.kind === "button" && element.label === "Pistol",
    );
    assert.ok(stripPistol, "the provisioned pistol must appear in the inventory strip");
    await clickUiElement(game, stripPistol);
    await clickUiElement(game, stripPistol);
    await game.input.trigger("ToggleUseMode");
    await game.step({ frames: 5 });
    assert.equal((await game.info()).player.wielded_entity_id, pistol.entity_id);

    // A freshly provisioned pistol carries standard rounds and faithfully
    // refuses to transmute a loaded magazine. Empty it away from the objective,
    // provision a real AP clip, then select and reload AP through normal inputs.
    const standardAmmo = ammoOf(await game.entities.detail(pistol.entity_id));
    const drainPosition = await game.player.position();
    await game.input.lookAtWorldPoint([
      drainPosition.x,
      drainPosition.y + 1.04,
      drainPosition.z - 100,
    ]);
    await game.step({ frames: 1 });
    for (let round = 0; round < standardAmmo; round += 1) {
      await fireOnce(game);
    }
    assert.equal(ammoOf(await game.entities.detail(pistol.entity_id)), 0);
    assert.equal(
      (await game.entities.byTemplate(SHUTTLE_B)).length,
      1,
      "standard-magazine setup must fire away from Shuttle B",
    );
    await game.player.spawnItem(SMALL_AP_CLIP_TEMPLATE);
    await game.input.trigger("CycleAmmo");
    await game.step({ frames: 2 });
    assert.equal((await game.info()).player.wielded_ammo_type, "ap");
    await game.input.trigger("Reload");
    await game.step({ frames: 130 });
    const ammoBefore = ammoOf(await game.entities.detail(pistol.entity_id));
    assert.ok(ammoBefore > 0, "the provisioned pistol must start loaded");

    // Teleport only stages the shooter on the supported bay floor. Acquisition
    // is visibility-checked and the hit itself comes from one real trigger edge.
    // This is the supported side-on firing lane from the accepted iteration27
    // save; it keeps the pistol muzzle clear of the bay-floor lip as well as
    // giving the camera ray an unobstructed shuttle surface.
    await game.player.teleport({ x: -286.4, y: -7.156, z: 91.0 });
    await game.step({ frames: 10 });
    const aim = await game.player.aimAt(shuttle, {
      hitbox: "center",
      visibility: "required",
    });
    assert.equal(aim.entity_id, shuttle.id, JSON.stringify(aim));
    assert.equal(aim.visibility.state, "visible", JSON.stringify(aim));
    // aimAt's ordinary-object acquisition deliberately picks the nearest
    // selectable surface. Refine the production camera/weapon to the authored
    // center after that visibility gate so the projectile ray continues
    // through the hull instead of grazing a close surface edge.
    await game.input.lookAtWorldPoint(shuttleDetail.position);
    await game.step({ frames: 1 });
    await fireOnce(game);
    assert.equal(
      ammoOf(await game.entities.detail(pistol.entity_id)),
      ammoBefore - 1,
      "the regression must consume exactly one AP round",
    );
    await game.step({ frames: 180 });

    assert.equal(
      (await game.entities.byTemplate(SHUTTLE_B)).length,
      0,
      "one unshielded AP impact must remove Shuttle B",
    );
    assert.equal(await game.quests.get("ShuttleBBoom"), "incomplete");
    assert.equal(await game.quests.get("HackEXP"), "incomplete");
    const note66After = await game.quests.get("Note_6_6");
    assert.notEqual(
      note66After,
      "unknown",
      "the InRoom QB-filter branch must grant the authored shuttle email objective",
    );
    assert.equal(
      await game.quests.get("Note_6_7"),
      "complete",
      "the two-second delay branch must reach its ShuttleABoom filter",
    );
    assert.equal(
      (await game.info()).player.stats?.cyber_modules,
      modulesBefore + 20,
      "the InRoom QB-filter branch must award the authored 20 modules",
    );
    for (const objectId of EXPLOSION_TWEQS) {
      assert.equal(
        (await game.entities.byTemplate(objectId)).length,
        0,
        `direct or delayed explosion Tweq${objectId} must receive the shuttle TurnOn`,
      );
    }
    assert.equal(
      (await game.entities.byTemplate(DELAYED_SLAY_TARGET)).length,
      0,
      "the half-second delay branch must relay through TrapSlayer631 to Window808",
    );
    assert.equal(
      (await game.entities.byTemplate(394)).length,
      1,
      "the same QB-filter fanout must preserve its Replicator394 target",
    );

    assert.equal((await game.save(saveName)).success, true);
    assert.equal((await game.load(saveName)).success, true);
    await game.step({ frames: 5 });
    assert.equal((await game.entities.byTemplate(SHUTTLE_B)).length, 0);
    assert.equal(await game.quests.get("ShuttleBBoom"), "incomplete");
    assert.equal(await game.quests.get("HackEXP"), "incomplete");
    assert.equal(await game.quests.get("Note_6_6"), note66After);
    assert.equal(await game.quests.get("Note_6_7"), "complete");
    assert.equal((await game.info()).player.stats?.cyber_modules, modulesBefore + 20);
  },
);
