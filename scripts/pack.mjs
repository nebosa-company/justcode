// Compresses the release executable with UPX after a build.
//
// Run via `npm run pack` (or `npm run build:packed`, which builds first).
// Bundling happens inside `tauri build`, before this script, so the NSIS/MSI
// installers still contain the uncompressed exe — they compress their payload
// themselves, so packing again inside them saves almost nothing. This shrinks
// the standalone `justcode.exe` that ships next to them.
//
// Trade-offs, stated plainly: a UPX-packed exe self-decompresses at launch (so
// it starts marginally slower) and is a well-known trigger for Windows Defender
// and other AV false positives. Kept out of the default build for those reasons.

import { spawnSync } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const upx = join(root, "tools", process.platform === "win32" ? "upx.exe" : "upx");
const exe = join(root, "src-tauri", "target", "release", "justcode.exe");

function fail(message) {
  console.error(`pack: ${message}`);
  process.exit(1);
}

if (!existsSync(upx)) fail(`UPX not found at ${upx}. See tools/README.md.`);
if (!existsSync(exe)) fail(`Release exe not found at ${exe}. Run "npm run tauri build" first.`);

const before = statSync(exe).size;

// UPX refuses to pack a file it has already packed, which is exactly what we
// want when the script runs twice — treat "AlreadyPackedException" as success.
const probe = spawnSync(upx, ["-t", exe], { encoding: "utf8" });
if (probe.status === 0) {
  console.log(`pack: ${exe} is already packed — nothing to do.`);
  process.exit(0);
}

const mb = (bytes) => (bytes / 1024 / 1024).toFixed(1);
console.log(`pack: compressing justcode.exe (${mb(before)} MB) with UPX…`);

const result = spawnSync(upx, ["--best", "--lzma", exe], { stdio: "inherit" });
if (result.status !== 0) fail(`UPX exited with code ${result.status}`);

const after = statSync(exe).size;
console.log(
  `pack: ${mb(before)} MB → ${mb(after)} MB ` +
    `(${Math.round((1 - after / before) * 100)}% smaller)`,
);
