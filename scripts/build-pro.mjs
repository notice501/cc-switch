import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const env = {
  ...process.env,
  CCSWITCH_APP_DISPLAY_NAME: "CCswitch Pro",
  CCSWITCH_APP_CONFIG_DIR_NAME: ".ccswitch-pro",
  CCSWITCH_DEEPLINK_SCHEME: "ccswitchpro",
  CCSWITCH_WEBDAV_REMOTE_ROOT: "ccswitch-pro-sync",
  VITE_APP_DISPLAY_NAME: "CCswitch Pro",
  VITE_APP_CONFIG_DIR_NAME: ".ccswitch-pro",
  VITE_APP_DEEPLINK_SCHEME: "ccswitchpro",
  VITE_APP_STORAGE_PREFIX: "ccswitch-pro",
};

const rootDir = path.resolve(import.meta.dirname, "..");
const infoPlistPath = path.join(rootDir, "src-tauri", "Info.plist");
const proInfoPlistPath = path.join(rootDir, "src-tauri", "Info.pro.plist");
const originalInfoPlist = fs.readFileSync(infoPlistPath, "utf8");
const proInfoPlist = fs.readFileSync(proInfoPlistPath, "utf8");

let result;
try {
  fs.writeFileSync(infoPlistPath, proInfoPlist);
  result = spawnSync(
    "pnpm",
    ["tauri", "build", "--config", "src-tauri/tauri.pro.conf.json"],
    {
      env,
      stdio: "inherit",
      shell: process.platform === "win32",
    },
  );
} finally {
  fs.writeFileSync(infoPlistPath, originalInfoPlist);
}

if (typeof result.status === "number") {
  process.exit(result.status);
}

process.exit(result.error ? 1 : 0);
