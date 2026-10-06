import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile, readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import { test, type TestContext } from "node:test";
import { GameServer } from "../src/index.js";
import { HttpClient } from "../src/client.js";

async function fakeRuntime(t: TestContext, infoDelay: number, hangShutdown = false) {
  const root = await mkdtemp(join(tmpdir(), "shock2-hung-runtime-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const pidFile = join(root, "pid");
  await writeFile(join(root, "cargo"), `#!${process.execPath}
const http = require('node:http');
const fs = require('node:fs');
const id = process.argv[process.argv.indexOf('--instance-id') + 1];
fs.writeFileSync(${JSON.stringify(pidFile)}, String(process.pid));
const server = http.createServer((req, res) => {
  if (req.url === '/v1/health') res.end(JSON.stringify({instance_id: id}));
  else if (req.url === '/v1/info') setTimeout(() => res.end('{}'), ${infoDelay});
  else if (req.url === '/v1/shutdown' && !${hangShutdown}) {
    res.end('{}'); server.close();
  }
});
server.listen(0, '127.0.0.1', () => console.log(
  'SHOCK2QUEST_PORT port='+server.address().port+' pid='+process.pid+' instance_id='+id));
// Bound the intentionally broken fixture even against the unfixed SDK.
setTimeout(() => process.exit(0), 15_000).unref();
`, { mode: 0o755 });
  return {
    pid: async () => Number(await readFile(pidFile, "utf8")),
    launch: async (launchTimeoutMs = 2_000) => {
      const path = process.env.PATH;
      const binary = process.env.SHOCK2_RUNTIME_BINARY;
      process.env.PATH = root + delimiter + path;
      delete process.env.SHOCK2_RUNTIME_BINARY;
      try {
        return await GameServer.launch({ mission: "fake", repoRoot: root, launchTimeoutMs });
      } finally {
        if (path === undefined) delete process.env.PATH; else process.env.PATH = path;
        if (binary === undefined) delete process.env.SHOCK2_RUNTIME_BINARY; else process.env.SHOCK2_RUNTIME_BINARY = binary;
      }
    },
  };
}

test("readiness deadline aborts a game-loop info response that arrives too late", { timeout: 5_000 }, async t => {
  const runtime = await fakeRuntime(t, 2_000);
  let game: GameServer | undefined;
  t.after(async () => { await game?.shutdown(); });
  const started = Date.now();
  await assert.rejects(async () => { game = await runtime.launch(300); }, /server not ready/);
  assert.ok(Date.now() - started < 1_500, "the request cannot outlive the launch budget");
});

test("shutdown kills its owned process even when HTTP shutdown never replies", { timeout: 20_000 }, async t => {
  const runtime = await fakeRuntime(t, 0, true);
  const game = await runtime.launch();
  const pid = await runtime.pid();
  const started = Date.now();
  await game.shutdown();
  assert.ok(Date.now() - started < 12_000, "the ten-second deadline includes HTTP time");
  assert.throws(() => process.kill(pid, 0), { code: "ESRCH" });
});

test("HTTP cancellation covers a response body that stalls after headers", { timeout: 1_500 }, async t => {
  const server = createServer((_request, response) => {
    response.writeHead(200, { "Content-Type": "application/json" });
    response.write('{"unfinished":');
  });
  t.after(() => { server.closeAllConnections(); server.close(); });
  await new Promise<void>(resolve => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert.ok(address && typeof address !== "string");
  const client = new HttpClient(`http://127.0.0.1:${address.port}`);
  await assert.rejects(client.get("/", AbortSignal.timeout(50)), /abort|timeout/i);
});
