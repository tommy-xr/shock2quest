import type { GameServer } from "../../src/index.js";
import { clickUiElement } from "./ui.js";
import { clickWorldPanelElement } from "./vr-hand.js";

/** Install a cartridge and open the handheld using the presentation's real inputs. */
export async function openGamePig(game: GameServer, vr: boolean, cartridge: string) {
  await game.step({ frames: 2 });
  if (vr) await game.input.set("left_hand.squeeze", 1);
  const handheld = await game.player.spawnItem("Gameboy", vr ? { hand: "left" } : undefined);
  await game.step({ frames: 2 });
  const cart = await game.player.spawnItem(cartridge);
  await game.entities.sendMessage(cart.entity_id, { type: "Frob" });
  await game.entities.sendMessage(handheld.entity_id, { type: "Frob" });
  await game.step({ frames: 2 });
  const click: typeof clickUiElement = vr
    ? (g, element) => clickWorldPanelElement(g, [188, 296, 0], 1, element)
    : clickUiElement;
  return { handheld, click };
}
