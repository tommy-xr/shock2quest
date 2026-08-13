// Ledger for the autonomous play-through modes (--auto / --auto-once). Persists
// the state that makes play-through work resumable across iterations:
// the frontier (furthest state, as a game save), the blocker->fix ledger, and
// the stacked-fixes branch. Atomic read-modify-write so a loop agent never
// corrupts it by hand-editing JSON.
//
//   node playthrough-state.mjs <cmd> [args]
//     init [order=earth,station,medsci1,eng1,...] [fixBranch=playthrough-fixes]
//     roll [seed=N] [scenario=<id>] [tweak=<id>] [presentation=<flat|vr>|--vr] [fixBranch=...] [--force|--restart]
//                                       init a RANDOMIZED campaign: pick a mission-
//                                       sequence scenario, a special tweak, and a
//                                       50/50 presentation mode (see scenarios.mjs)
//                                       and persist them in the ledger. Assets are
//                                       always 25th Anniversary. Idempotent like init —
//                                       an existing campaign keeps its roll;
//                                       --force re-rolls from scratch; --restart
//                                       forgets the previous ledger and rolls a
//                                       fresh campaign.
//     show                              print the ledger + the recommended next action
//     advance <level> <save> <x,y,z> [note...]   set frontier, bump iteration, log history
//     complete <level> [note...]        mark the campaign complete at its final mission
//     blocker add <level> <bug|feature-gap> <issue#> <desc...>   record a found blocker (status:open)
//     blocker set <issue#> <status> [pr#]        update a blocker (status: open|reworking|merged)
//     blocker fail <issue#> [pr#]       count a failed fix; pause after the third failure
//
// State file: $PT_STATE, else /tmp/claude/playthrough/state.json.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { ANNIVERSARY_ASSETS, PRESENTATION_MODES, roll } from "./scenarios.mjs";

const FILE = process.env.PT_STATE || "/tmp/claude/playthrough/state.json";
const save = (s) => {
  mkdirSync(dirname(FILE), { recursive: true });
  writeFileSync(FILE, JSON.stringify(s, null, 2) + "\n");
};
const presentationById = (id) => PRESENTATION_MODES.find((mode) => mode.id === id);
const load = () => {
  if (!existsSync(FILE)) return null;
  const state = JSON.parse(readFileSync(FILE, "utf8"));
  if (!state.scenario) return state;

  // Migrate rolled ledgers created before 25th-only / presentation-mode
  // campaigns. Preserve their frontier, blockers, scenario, tweak, and seed.
  let changed = false;
  if (state.assets?.id !== ANNIVERSARY_ASSETS.id || state.assets?.path !== ANNIVERSARY_ASSETS.path) {
    state.assets = ANNIVERSARY_ASSETS;
    changed = true;
  }
  if (!state.presentation) {
    state.presentation = roll({
      seed: state.seed,
      scenarioId: state.scenario.id,
      tweakId: state.tweak?.id,
    }).presentation;
    changed = true;
  }
  if (changed) save(state);
  return state;
};
const baseState = (order, fixBranch) => ({
  iteration: 0,
  mission_order: order,
  fix_branch: fixBranch,
  frontier: null,
  blockers: [],
  history: [],
  completed: false,
  completion: null,
});

const [cmd, ...args] = process.argv.slice(2);
const rest = args; // command-specific positional args (after the command)

