// Ledger for the autonomous play-through loop (--auto). Persists the state that
// makes repeated play-through skill invocations resumable across iterations:
// the frontier (furthest state, as a game save), the blocker->fix ledger, and
// the stacked-fixes branch. Atomic read-modify-write so a loop agent never
// corrupts it by hand-editing JSON.
//
//   node playthrough-state.mjs <cmd> [args]
//     init [order=earth,station,medsci1,eng1,...] [fixBranch=playthrough-fixes]
//     show                              print the ledger + the recommended next action
//     advance <level> <save> <x,y,z> [note...]   set frontier, bump iteration, log history
//     checkpoint add <level> <id> <data.json> [note...]  record reviewed evidence
//     blocker add <level> <bug|feature-gap> <issue#> <desc...>   record a found blocker (status:open)
//     blocker set <issue#> <status> [pr#]        update a blocker (status: open|reworking|merged|failed)
//
// State file: $PT_STATE, else /tmp/claude/playthrough/state.json.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

const FILE = process.env.PT_STATE || "/tmp/claude/playthrough/state.json";
const load = () => {
  if (!existsSync(FILE)) return null;
  const state = JSON.parse(readFileSync(FILE, "utf8"));
  state.checkpoints ??= [];
  return state;
};
const save = (s) => { mkdirSync(dirname(FILE), { recursive: true }); writeFileSync(FILE, JSON.stringify(s, null, 2) + "\n"); };

const [cmd, ...args] = process.argv.slice(2);
const rest = args; // command-specific positional args (after the command)

function nextAction(s) {
  const openBlockers = s.blockers.filter((b) => !["merged"].includes(b.status));
  const failed = s.blockers.find((b) => b.status === "failed");
  if (failed) return `PAUSE — blocker #${failed.issue} (${failed.desc}) fix did not clear it; needs a human.`;
  if (!s.frontier) return `Iteration ${s.iteration}: launch fresh at "${s.mission_order[0]}" and playtest.`;
  return `Iteration ${s.iteration}: rebuild the '${s.fix_branch}' branch, launch a runtime, POST /v1/load {file:"${s.frontier.save}"} (resumes ${s.frontier.level}), and playtest onward.`
    + (openBlockers.length
      ? ` Open fixes: ${openBlockers.map((b) => b.pr
        ? `PR #${b.pr} for issue #${b.issue} (${b.status})`
        : `issue #${b.issue} awaiting PR (${b.status})`).join(", ")}.`
      : "");
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
    save({ iteration: 0, mission_order: order, fix_branch: fixBranch, frontier: null, checkpoints: [], blockers: [], history: [] });
    console.log(`initialized ${FILE} (${order.length} missions, fix branch '${fixBranch}')`);
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
  case "checkpoint": {
    const s = load(); if (!s) process.exit(1);
    const [sub, level, checkpoint, data, ...note] = args;
    if (sub !== "add" || !level || !checkpoint || !data) {
      console.error("usage: checkpoint add <level> <id> <data.json> [note...]");
      process.exit(1);
    }
    const evidence = { level, checkpoint, data, iteration: s.iteration, note: note.join(" ") };
    const existing = s.checkpoints.findIndex((x) => x.level === level && x.checkpoint === checkpoint);
    if (existing >= 0) s.checkpoints[existing] = evidence;
    else s.checkpoints.push(evidence);
    save(s);
    console.log(`reviewed checkpoint '${checkpoint}' @ ${level} -> ${data}`);
    break;
  }
  default:
    console.error("usage: init | show | advance | checkpoint add | blocker add|set (see file header)");
    process.exit(1);
}
