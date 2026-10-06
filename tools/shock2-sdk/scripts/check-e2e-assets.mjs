// The full suite exercises remaster models, strings and animation tables.
// A legacy install can load missions but produces misleading feature failures.
import { existsSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve, join } from "node:path";

const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));
const candidates = process.env.DARK_ASSET_PATH
  ? [resolve(process.env.DARK_ASSET_PATH)]
  : [join(repoRoot, "Data"), resolve(repoRoot, "../Data")];
const sentinels = ["sshock2.kpf", "shock2.gam", "res/obj.crf", "res/mesh.crf", "motiondb.bin"];
const root = candidates.find(candidate => sentinels.some(name => existsSync(join(candidate, name))));
const isFile = path => existsSync(path) && statSync(path).isFile();
const isDir = path => existsSync(path) && statSync(path).isDirectory();
if (!root || !isFile(join(root, "sshock2.kpf")) || !isDir(join(root, "mods"))) {
  console.error([
    "shock2-sdk e2e assets: FAIL",
    `Checked: ${candidates.join(", ")}`,
    "The full e2e suite requires the 25th Anniversary install's sshock2.kpf and mods/.",
    "Set DARK_ASSET_PATH to that install before running npm run test:e2e.",
    "Legacy data can load missions but lacks the models, strings and animations these tests assert.",
  ].join("\n"));
  process.exit(1);
}
console.log(`shock2-sdk e2e assets: ${root} (25th Anniversary)`);