function nextAction(s) {
  const openBlockers = s.blockers.filter((b) => !["merged"].includes(b.status));
  const failed = s.blockers.find((b) =>
    (b.fix_failures ?? (b.status === "failed" ? 3 : 0)) >= 3
  );
  const campaign = (s.assets
    ? ` Launch every runtime with DARK_ASSET_PATH=${s.assets.path} (${s.assets.name}) in ${s.presentation?.name ?? "the recorded presentation"}${s.presentation?.id === "vr" ? " with --vr" : ""}.`
    : "")
    + (s.scenario ? ` Campaign goal: ${s.scenario.goal}` : "")
    + (s.tweak && s.tweak.id !== "none" ? ` TWEAK [${s.tweak.name}]: ${s.tweak.instructions}` : "");
  if (s.completed) {
    return `COMPLETE — ${s.completion?.note || `finished ${s.completion?.level || "the campaign"}`}`;
  }
  if (failed) {
    const failures = failed.fix_failures ?? 3;
    return `PAUSE — blocker #${failed.issue} (${failed.desc}) fix failed ${failures} times and needs a human.`;
  }
  if (!s.frontier) return `Iteration ${s.iteration}: launch fresh at "${s.mission_order[0]}" and playtest.` + campaign;
  return `Iteration ${s.iteration}: rebuild the '${s.fix_branch}' branch, launch a runtime, POST /v1/load {file:"${s.frontier.save}"} (resumes ${s.frontier.level}), and playtest onward.`
    + (openBlockers.length ? ` Open fix PRs to merge: ${openBlockers.map((b) => `#${b.pr ?? "?"}(${b.status})`).join(", ")}.` : "")
    + campaign;
}

switch (cmd) {
  case "init": {
    // Idempotent: safe to call at the start of every autonomous iteration.
    // Only creates the ledger if absent; `--force` resets an existing campaign.
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
    // Randomized init: pick scenario + tweak + presentation and persist them so
    // every later iteration of the campaign plays under the same roll.
    // Idempotent like init — only rolls when no ledger exists. `--force`
    // replaces the roll; `--restart` forgets the old ledger and starts fresh.
    const force = args.includes("--force");
    const restart = args.includes("--restart");
    const forceVr = args.includes("--vr");
    const kv = (k) => rest.find((a) => a.startsWith(`${k}=`))?.slice(k.length + 1);
    if (force && restart) {
      console.error("--force and --restart are mutually exclusive: choose one way to replace the current campaign");
      process.exit(1);
    }
    if (forceVr && kv("presentation") && kv("presentation") !== "vr") {
      console.error("--vr conflicts with a non-VR presentation= override");
      process.exit(1);
    }
    if (kv("assets") !== undefined) {
      console.error("assets= is no longer configurable — every playtest uses the 25th Anniversary assets");
      process.exit(1);
    }
    const hadLedger = existsSync(FILE);
    if (hadLedger && !force && !restart) {
      const s = load();
      if (s.scenario) {
        if (forceVr && s.presentation?.id !== "vr") {
          s.presentation = presentationById("vr");
          save(s);
          console.log(
            `overrode presentation to VR for existing campaign at ${FILE}; frontier and iteration kept.`,
          );
        }
        console.log(`ledger already exists at ${FILE} (iteration ${s.iteration}) — kept its roll. Use --restart or --force to replace it with a fresh roll.`);
        console.log(
          `scenario: ${s.scenario.name} · tweak: ${s.tweak?.name} · assets: ${s.assets?.name} · presentation: ${s.presentation?.name ?? "not recorded"}`,
        );
      } else {
        console.log(`WARNING: ledger at ${FILE} (iteration ${s.iteration}) predates campaign randomization — it has NO scenario/tweak/presentation roll.`);
        console.log(`Kept as-is (a mid-campaign re-roll would strand its frontier). Finish or abandon it, then 'roll --force' to start a rolled campaign.`);
      }
      break;
    }
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
        presentationId: forceVr ? "vr" : kv("presentation"),
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
      presentation: picked.presentation,
    });
    if (restart && hadLedger) {
      console.log(`forgot previous ledger and rolled fresh campaign (seed ${picked.seed}) -> ${FILE}`);
    } else {
      console.log(`rolled campaign (seed ${picked.seed}) -> ${FILE}`);
    }
    for (const w of picked.warnings) console.log(`  WARNING: ${w}`);
    console.log(`  scenario: ${picked.scenario.name} [${picked.scenario.id}] — ${picked.scenario.missions.join(", ")}`);
    console.log(`  goal:     ${picked.scenario.goal}`);
    console.log(`  tweak:    ${picked.tweak.name} [${picked.tweak.id}] — ${picked.tweak.instructions}`);
    console.log(`  assets:   ${picked.assets.name} [${picked.assets.id}] — DARK_ASSET_PATH=${picked.assets.path}`);
    console.log(
      `  mode:     ${picked.presentation.name} [${picked.presentation.id}]${picked.presentation.runtimeArgs.length ? ` — ${picked.presentation.runtimeArgs.join(" ")}` : ""}`,
    );
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
  case "complete": {
    const s = load(); if (!s) process.exit(1);
    const [level, ...note] = rest;
    const finalLevel = s.mission_order[s.mission_order.length - 1];
    if (level !== finalLevel) {
      console.error(`cannot complete at '${level || ""}' — final mission is '${finalLevel}'`);
      process.exit(1);
    }
    const completionNote = note.join(" ") || `finished ${level}`;
    s.completed = true;
    s.completion = { level, note: completionNote };
    s.history.push({ iteration: s.iteration, level, note: completionNote, completed: true });
    save(s);
    console.log(`campaign complete at ${level}: ${completionNote}`);
    break;
  }
  case "blocker": {
    const s = load(); if (!s) process.exit(1);
    const [sub, ...bargs] = args;
    if (sub === "add") {
      const [level, kind, issue, ...desc] = bargs;
      s.blockers.push({
        issue: Number(issue),
        level,
        kind,
        desc: desc.join(" "),
        pr: null,
        status: "open",
        fix_failures: 0,
      });
      console.log(`recorded blocker #${issue} (${kind}) @ ${level}`);
    } else if (sub === "set") {
      const [issue, status, pr] = bargs;
      const b = s.blockers.find((x) => x.issue === Number(issue));
      if (!b) { console.error(`no blocker #${issue}`); process.exit(1); }
      if (status === "failed") {
        console.error("use 'blocker fail <issue#> [pr#]' to count a failed fix");
        process.exit(1);
      }
      if (!["open", "reworking", "merged"].includes(status)) {
        console.error(`invalid blocker status '${status}' — use open, reworking, or merged`);
        process.exit(1);
      }
      b.status = status; if (pr) b.pr = Number(pr);
      console.log(`blocker #${issue} -> ${status}${pr ? ` (PR #${pr})` : ""}`);
    } else if (sub === "fail") {
      const [issue, pr] = bargs;
      const b = s.blockers.find((x) => x.issue === Number(issue));
      if (!b) { console.error(`no blocker #${issue}`); process.exit(1); }
      const previousFailures = b.fix_failures ?? 0;
      if (previousFailures >= 3) {
        console.error(`blocker #${issue} already failed 3 times and needs a human`);
        process.exit(1);
      }
      b.fix_failures = previousFailures + 1;
      b.status = b.fix_failures >= 3 ? "failed" : "reworking";
      if (pr) b.pr = Number(pr);
      console.log(
        `blocker #${issue} fix failure ${b.fix_failures}/3`
          + (b.status === "failed" ? " — pause for a human" : " — revise and replay"),
      );
    } else { console.error("usage: blocker add|set|fail ..."); process.exit(1); }
    save(s);
    break;
  }
  default:
    console.error("usage: init | roll [--vr] [--force|--restart] | show | advance | complete | blocker add|set|fail (see file header)");
    process.exit(1);
}
