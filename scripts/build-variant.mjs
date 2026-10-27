import { spawnSync } from "node:child_process";
import path from "node:path";

const variant = process.argv[2] === "cn" ? "cn" : "standard";
const env = { ...process.env };
if (variant === "cn") {
  env.AURORA_REGION_LOCK = "CN";
  env.CARGO_TARGET_DIR = path.join(process.cwd(), "target", "cn");
}

// Frontend must be built first: tauri's generate_context! embeds ../dist at
// compile time. Run tsc/vite directly to avoid .cmd shims, which spawnSync
// rejects with EINVAL on Windows.
const tsc = spawnSync(
  process.execPath,
  [path.join("node_modules", "typescript", "bin", "tsc"), "--noEmit"],
  { stdio: "inherit", env },
);
if (tsc.status !== 0) process.exit(tsc.status ?? 1);

const vite = spawnSync(
  process.execPath,
  [path.join("node_modules", "vite", "bin", "vite.js"), "build"],
  { stdio: "inherit", env },
);
if (vite.status !== 0) process.exit(vite.status ?? 1);

const args = [
  path.join("node_modules", "@tauri-apps", "cli", "tauri.js"),
  "build",
  "--bundles",
  "nsis",
];
if (variant === "cn") {
  args.push("--config", path.join("src-tauri", "tauri.cn.conf.json"));
}
console.log(`Building ${variant} variant...`);
const result = spawnSync(process.execPath, args, { stdio: "inherit", env });
process.exit(result.status ?? 1);
