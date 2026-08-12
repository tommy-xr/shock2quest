// Fail loudly when this package's dev dependencies are not installed.
//
// npm puts `node_modules/.bin` first on PATH, so with deps installed `tsc`
// resolves to the pinned local TypeScript. Without them it silently falls
// through to whatever `tsc` happens to be on the global PATH - often one many
// major versions old - and `npm test` / `npm run test:e2e` die in a wall of
// TS6046/TS1005 errors that read like source problems but are just the wrong
// compiler. That is easy to hit in a fresh git worktree, where `node_modules`
// does not carry over.
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

try {
  require.resolve("typescript");
} catch {
  console.error(
    [
      "",
      "@shock2vr/sdk: dependencies are not installed in this worktree.",
      "",
      "  cd tools/shock2-sdk && npm install",
      "",
      "(Without them `tsc` resolves to the global TypeScript, if any, and the",
      " build fails with confusing TS6046/TS1005 errors.)",
      "",
    ].join("\n"),
  );
  process.exit(1);
}
