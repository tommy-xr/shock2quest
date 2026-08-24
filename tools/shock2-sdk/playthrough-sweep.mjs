// Full-game play-through sweep. For each mission in narrative order: load it,
// run baseline checkpoints (spawn/health, carried inventory, notable items,
// transition targets), log richly, and screenshot each checkpoint. Writes a
// markdown report to /tmp/claude/playthrough/report.md; screenshots land in
// /tmp/claude/ (the runtime's fixed output dir). Robust per-mission (one crash
// won't abort the sweep). Report-only: does not file issues / spawn fix-agents.
import { GameServer } from "./dist/src/index.js";
import { mkdirSync, writeFileSync, renameSync, existsSync } from "node:fs";

// Narrative order (Data/ has all of these). shodan is last and known to crash
// on load (#267) - kept in so the sweep records it as a [functionality] gap.
const MISSIONS = process.env.SWEEP_MISSIONS
  ? process.env.SWEEP_MISSIONS.split(",")
  : [
      "earth", "station",
      "medsci1", "medsci2",
      "eng1", "eng2",
      "hydro1", "hydro2", "hydro3",
      "ops1", "ops2", "ops3", "ops4",
      "rec1", "rec2", "rec3",
      "command1", "command2",
      "rick1", "rick2", "rick3",
      "many", "shodan",
    ];

const ITEM_CATEGORIES = {
  weapon: /wrench|pistol|shotgun|assault|laser|rifle|grenade|crystal|fusion|worm launcher|stasis/i,
  ammo: /bullet|shell|nanite|clip|cell|ammo|slug|dart/i,
  consumable: /hypo|medkit|med patch|patch|booster|kit|drink|snack|juice|chips/i,
  keycard: /keycard|access card|key card|regen|replicator code|card$/i,
  log: /log|email|audiolog|data ?log|note/i,
};
const SCR = "/tmp/claude";
const OUT = "/tmp/claude/playthrough";
mkdirSync(OUT, { recursive: true });

const finite = (p) => [p.x, p.y, p.z].every(Number.isFinite);
const report = [];
const summary = [];

async function shot(game, name) {
  try {
    await game.screenshot(name);
    const src = `${SCR}/${name}`;
    if (existsSync(src)) {
      renameSync(src, `${OUT}/${name}`);
      return `![${name}](${name})`;
    }
  } catch {}
  return `(screenshot ${name} unavailable)`;
}

async function sweepMission(mission) {
  // No port: the runtime binds an ephemeral one and the SDK reads it back, so
  // a leftover runtime can never collide with this sweep.
  const lines = [`\n## ${mission}`];
  let loaded = false;
  let game;
  try {
    game = await GameServer.launch({ mission: `${mission}.mis` });
  } catch (e) {
    lines.push(`- **LOAD FAILED** — \`${String(e.message).slice(0, 200)}\``);
    lines.push(`- GAP [functionality]: ${mission} did not launch/become ready.`);
    summary.push({ mission, loaded: false, gap: "load-failed" });
    report.push(lines.join("\n"));
    return;
  }
  try {
    await game.step({ frames: 5 });
    const info = await game.info();
    const pos = await game.player.position();
    loaded = info.mission != null;

    // CP1 — load & player state
    const hp = info.player.hit_points;
    lines.push(`### CP1 — Load & player state`);
    lines.push(`- mission: \`${info.mission}\``);
    lines.push(`- health: ${hp == null ? "none" : `${hp}/${info.player.max_hit_points}`}`);
    lines.push(`- psi: ${info.player.psi_points == null ? "none" : `${info.player.psi_points}/${info.player.max_psi_points}`}`);
    lines.push(`- wielded: ${info.player.wielded_entity_id ?? "none"}`);
    lines.push(`- position: (${pos.x.toFixed(1)}, ${pos.y.toFixed(1)}, ${pos.z.toFixed(1)}) finite=${finite(pos)}`);
    lines.push(`- ${await shot(game, `${mission}-cp1-spawn.png`)}`);

    // full entity list (one call), used for items + transitions
    const all = (await game.entities.list()).entities;
    lines.push(`- entities: ${all.length}`);

    // CP2 — carried inventory
    const inv = await game.player.inventory();
    lines.push(`### CP2 — Carried inventory (${inv.count})`);
    if (inv.items.length === 0) lines.push(`- (empty)`);
    for (const it of inv.items)
      lines.push(`- ${it.name ?? "?"} (#${it.entity_id}) @ ${it.location}`);

    // CP3 — notable items present in the level
    lines.push(`### CP3 — Notable items in level`);
    const byCat = {};
    for (const e of all) {
      for (const [cat, re] of Object.entries(ITEM_CATEGORIES)) {
        if (re.test(e.name)) {
          (byCat[cat] ??= []).push(e);
          break;
        }
      }
    }
    for (const [cat, es] of Object.entries(byCat)) {
      const names = [...new Set(es.map((e) => e.name))].slice(0, 6).join(", ");
      lines.push(`- **${cat}** (${es.length}): ${names}${es.length > 6 ? " …" : ""}`);
    }
    if (Object.keys(byCat).length === 0) lines.push(`- (none matched)`);
    lines.push(`- ${await shot(game, `${mission}-cp3-items.png`)}`);

    // CP4 — transition triggers present (the exact DestLevel is an inherited
    // property, which the runtime's entity-detail exposes only as direct props;
    // resolving it needs dark_query - a surfaced [debug_runtime] gap).
    lines.push(`### CP4 — Transition triggers`);
    const triggers = all.filter((e) => /tripwire|bulk_on|levelchange|elevator/i.test(e.name));
    const names = [...new Set(triggers.map((t) => t.name))].slice(0, 8).join(", ");
    lines.push(`- ${triggers.length} trigger(s): ${names || "(none)"}`);

    summary.push({ mission, loaded: true, hp, entities: all.length, items: Object.keys(byCat).length, triggers: triggers.length });
  } catch (e) {
    lines.push(`- **ERROR during sweep** — \`${String(e.message).slice(0, 200)}\``);
    summary.push({ mission, loaded, gap: "sweep-error" });
  } finally {
    try { await game[Symbol.asyncDispose](); } catch {}
  }
  report.push(lines.join("\n"));
  console.log(`  swept ${mission}: loaded=${loaded}`);
}

console.log(`Sweeping ${MISSIONS.length} missions...`);
for (let i = 0; i < MISSIONS.length; i++) {
  await sweepMission(MISSIONS[i]);
}

// Assemble report
const header = [
  `# Full-game play-through sweep`,
  ``,
  `| mission | loaded | health | entities | item cats | triggers |`,
  `| --- | --- | --- | --- | --- | --- |`,
  ...summary.map(
    (s) =>
      `| ${s.mission} | ${s.loaded ? "✅" : "❌ " + (s.gap ?? "")} | ${s.hp ?? "-"} | ${s.entities ?? "-"} | ${s.items ?? "-"} | ${s.triggers ?? "-"} |`,
  ),
  ``,
  `Loaded ${summary.filter((s) => s.loaded).length}/${MISSIONS.length} missions.`,
];
writeFileSync(`${OUT}/report.md`, header.join("\n") + "\n" + report.join("\n") + "\n");
console.log(`\nReport: ${OUT}/report.md`);
console.log(header.join("\n"));
