import assert from "node:assert/strict";
import { readFileSync, writeFileSync, rmSync } from "node:fs";
import { test } from "node:test";
import { GameServer } from "../src/index.js";
import { openGamePig } from "./helpers/gamepig.js";
import { savedGamePath } from "./helpers/save.js";

for (const vr of [false, true]) {
  test(`OverWorld movement, inspection, resume and one-time victory reward (${vr ? "VR" : "flat"})`,
    { skip: process.env.SHOCK2_E2E !== "1", timeout: 180_000 }, async () => {
      await using game = await GameServer.launch({ mission: "medsci1.mis", debugFlags: vr ? ["--vr"] : [] });
      const { click } = await openGamePig(game, vr, "Overworld Cart");
      const activate = async (label: string) => {
        const element = (await game.ui.state()).active_panel?.elements.find(e => e.label === label);
        assert.ok(element, `${label} must be available`);
        await click(game, element);
      };
      const face = async (button: "lower" | "upper") => {
        await game.input.trigger(button === "lower" ? "LeftHandLowerButton" : "LeftHandUpperButton");
        await game.step({ frames: 3 });
      };
      const name = `overworld_${vr}_${Date.now()}`;
      let path: string | undefined;
      const snapshot = async () => {
        await game.save(name);
        path = savedGamePath(name);
        return JSON.parse(readFileSync(path, "utf8"));
      };
      const reopen = async () => {
        const handheld = (await game.entities.list({ filter: "Gameboy" })).entities.find(e => e.template_id === -3864);
        assert.ok(handheld);
        await game.entities.sendMessage(handheld.id, { type: "Frob" });
        await game.step({ frames: 3 });
        await activate("game-4");
      };
      const loadFixture = async (save: unknown) => {
        writeFileSync(path!, JSON.stringify(save));
        await game.load(name);
        await game.step({ frames: 2 });
        await reopen();
      };
      try {
        await activate("game-4");
        assert.equal((await game.ui.state()).active_panel!.elements.filter(e => e.label?.startsWith("ow-tile-")).length, 187);
        const initial = (await snapshot()).global_data.quest_info.overworld.game;
        const directions = [-1, 0, 1].flatMap(x => [-1, 0, 1].filter(y => x || y).map(y => [x, y]));
        const direction = directions.find(([x, y]) => {
          const px = (initial.player.position.x + x + 64) % 64;
          const py = (initial.player.position.y + y + 64) % 64;
          return ["Plain", "Camp", "Forest", "Road", "Village", "City", "Shrine", "Temple"].includes(initial.map.tiles[py * 64 + px].terrain)
            && !initial.actors.some((a: any) => a?.position.x === px && a?.position.y === py);
        });
        assert.ok(direction, "a generated start has a free walkable neighbor");
        const [dx, dy] = direction;
        if (vr) {
          await game.input.set("left_hand.thumbstick", [dx, -dy]);
          await game.step({ frames: 2 });
          await game.input.set("left_hand.thumbstick", [0, 0]);
          await game.step({ frames: 2 });
        } else await activate(`ow-tile-${5 + dx}-${8 + dy}`);
        let player = (await snapshot()).global_data.quest_info.overworld.game.player;
        assert.deepEqual(player.position, { x: (initial.player.position.x + dx + 64) % 64, y: (initial.player.position.y + dy + 64) % 64 });
        assert.equal(player.turns, initial.player.turns + 1);
        if (vr) await face("lower"); else await activate("ow-wait");
        const waited = (await snapshot()).global_data.quest_info.overworld.game.player;
        assert.equal(waited.turns, player.turns + 1);
        await activate("ow-look");
        await activate("ow-tile-5-8");
        player = (await snapshot()).global_data.quest_info.overworld.game.player;
        assert.equal(player.turns, waited.turns, "inspection is free");
        await activate("ow-back");
        await activate("game-4");
        assert.deepEqual((await snapshot()).global_data.quest_info.overworld.game.player, player);
        await game.load(name);
        await game.step({ frames: 2 });
        await reopen();
        assert.deepEqual((await snapshot()).global_data.quest_info.overworld.game.player, player, "same-build load resumes the adventure");

        if (vr) {
          await face("upper");
          await game.input.set("left_hand.thumbstick", [1, 0]);
          await game.step({ frames: 2 });
          await game.input.set("left_hand.thumbstick", [0, 0]);
          await game.step({ frames: 2 });
          await face("lower");
          assert.equal((await snapshot()).global_data.quest_info.overworld.game.player.turns, player.turns + 1,
            "secondary focuses actions; stick and primary choose Heal");
        }
        const handheld = (await game.entities.list({ filter: "Gameboy" })).entities.find(e => e.template_id === -3864)!;
        assert.ok(!(await game.ui.state()).active_panel!.elements.some(e => e.label === "ow-cast"));
        await game.entities.sendMessage(handheld.id, { type: "SetObjectState", state: "Hacked" });
        await game.step({ frames: 2 });
        await activate("ow-cast");
        await activate("ow-tile-6-8");
        assert.ok((await snapshot()).global_data.quest_info.overworld.game.player.spell_cooldown > 0,
          "hacked device can launch a missile");
        assert.ok(!(await game.ui.state()).active_panel!.elements.some(e => e.label === "ow-cast"), "cooldown disables Cast");

        // Deterministic terminal fixtures verify the integration boundary, not a full adventure playthrough.
        let save = await snapshot();
        let quest = save.global_data.quest_info;
        let run = quest.overworld.game;
        const wallet = quest.player_stats.nanites;
        run.player.hp = 1;
        run.animations = [{ position: run.player.position, visual: "Fireball", direction: [1, 0], steps: 1, remaining_ms: 400 }];
        await loadFixture(save);
        await game.step({ frames: 185 });
        quest = (await snapshot()).global_data.quest_info;
        assert.equal(quest.overworld.game.phase, "Lost");
        assert.equal(quest.player_stats.nanites, wallet, "loss pays no nanites");
        await activate("ow-continue");
        await activate("ow-restart");
        save = await snapshot();
        quest = save.global_data.quest_info;
        run = quest.overworld.game;
        run.player.position = { x: 32, y: 32 };
        run.player.dragons = 4;
        run.player.hit = [0, 0, 100];
        run.player.damage = [0, 0, 100];
        run.animations = [];
        run.actors = Array(128).fill(null);
        run.actors[1] = { kind: "Dragon", position: { x: 33, y: 32 }, hp: 1, behavior: "Guard", home: null, element: "Road" };
        const scoreCount = quest.mini_game_scores["4"].length;
        await loadFixture(save);
        await activate("ow-tile-6-8");
        await game.step({ frames: 185 });
        quest = (await snapshot()).global_data.quest_info;
        assert.equal(quest.overworld.game.phase, "Won");
        assert.equal(quest.player_stats.nanites, wallet + 20);
        assert.equal(quest.mini_game_scores["4"].length, scoreCount + 1);
        await game.load(name);
        await game.step({ frames: 2 });
        await reopen();
        await game.step({ frames: 185 });
        await activate("ow-continue");
        assert.ok((await game.ui.state()).active_panel!.elements.some(e => e.text?.includes("High Score")));
        await activate("ow-restart");
        quest = (await snapshot()).global_data.quest_info;
        assert.equal(quest.player_stats.nanites, wallet + 20, "reopen/load/restart cannot repay victory");
        assert.equal(quest.mini_game_scores["4"].length, scoreCount + 1);
      } finally { if (path) rmSync(path, { force: true }); }
    });
}
