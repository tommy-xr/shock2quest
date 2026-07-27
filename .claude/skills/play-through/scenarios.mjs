// Campaign randomization tables for the play-through skill: the mission-sequence
// scenarios and the bonus-objective / special-tweak playstyles a campaign can
// roll. Consumed by `playthrough-state.mjs roll` (which persists the pick in the
// ledger) — run directly only to browse the tables:
//
//   node scenarios.mjs list          print all scenarios and tweaks with ids

import { existsSync } from "node:fs";
import { join } from "node:path";

export const SCENARIOS = [
  {
    id: "earth-to-medsci",
    name: "Earth → Station → MedSci",
    missions: ["earth", "station", "medsci1", "medsci2"],
    goal: "Complete basic + advanced training on Earth, progress through the station (career choice), then play medsci1 and medsci2 to their exits.",
  },
  {
    id: "engineering",
    name: "Engineering",
    missions: ["eng1", "eng2"],
    goal: "Play eng1 and eng2; get power enabled to the elevator.",
  },
  {
    id: "hydroponics",
    name: "Hydroponics",
    missions: ["hydro2", "hydro1", "hydro3"],
    goal: "Start from the elevator (hydro2 is the elevator hub), get and research the Toxin-A vials, and apply them to the environmental regulators.",
  },
  {
    id: "operations",
    name: "Operations",
    missions: ["ops2", "ops1", "ops3", "ops4"],
    goal: "Start at ops2 (the elevator), go through the cutscene, then complete the simulation unit overrides.",
  },
  {
    id: "recreation",
    name: "Recreation",
    missions: ["rec1", "rec2", "rec3"],
    goal: "Identify the codes from the paintings and activate the transmitter.",
  },
  {
    id: "command",
    name: "Command",
    missions: ["command1", "command2"],
    goal: "Play through the command missions to their exits.",
  },
  {
    id: "rickenbacker",
    name: "Rickenbacker",
    missions: ["rick1", "rick2", "rick3"],
    goal: "Play through the Rickenbacker missions, destroying the eggs.",
  },
  {
    id: "shodan",
    name: "SHODAN finale",
    missions: ["shodan"],
    goal: "Verify the end-game sequence and the final boss battle, especially the SHODAN AI.",
  },
];

