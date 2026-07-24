import type { GameServer } from "../../src/index.js";

type Quat = [number, number, number, number];

function multiplyQuat(a: Quat, b: Quat): Quat {
  const [ax, ay, az, aw] = a;
  const [bx, by, bz, bw] = b;
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
}

function lookQuat(direction: [number, number, number]): Quat {
  const length = Math.hypot(...direction);
  const [bx, by, bz] = direction.map((value) => value / length) as [
    number,
    number,
    number,
  ];
  const dot = -bz;
  if (dot < -0.999999) {
    return [0, 1, 0, 0];
  }
  const quaternion: Quat = [by, -bx, 0, 1 + dot];
  const quaternionLength = Math.hypot(...quaternion);
  return quaternion.map((value) => value / quaternionLength) as Quat;
}

/**
 * Aim the flat runtime's world-space view at a point while respecting the
 * authored player pawn rotation.
 */
export async function aimAtWorldPoint(
  game: GameServer,
  target: [number, number, number],
): Promise<void> {
  const player = await game.player.position();
  const pawn = (await game.info()).player.rotation as Quat;
  const worldLook = lookQuat([
    target[0] - player.x,
    target[1] - (player.y + 1.6),
    target[2] - player.z,
  ]);
  const inversePawn: Quat = [-pawn[0], -pawn[1], -pawn[2], pawn[3]];
  await game.input.set("head.rotation", multiplyQuat(inversePawn, worldLook));
}
