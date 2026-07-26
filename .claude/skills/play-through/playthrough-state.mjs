// Ledger for the autonomous play-through loop (--auto). Persists the state that
// makes repeated play-through skill invocations resumable across iterations:
// the frontier (furthest state, as a game save), the blocker->fix ledger, and
// the stacked-fixes branch. Atomic read-modify-write so a loop agent never
// corrupts it by hand-editing JSON.
//
//   node playthrough-state.mjs <cmd> [args]
//     init [order=earth,station,medsci1,eng1,...] [fixBranch=playthrough-fixes]
//     roll [seed=N] [scenario=<id>] [tweak=<id>] [assets=<id>] [fixBranch=...] [--force]
//                                       init a RANDOMIZED campaign: pick a mission-
//                                       sequence scenario, a special tweak, and an
//                                       asset set (see scenarios.mjs) and persist
//                                       them in the ledger. Idempotent like init —
//                                       an existing campaign keeps its roll;
//                                       --force re-rolls from scratch.
//     show                              print the ledger + the recommended next action
//     advance <level> <save> <x,y,z> [note...]   set frontier, bump iteration, log history
//     blocker add <level> <bug|feature-gap> <issue#> <desc...>   record a found blocker (status:open)
//     blocker set <issue#> <status> [pr#]        update a blocker (status: open|reworking|merged|failed)
//
// State file: $PT_STATE, else /tmp/claude/playthrough/state.json.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { roll } from "./scenarios.mjs";

const FILE = process.env.PT_STATE || "/tmp/claude/playthrough/state.json";
const load = () => (existsSync(FILE) ? JSON.parse(readFileSync(FILE, "utf8")) : null);
const save = (s) => { mkdirSync(dirname(FILE), { recursive: true }); writeFileSync(FILE, JSON.stringify(s, null, 2) + "\n"); };
const baseState = (order, fixBranch) => ({ iteration: 0, mission_order: order, fix_branch: fixBranch, frontier: null, blockers: [], history: [] });

const [cmd, ...args] = process.argv.slice(2);
const rest = args; // command-specific positional args (after the command)