export const TWEAKS = [
  {
    id: "none",
    name: "None",
    instructions: "No special constraint — play as you wish.",
  },
  {
    id: "melee-only",
    name: "Melee only",
    instructions: "Use only melee weapons (wrench, laser rapier, crystal shard) for all combat; never fire a ranged weapon.",
  },
  {
    id: "loot-everything",
    name: "Loot everything",
    instructions: "Grab and loot everything possible: search every corpse and container, pick up every item, and report anything that can't be grabbed or looted.",
  },
  {
    id: "hack-everything",
    name: "Hack everything hackable",
    instructions: "Find and hack every hackable object in each mission, not just those on the shortest route; verify each successful hack changes the object's behavior as expected and report anything the game identifies as hackable that cannot be hacked.",
  },
  {
    id: "repair-everything",
    name: "Repair everything repairable",
    instructions: "Find and repair every repairable object in each mission, not just those on the shortest route; verify each repair restores the expected function and report anything the game identifies as repairable that cannot be repaired.",
  },
  {
    id: "buy-every-replicator",
    name: "Buy from every replicator",
    instructions: "Find every replicator in each mission and buy at least one item from each; verify the transaction consumes the expected nanites and dispenses the selected item, including hacked inventory where available.",
  },
  {
    id: "verify-os-upgraders",
    name: "Verify OS upgraders",
    instructions: "Find and use every OS Upgrade Unit in each mission; choose different upgrades where possible, verify each grants the selected upgrade and follows the expected one-use behavior, and report any missing or incorrect upgrade effect.",
  },
  {
    id: "verify-cutscenes",
    name: "Verify cutscenes",
    instructions: "Verify every cutscene / scripted sequence along the path: trigger each one, watch it play out, and report any that fail to start, hang, or misbehave.",
  },
  {
    id: "verify-research",
    name: "Verify research + chemicals",
    instructions: "Verify researching and chemicals work as expected: pick up researchable items, run research, fetch the required chemicals, and confirm each research completes with the right result.",
  },
  {
    id: "navy-tech",
    name: "Navy (hack/repair/modify)",
    instructions: "Play as Navy: hack, repair, or modify whenever possible — every keypad, security computer, replicator, broken weapon, and upgradable weapon along the path.",
  },
  {
    id: "marine-standard",
    name: "Marine (standard weapons)",
    instructions: "Play as Marine: acquire and fire every standard weapon available on the path (pistol, shotgun, assault rifle), verifying each fires, reloads, and damages enemies.",
  },
  {
    id: "marine-electronic",
    name: "Marine (electronic weapons)",
    instructions: "Play as Marine: acquire and fire every energy/electronic weapon available on the path (laser pistol, EMP rifle), verifying each fires, recharges, and damages appropriate enemies.",
  },
  {
    id: "marine-organic",
    name: "Marine (organic weapons)",
    instructions: "Play as Marine: acquire and fire every exotic/organic weapon available on the path (viral proliferator, worm launcher, crystal shard), verifying each works.",
  },
  {
    id: "marine-heavy",
    name: "Marine (heavy weapons)",
    instructions: "Play as Marine: acquire and fire every heavy weapon available on the path (grenade launcher, fusion cannon), verifying each fires and its projectiles/explosions behave.",
  },
  {
    id: "osa-tier1",
    name: "OSA (tier 1 psi)",
    instructions: "Play as OSA: use and verify every tier 1 psi power (e.g. kinetic redirection, psycho-reflective screen, neuro-reflex dampening, cryokinesis, remote electron tampering).",
  },
  {
    id: "osa-tier2",
    name: "OSA (tier 2 psi)",
    instructions: "Play as OSA: use and verify every tier 2 psi power (e.g. adrenaline overproduction, neural decontamination, cerebro-stimulated regeneration, psychogenic agility, recursive psionic amplification, localized pyrokinesis).",
  },
  {
    id: "osa-tier3",
    name: "OSA (tier 3 psi)",
    instructions: "Play as OSA: use and verify every tier 3 psi power (e.g. electron cascade, energy reflection, neural toxin-blocker, enhanced motion sensitivity, projected cryokinesis, psionic hypnogenesis).",
  },
  {
    id: "osa-tier4",
    name: "OSA (tier 4 psi)",
    instructions: "Play as OSA: use and verify every tier 4 psi power (e.g. photonic redirection, remote pattern detection, electron suppression, psychogenic cyber-affinity, imposed neural restructuring, cerebro-energetic extension).",
  },
  {
    id: "osa-tier5",
    name: "OSA (tier 5 psi)",
    instructions: "Play as OSA: use and verify every tier 5 psi power (e.g. instantaneous quantum relocation, imposed vacillation, metacreative barrier, external psionic detonation, psycho-reflective aura, soma transference).",
  },
  {
    id: "verify-regen",
    name: "Verify regeneration units",
    instructions: "Verify the quantum bio-reconstruction (regeneration) units on every deck in the sequence: activate each one, die, and confirm respawn at the unit works.",
  },
  {
    id: "verify-camera-alarms",
    name: "Verify camera alarms",
    instructions: "Verify security camera alarm triggers: let cameras spot you, confirm the alarm raises, and confirm it alerts enemies and draws them to you.",
  },
  {
    id: "ai-pathfinding",
    name: "AI / pathfinding stress",
    instructions: "Exercise the AI and pathfinding specially: kite enemies across rooms and floors, lure them through doors and around obstacles, and report any stuck, teleporting, or route-failing AI (verify with GET /v1/ai/paths).",
  },
  {
    id: "detailed",
    name: "Detailed test run",
    instructions: "Detailed test run: move slowly, interact with everything interactive, read every log/email, verify every objective update, and record smaller issues that a speed-run would skip.",
  },
];

