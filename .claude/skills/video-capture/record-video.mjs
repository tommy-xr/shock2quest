// Assemble a directory of periodic screenshots into a video with ffmpeg.
//
//   node record-video.mjs <frameDir> [outFile] [fps=15]
//
// Expects sequential frames named frame-0001.png, frame-0002.png, ... in
// <frameDir> (default out: <frameDir>/video.mp4). Handy for a playtest "video
// record": during a session, capture a frame every 4 sim-frames (60Hz / 4 =
// real-time 15fps) -
//   POST /v1/step {frames:4}; POST /v1/screenshot {filename:"<sub>/frame-0001.png"}
// (screenshots land under /tmp/claude/) - then run this to stitch them at 15fps.
// Smooth footage needs continuous motion (drive left/right-stick via
// /v1/control/input while capturing, e.g. a slow turn/walk); a discrete
// teleport-and-poke session yields a choppier record.
import { execFileSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";

const dir = process.argv[2];
if (!dir || !existsSync(dir)) {
  console.error("usage: node record-video.mjs <frameDir> [outFile] [fps]");
  process.exit(1);
}
const out = process.argv[3] || join(dir, "video.mp4");
const fps = process.argv[4] || "15";

const frames = readdirSync(dir).filter((f) => /^frame-\d+\.png$/.test(f));
if (frames.length === 0) {
  console.error(`no frame-*.png in ${dir}`);
  process.exit(1);
}

// -vf scale trunc-to-even keeps libx264/yuv420p happy for odd-sized captures.
try {
  execFileSync(
    "ffmpeg",
    [
      "-y", "-framerate", String(fps),
      "-pattern_type", "glob", "-i", join(dir, "frame-*.png"),
      "-c:v", "libx264", "-pix_fmt", "yuv420p",
      "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2",
      out,
    ],
    { stdio: ["ignore", "ignore", "inherit"] },
  );
} catch (e) {
  console.error(
    e.code === "ENOENT"
      ? "ffmpeg not found on PATH — install ffmpeg (e.g. brew install ffmpeg)."
      : `ffmpeg failed assembling ${dir}/frame-*.png -> ${out}: ${e.message}`,
  );
  process.exit(1);
}
console.log(`wrote ${out} (${frames.length} frames @ ${fps}fps)`);
