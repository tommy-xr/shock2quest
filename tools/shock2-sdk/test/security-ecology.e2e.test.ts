import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";
import type { EntityDetailResult, EntitySummary } from "../src/index.js";

const e2eEnabled = process.env.SHOCK2_E2E === "1";
const basePort = Number(process.env.SHOCK2_E2E_PORT ?? 8482);

// One complete authored medsci1 security ecology:
//
//   Camera 68 --SwitchLink--> Ecology 71 --SwitchLink--> Generator 73
//        ^                          |
//        +---------SwitchLink------+
//   Generator 73 --SpawnPoint--> Marker 147
//
// These are stable mission object/template ids. Runtime entity ids are
// deliberately discovered anew on every launch.
const CAMERA = 68;
const ECOLOGY = 71;
const GENERATOR = 73;
const SPAWN_MARKER = 147;

function property(detail: EntityDetailResult, name: string): string | undefined {
  return detail.properties.find((candidate) => candidate.name === name)?.value;
}

async function exactlyOne(
  game: GameServer,
  templateId: number,
  label: string,
): Promise<EntitySummary> {
  const matches = await game.entities.byTemplate(templateId);
  assert.equal(
    matches.length,
    1,
    `${label}: expected stable mission object ${templateId}, got ${JSON.stringify(matches)}`,
  );
  return matches[0]!;
}

async function pipeOrganisms(game: GameServer): Promise<EntitySummary[]> {
  return (await game.entities.list({ filter: "OG-Pipe" })).entities.filter(
    (entity) => entity.name.toLocaleLowerCase() === "og-pipe",
  );
}

test(
  "medsci security camera drives alert ecology, pursuit, and recovery",
  { skip: !e2eEnabled, timeout: 900_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: basePort,
    });
    await game.step({ frames: 10 });

    const camera = await exactlyOne(game, CAMERA, "security camera");
    const ecology = await exactlyOne(game, ECOLOGY, "security ecology");
    const generator = await exactlyOne(game, GENERATOR, "monster generator");
    const marker = await exactlyOne(game, SPAWN_MARKER, "spawn marker");
    assert.ok(
      (await game.entities.detail(camera.id)).outgoing_links.some(
        (link) => link.link_type === "SwitchLink" && link.target_id === ecology.id,
      ),
      "the test must exercise the camera's authored SwitchLink",
    );
    assert.ok(
      (await game.entities.detail(ecology.id)).outgoing_links.some(
        (link) => link.link_type === "SwitchLink" && link.target_id === generator.id,
      ),
      "the test must exercise the ecology's authored generator",
    );
    assert.ok(
      (await game.entities.detail(generator.id)).outgoing_links.some(
        (link) => link.link_type === "SpawnPoint" && link.target_id === marker.id,
      ),
      "the test must exercise the generator's authored spawn marker",
    );
    assert.equal(
      property(await game.entities.detail(ecology.id), "EcologyState"),
      "Normal",
    );
    const organismsBefore = new Set((await pipeOrganisms(game)).map((entity) => entity.id));

    // Enter camera 68's real scan cone. CameraAI escalates through production
    // perception; no script message or debug alert action raises the alarm.
    // This is the open floor directly under camera 68's sweep. Its line of
    // sight was verified against the runtime world raycast; the player settles
    // on the authored floor at y=-0.6.
    await game.player.teleport({ x: 22.23, y: -1.0, z: -74.03 });

    let cameraDetail = await game.entities.detail(camera.id);
    let ecologyDetail = await game.entities.detail(ecology.id);
    for (
      let attempt = 0;
      attempt < 20 &&
      (property(cameraDetail, "AIAlertness") !== "High" ||
        property(ecologyDetail, "EcologyState") !== "Alert");
      attempt += 1
    ) {
      await game.step({ frames: 120 });
      cameraDetail = await game.entities.detail(camera.id);
      ecologyDetail = await game.entities.detail(ecology.id);
    }
    assert.equal(
      property(cameraDetail, "AIAlertness"),
      "High",
      "the real camera should identify the player",
    );
    assert.equal(
      property(ecologyDetail, "EcologyState"),
      "Alert",
      "CameraAlert should raise its linked ecology",
    );

    // The alarm was raised at most one 120-frame poll before it was observed,
    // so `framesSinceAlert + 120` bounds the frames elapsed since the alarm.
    // The authored recovery is 120 seconds (7200 fixed-timestep frames).
    let framesSinceAlert = 0;
    const step = async (frames: number) => {
      await game.step({ frames });
      framesSinceAlert += frames;
    };

    let spawned: EntitySummary | undefined;
    for (let poll = 0; poll < 4 && !spawned; poll += 1) {
      await step(901);
      spawned = (await pipeOrganisms(game)).find(
        (organism) => !organismsBefore.has(organism.id),
      );
    }
    assert.ok(spawned, "the alert population should spawn its authored OG-Pipe");
    await step(2);

    let spawnedDetail = await game.entities.detail(spawned.id);
    assert.equal(
      property(spawnedDetail, "AIAlertness"),
      "High",
      "GotoAlarm should alert the new ecology child immediately",
    );
    assert.ok(
      ["Chase", "MeleeAttack", "RangedAttack"].includes(
        property(spawnedDetail, "AIBehavior") ?? "",
      ),
      `the alarm-spawned child should pursue, got ${property(spawnedDetail, "AIBehavior")}`,
    );

    // Put the player in the newly-created monster's open spawn area. The
    // pinned GotoAlarm target follows this live position through normal AI.
    const [mx, my, mz] = spawnedDetail.position;
    await game.player.teleport({ x: mx + 3.0, y: my + 0.5, z: mz });
    let attacked = false;
    for (let second = 0; second < 20 && !attacked; second += 1) {
      await step(60);
      spawnedDetail = await game.entities.detail(spawned.id);
      attacked ||= property(spawnedDetail, "AIBehavior")?.endsWith("Attack") ?? false;
    }
    assert.ok(attacked, "the alarm-spawned organism should engage the player");

    // Leave the camera's visibility, then bracket the authored recovery
    // deadline: at 110s since the alert was observed at most 112s have
    // elapsed since the alarm (still inside the 120s window), and at 125s at
    // least 125s have (past it, with margin for the expiry-frame Reset).
    await game.player.teleport({ x: 300, y: 0, z: 300 });
    await step(110 * 60 - framesSinceAlert);
    assert.equal(
      property(await game.entities.detail(ecology.id), "EcologyState"),
      "Alert",
      "the ecology must remain alerted until recovery expires",
    );
    await step(15 * 60);
    assert.equal(
      property(await game.entities.detail(ecology.id), "EcologyState"),
      "Normal",
      "authored recovery should restore the normal ecology profile",
    );
    assert.equal(
      property(await game.entities.detail(camera.id), "AIAlertness"),
      "Lowest",
      "recovery Reset should clear the linked security camera",
    );
  },
);
