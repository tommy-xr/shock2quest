// Run after npm run build, with Node 22+. This intentionally runs the expensive
// search headlessly; gameplay only reads the prepared resource it writes.
import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { GameServer } from '../dist/src/index.js';
import { aimVrHandAt } from '../dist/test/helpers/vr-hand.js';

const output = process.argv[2] ?? fileURLToPath(new URL('../../../assets/astra-vr-grips.json', import.meta.url));
const entries = [];
let solverRevision;
const game = await GameServer.launch({mission: 'debug_interactions', debugFlags: ['--vr', '--experimental', 'astra-bake-vr-grips']});
try {
  for (const hand of ['left', 'right']) {
    if (hand === 'right') await game.input.trigger('DebugReloadLevel');
    await game.step({frames:90});
    for (const template of [-1221, -1255, -4286, -52, -57, -54, -53, -2949, -1488, -74, -157]) {
      const item = (await game.entities.list()).entities.find(e => e.template_id === template);
      assert.ok(item, `Missing fixture ${template}`);
      await game.player.teleport({x:item.position[0],y:1,z:0});
      await aimVrHandAt(game,item.position,0.2,0,0,{hand});
      await game.step({frames:2});
      await game.input.set(`${hand}_hand.squeeze`,1);
      await game.step({frames:3});
      const result = (await game.info()).player.hand_grips.find(g => g.hand === hand);
      assert.ok(result?.grip && result.source === 'bake', `${hand}: failed to bake ${item.name}`);
      assert.ok(result.grip.contacts.filter(Boolean).length >= 3, `${hand}: fewer than three supported fingers on ${item.name}`);
      assert.ok(Number.isFinite(result.grip.score), `${hand}: invalid geometry on ${item.name}`);
      const {model,surface_hash,kinematics_hash,hints_hash,grip} = result;
      solverRevision=result.solver_revision;
      entries.push({model,hand,surface_hash,kinematics_hash,hints_hash,grip});
      console.log(`${model} ${hand}: ${result.solve_ms.toFixed(0)}ms; curls ${grip.curls.map(v => v.toFixed(2)).join(', ')}`);
      await game.input.set(`${hand}_hand.squeeze`,0);
      await game.step({frames:3});
    }
  }
} finally {
  await game.shutdown();
}
await writeFile(output,JSON.stringify({version:1,solver_revision:solverRevision,entries},null,2)+'\n');
console.log(`Wrote ${entries.length} prepared grips to ${output}`);
