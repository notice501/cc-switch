import { spawnSync } from "node:child_process";

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

const result = spawnSync("pnpm", ["tauri", "build"], {
  env,
  stdio: "inherit",
  shell: process.platform === "win32",
});

if (typeof result.status === "number") {
  process.exit(result.status);
}

process.exit(result.error ? 1 : 0);
