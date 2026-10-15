/** Supported gameplay data opt-ins; remaster texture/art layers are independent. */
export type GameMod = "scp";

/** Iterate supported configurations in tests without inventing arbitrary mod combinations. */
export const supportedModConfigurations: readonly { name: "base" | "scp"; mods: GameMod[] }[] = [
  { name: "base", mods: [] },
  { name: "scp", mods: ["scp"] },
];
