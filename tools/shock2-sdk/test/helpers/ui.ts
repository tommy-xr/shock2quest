import type { GameServer } from "../../src/index.js";
import type { UiElement } from "../../src/types.js";

/** Click the center of one introspected flat-panel element through real input. */
export async function clickUiElement(
  game: GameServer,
  element: UiElement,
): Promise<void> {
  const [x, y, width, height] = element.screen_rect;
  await game.input.set("pointer.position", [
    x + width / 2,
    y + height / 2,
  ]);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 1);
  await game.step({ frames: 2 });
  await game.input.set("pointer.pressed", 0);
  await game.step({ frames: 2 });
}
