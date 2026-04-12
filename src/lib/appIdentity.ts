const DEFAULT_APP_DISPLAY_NAME = "TermPilot";
const DEFAULT_APP_CONFIG_DIR_NAME = ".termpilot";
const DEFAULT_DEEPLINK_SCHEME = "termpilot";
const DEFAULT_STORAGE_PREFIX = "termpilot";

export const APP_DISPLAY_NAME =
  import.meta.env.VITE_APP_DISPLAY_NAME ?? DEFAULT_APP_DISPLAY_NAME;

export const APP_CONFIG_DIR_NAME =
  import.meta.env.VITE_APP_CONFIG_DIR_NAME ?? DEFAULT_APP_CONFIG_DIR_NAME;

export const APP_DEEPLINK_SCHEME =
  import.meta.env.VITE_APP_DEEPLINK_SCHEME ?? DEFAULT_DEEPLINK_SCHEME;

export const APP_STORAGE_PREFIX =
  import.meta.env.VITE_APP_STORAGE_PREFIX ?? DEFAULT_STORAGE_PREFIX;

export function appStorageKey(key: string): string {
  return `${APP_STORAGE_PREFIX}-${key}`;
}

export function defaultAppConfigPath() {
  return `~/${APP_CONFIG_DIR_NAME}/config.json`;
}
