import assert from "node:assert/strict";
import { test } from "node:test";

import { GameServer } from "../src/index.js";

// Hydro2's authored corpse/card pair from #809:
//   corpse 1297 --Contains--> Hydro Card A 934
// Runtime ids vary per launch, so both endpoints are resolved by their stable
// mission template ids before checking the live bidirectional API response.
const HYDRO_CARD_A_CORPSE = 1297;
const HYDRO_CARD_A = 934;

const e2eEnabled = process.env.SHOCK2_E2E === "1";

test(
  "entity detail reports both endpoints of authored containment",
  { skip: !e2eEnabled, timeout: 600_000 },
  async () => {
    await using game = await GameServer.launch({
      mission: "hydro2.mis",
      port: Number(process.env.SHOCK2_E2E_PORT ?? 8196),
    });
    await game.step({ frames: 5 });

    const corpses = await game.entities.byTemplate(HYDRO_CARD_A_CORPSE);
    const cards = await game.entities.byTemplate(HYDRO_CARD_A);
    assert.equal(corpses.length, 1, "expected Hydro2 corpse 1297");
    assert.equal(cards.length, 1, "expected Hydro Card A 934");
    const [corpse] = corpses;
    const [card] = cards;

    const corpseDetail = await game.entities.detail(corpse.id);
    const outgoingContains = corpseDetail.outgoing_links.filter(
      (link) => link.link_type.startsWith("Contains") && link.target_id === card.id,
    );
    assert.equal(outgoingContains.length, 1, "corpse must expose its Contains link to Hydro Card A");
    assert.equal(outgoingContains[0].target_id, card.id);
    assert.equal(outgoingContains[0].target_name, card.name);

    const cardDetail = await game.entities.detail(card.id);
    const incomingContains = cardDetail.incoming_links.filter((link) =>
      link.link_type.startsWith("Contains"),
    );
    assert.equal(
      incomingContains.length,
      1,
      "contained card must expose the corpse's incoming Contains link",
    );
    assert.equal(incomingContains[0].target_id, corpse.id);
    assert.equal(incomingContains[0].target_name, corpse.name);
    assert.equal(incomingContains[0].contains_ordinal, outgoingContains[0].contains_ordinal);
    assert.equal(cardDetail.contained_by, corpse.id);
  },
);