// Asset set to launch with: exported as DARK_ASSET_PATH for every runtime the
// campaign starts. Paths are the two install locations used on our machines.
// A set is only eligible for the random draw when a data-root sentinel exists
// there — the same sentinel list as `shock2vr::paths::data_root()`: loose
// classic data (`shock2.gam`, ...) or a 25th Anniversary install
// (`sshock2.kpf`, supported since #557). A set with no sentinel (missing or
// broken install) can still be forced (assets=<id>), never rolled.
export const ASSET_SETS = [
  {
    id: "legacy",
    name: "Legacy assets",
    path: `${process.env.HOME}/ss2-data-unpacked`,
  },
  {
    id: "25th",
    name: "25th Anniversary assets",
    path: `${process.env.HOME}/ss2-25th`,
  },
];

const DATA_ROOT_SENTINELS = ["shock2.gam", "res/obj.crf", "res/mesh.crf", "motiondb.bin", "sshock2.kpf"];
export const assetSetUsable = (a) => DATA_ROOT_SENTINELS.some((f) => existsSync(join(a.path, f)));

// Deterministic PRNG (mulberry32) so a roll is reproducible from its seed.
function mulberry32(seed) {
  let a = seed >>> 0;
  return () => {
    a |= 0; a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Roll a campaign: random scenario + tweak + asset set, seeded. Any of the
// three can be forced by id. The RNG stream is drawn identically whether or
// not picks are forced, so `seed=N` alone always reproduces the same random
// draws regardless of which overrides were combined with it.
export function roll({ seed, scenarioId, tweakId, assetsId } = {}) {
  const s = seed ?? Math.floor(Math.random() * 2 ** 31);
  const rng = mulberry32(s);
  const byId = (table, id, label) => {
    const hit = table.find((e) => e.id === id);
    if (!hit) throw new Error(`unknown ${label} '${id}' (see: node scenarios.mjs list)`);
    return hit;
  };
  const draw = (table) => table[Math.floor(rng() * table.length)];
  const rolledScenario = draw(SCENARIOS);
  const rolledTweak = draw(TWEAKS);
  const usableSets = ASSET_SETS.filter(assetSetUsable);
  const rolledAssets = draw(usableSets.length ? usableSets : ASSET_SETS);
  const scenario = scenarioId ? byId(SCENARIOS, scenarioId, "scenario") : rolledScenario;
  const tweak = tweakId ? byId(TWEAKS, tweakId, "tweak") : rolledTweak;
  const assets = assetsId ? byId(ASSET_SETS, assetsId, "assets") : rolledAssets;
  const warnings = [];
  if (!assetSetUsable(assets))
    warnings.push(`asset set '${assets.id}' has no data-root sentinel (shock2.gam / sshock2.kpf / ...) at ${assets.path} — the engine cannot load it (missing install?)`);
  return { seed: s, scenario, tweak, assets, warnings };
}

if (process.argv[1]?.endsWith("scenarios.mjs")) {
  const [cmd] = process.argv.slice(2);
  if (cmd === "list") {
    console.log("Scenarios:");
    for (const s of SCENARIOS) console.log(`  ${s.id.padEnd(18)} ${s.missions.join(",").padEnd(28)} ${s.goal}`);
    console.log("Bonus objectives / tweaks:");
    for (const t of TWEAKS) console.log(`  ${t.id.padEnd(22)} ${t.instructions}`);
    console.log("Asset sets:");
    for (const a of ASSET_SETS) console.log(`  ${a.id.padEnd(22)} ${a.name} (DARK_ASSET_PATH=${a.path})${assetSetUsable(a) ? "" : " [NOT USABLE: no data-root sentinel — force-only, excluded from random draw]"}`);
  } else if (cmd) {
    console.error("usage: node scenarios.mjs list");
    process.exit(1);
  }
}