function nextAction(s) {
  const openBlockers = s.blockers.filter((b) => !["merged"].includes(b.status));
  const failed = s.blockers.find((b) => b.status === "failed");
  const campaign = (s.assets ? ` Launch every runtime with DARK_ASSET_PATH=${s.assets.path} (${s.assets.name}).` : "")
    + (s.scenario ? ` Campaign goal: ${s.scenario.goal}` : "")
    + (s.tweak && s.tweak.id !== "none" ? ` TWEAK [${s.tweak.name}]: ${s.tweak.instructions}` : "");
  if (failed) return `PAUSE — blocker #${failed.issue} (${failed.desc}) fix did not clear it; needs a human.`;
  if (!s.frontier) return `Iteration ${s.iteration}: launch fresh at "${s.mission_order[0]}" and playtest.` + campaign;
  return `Iteration ${s.iteration}: rebuild the '${s.fix_branch}' branch, launch a runtime, POST /v1/load {file:"${s.frontier.save}"} (resumes ${s.frontier.level}), and playtest onward.`
    + (openBlockers.length ? ` Open fix PRs to merge: ${openBlockers.map((b) => `#${b.pr ?? "?"}(${b.status})`).join(", ")}.` : "")
    + campaign;
}

switch (cmd) {
  case "init": {
    // Idempotent: safe to call at the start of every --auto iteration. Only
    // creates the ledger if absent; `--force` resets an existing campaign.
    if (existsSync(FILE) && !args.includes("--force")) {
      console.log(`ledger already exists at ${FILE} (iteration ${load().iteration}) — kept. Use --force to reset.`);
      break;
    }
    const order = (rest.find((a) => a.startsWith("order="))?.slice(6) || "earth,station,medsci1,medsci2,eng1,eng2,hydro1,hydro2,hydro3,ops1,ops2,ops3,ops4,rec1,rec2,rec3,command1,command2,rick1,rick2,rick3,many,shodan").split(",");
    const fixBranch = rest.find((a) => a.startsWith("fixBranch="))?.slice(10) || "playthrough-fixes";
    save(baseState(order, fixBranch));
    console.log(`initialized ${FILE} (${order.length} missions, fix branch '${fixBranch}')`);
    break;
  }
  case "roll": {
    // Randomized init: pick scenario + tweak + asset set and persist them so
    // every later iteration of the campaign plays under the same roll.
    // Idempotent like init — only rolls when no ledger exists (--force resets).
    if (existsSync(FILE) && !args.includes("--force")) {
      const s = load();
      if (s.scenario) {
        console.log(`ledger already exists at ${FILE} (iteration ${s.iteration}) — kept its roll. Use --force to re-roll.`);
        console.log(`scenario: ${s.scenario.name} · tweak: ${s.tweak?.name} · assets: ${s.assets?.name}`);
      } else {
        console.log(`WARNING: ledger at ${FILE} (iteration ${s.iteration}) predates campaign randomization — it has NO scenario/tweak/assets roll.`);
        console.log(`Kept as-is (a mid-campaign re-roll would strand its frontier). Finish or abandon it, then 'roll --force' to start a rolled campaign.`);
      }
      break;
    }
    const kv = (k) => rest.find((a) => a.startsWith(`${k}=`))?.slice(k.length + 1);
    const rawSeed = kv("seed");
    if (rawSeed !== undefined && !Number.isFinite(Number(rawSeed))) {
      console.error(`invalid seed '${rawSeed}' — must be a number`);
      process.exit(1);
    }
    let picked;
    try {
      picked = roll({
        seed: rawSeed !== undefined ? Number(rawSeed) : undefined,
        scenarioId: kv("scenario"),
        tweakId: kv("tweak"),
        assetsId: kv("assets"),
      });
    } catch (e) {
      console.error(e.message);
      process.exit(1);
    }
    const fixBranch = kv("fixBranch") || "playthrough-fixes";
    save({
      ...baseState(picked.scenario.missions, fixBranch),
      seed: picked.seed,
      scenario: { id: picked.scenario.id, name: picked.scenario.name, goal: picked.scenario.goal },
      tweak: picked.tweak,
      assets: picked.assets,
    });
    console.log(`rolled campaign (seed ${picked.seed}) -> ${FILE}`);
    for (const w of picked.warnings) console.log(`  WARNING: ${w}`);
    console.log(`  scenario: ${picked.scenario.name} [${picked.scenario.id}] — ${picked.scenario.missions.join(", ")}`);
    console.log(`  goal:     ${picked.scenario.goal}`);
    console.log(`  tweak:    ${picked.tweak.name} [${picked.tweak.id}] — ${picked.tweak.instructions}`);
    console.log(`  assets:   ${picked.assets.name} [${picked.assets.id}] — DARK_ASSET_PATH=${picked.assets.path}`);
    break;
  }
  case "show": {
    const s = load();
    if (!s) { console.error(`no ledger at ${FILE} — run: node playthrough-state.mjs init`); process.exit(1); }
    console.log(JSON.stringify(s, null, 2));
    console.log(`\nNEXT: ${nextAction(s)}`);
    break;
  }
  case "advance": {
    const s = load(); if (!s) process.exit(1);
    const [level, saveName, pos, ...note] = rest;
    s.frontier = { level, save: saveName, position: (pos || "").split(",").map(Number), note: note.join(" ") };
    s.history.push({ iteration: s.iteration, level, note: note.join(" ") });
    s.iteration += 1;
    save(s);
    console.log(`frontier -> ${level} (save '${saveName}'); iteration now ${s.iteration}`);
    break;
  }
  case "blocker": {
    const s = load(); if (!s) process.exit(1);
    const [sub, ...bargs] = args;
    if (sub === "add") {
      const [level, kind, issue, ...desc] = bargs;
      s.blockers.push({ issue: Number(issue), level, kind, desc: desc.join(" "), pr: null, status: "open" });
      console.log(`recorded blocker #${issue} (${kind}) @ ${level}`);
    } else if (sub === "set") {
      const [issue, status, pr] = bargs;
      const b = s.blockers.find((x) => x.issue === Number(issue));
      if (!b) { console.error(`no blocker #${issue}`); process.exit(1); }
      b.status = status; if (pr) b.pr = Number(pr);
      console.log(`blocker #${issue} -> ${status}${pr ? ` (PR #${pr})` : ""}`);
    } else { console.error("usage: blocker add|set ..."); process.exit(1); }
    save(s);
    break;
  }
  default:
    console.error("usage: init | roll | show | advance | blocker add|set (see file header)");
    process.exit(1);
}
