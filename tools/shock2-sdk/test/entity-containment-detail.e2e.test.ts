import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// End-to-end regression for #776: the entity-detail endpoint used to expose
// only outgoing links and omitted HasRefs, so contained loot with no physics
// body was indistinguishable from a broken world-placed object.
const e2eEnabled = process.env.SHOCK2_E2E === "1";

const CORPSE_PSI_AMP = 219; // Male Corpse 1 -> Contains -> Psi Amp
const CRYO_CARD = 1050; // world-placed control item (no incoming Contains)

test(
  "entity detail identifies contained and world-placed items",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "medsci1.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8483),
    });
    await game.step({ frames: 5 });

    const [corpse] = await game.entities.byTemplate(CORPSE_PSI_AMP);
    assert.ok(corpse, "expected the corpse containing the Psi Amp");
    const corpseDetail = await game.entities.detail(corpse.id);
    const [contains] = corpseDetail.outgoing_links.filter((link) =>
      link.link_type.startsWith("Contains"),
    );
    assert.ok(contains, "expected the corpse's authored Contains link");

    const ampDetail = await game.entities.detail(contains.target_id);
    assert.equal(
      ampDetail.has_refs,
      false,
      "contained loot should report that it has no world references",
    );
    assert.deepEqual(
      ampDetail.incoming_links.filter((link) => link.link_type.startsWith("Contains")),
      [
        {
          link_type: contains.link_type,
          target_id: corpse.id,
          target_name: corpseDetail.name,
        },
      ],
      "contained loot should identify its source container",
    );

    const [cryoCard] = await game.entities.byTemplate(CRYO_CARD);
    assert.ok(cryoCard, "expected the world-placed Cryo Card control");
    const cryoCardDetail = await game.entities.detail(cryoCard.id);
    assert.equal(
      cryoCardDetail.has_refs,
      true,
      "an entity without P$HasRefs should report the faithful true default",
    );
    assert.equal(
      cryoCardDetail.incoming_links.filter((link) => link.link_type.startsWith("Contains"))
        .length,
      0,
      "the world-placed control should have no incoming Contains links",
    );
  },
);
