import { spawnSync } from "node:child_process";
import path from "node:path";

const variant = process.argv[2] === "cn" ? "cn" : "standard";
const env = { ...process.env };
if (variant === "cn") {
  env.AURORA_REGION_LOCK = "CN";
  env.CARGO_TARGET_DIR = path.join(process.cwd(), "target", "cn");
}
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
