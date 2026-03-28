import { invoke } from "@tauri-apps/api/core";
import type { DispatchStatusSnapshot } from "@/types";

export const dispatchApi = {
  async getStatus(): Promise<DispatchStatusSnapshot> {
    return await invoke("get_dispatch_status");
  },
};
