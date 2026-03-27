type StorageKind = "localStorage" | "sessionStorage";

function getStorage(kind: StorageKind): Storage | null {
  if (typeof window === "undefined") {
    return null;
  }

  try {
    return window[kind];
  } catch (error) {
    console.warn(`[storage] Failed to access ${kind}`, error);
    return null;
  }
}

function readFromStorage(kind: StorageKind, key: string): string | null {
  const storage = getStorage(kind);
  if (!storage) {
    return null;
  }

  try {
    return storage.getItem(key);
  } catch (error) {
    console.warn(`[storage] Failed to read ${kind}:${key}`, error);
    return null;
  }
}

function writeToStorage(
  kind: StorageKind,
  key: string,
  value: string,
): boolean {
  const storage = getStorage(kind);
  if (!storage) {
    return false;
  }

  try {
    storage.setItem(key, value);
    return true;
  } catch (error) {
    console.warn(`[storage] Failed to write ${kind}:${key}`, error);
    return false;
  }
}

function removeFromStorage(kind: StorageKind, key: string): boolean {
  const storage = getStorage(kind);
  if (!storage) {
    return false;
  }

  try {
    storage.removeItem(key);
    return true;
  } catch (error) {
    console.warn(`[storage] Failed to remove ${kind}:${key}`, error);
    return false;
  }
}

export function readLocalStorage(key: string): string | null {
  return readFromStorage("localStorage", key);
}

export function writeLocalStorage(key: string, value: string): boolean {
  return writeToStorage("localStorage", key, value);
}

export function removeLocalStorage(key: string): boolean {
  return removeFromStorage("localStorage", key);
}

export function readSessionStorage(key: string): string | null {
  return readFromStorage("sessionStorage", key);
}

export function writeSessionStorage(key: string, value: string): boolean {
  return writeToStorage("sessionStorage", key, value);
}

export function removeSessionStorage(key: string): boolean {
  return removeFromStorage("sessionStorage", key);
}
