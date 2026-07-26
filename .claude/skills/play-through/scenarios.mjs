// Campaign randomization tables for the play-through skill: the mission-sequence
// scenarios and the special-tweak playstyles a campaign can roll. Consumed by
// `playthrough-state.mjs roll` (which persists the pick in the ledger) — run
// directly only to browse the tables:
//
//   node scenarios.mjs list          print all scenarios and tweaks with ids

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
    missions: ["hydro1", "hydro2", "hydro3"],
    goal: "Start from the elevator, get and research the Toxin-A vials, and apply them to the environmental regulators.",
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
// three can be forced by id.
export function roll({ seed, scenarioId, tweakId, assetsId } = {}) {
  const s = seed ?? Math.floor(Math.random() * 2 ** 31);
  const rng = mulberry32(s);
  const byId = (table, id, label) => {
    const hit = table.find((e) => e.id === id);
    if (!hit) throw new Error(`unknown ${label} '${id}' (see: node scenarios.mjs list)`);
    return hit;
  };
  const scenario = scenarioId
    ? byId(SCENARIOS, scenarioId, "scenario")
    : SCENARIOS[Math.floor(rng() * SCENARIOS.length)];
  const tweak = tweakId
    ? byId(TWEAKS, tweakId, "tweak")
    : TWEAKS[Math.floor(rng() * TWEAKS.length)];
  const assets = assetsId
    ? byId(ASSET_SETS, assetsId, "assets")
    : ASSET_SETS[Math.floor(rng() * ASSET_SETS.length)];
  return { seed: s, scenario, tweak, assets };
}

if (process.argv[1]?.endsWith("scenarios.mjs")) {
  const [cmd] = process.argv.slice(2);
  if (cmd === "list") {
    console.log("Scenarios:");
    for (const s of SCENARIOS) console.log(`  ${s.id.padEnd(18)} ${s.missions.join(",").padEnd(28)} ${s.goal}`);
    console.log("Tweaks:");
    for (const t of TWEAKS) console.log(`  ${t.id.padEnd(22)} ${t.instructions}`);
    console.log("Asset sets:");
    for (const a of ASSET_SETS) console.log(`  ${a.id.padEnd(22)} ${a.name} (DARK_ASSET_PATH=${a.path})`);
  } else if (cmd) {
    console.error("usage: node scenarios.mjs list");
    process.exit(1);
  }
}
