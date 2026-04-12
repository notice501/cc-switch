import { invoke } from "@tauri-apps/api/core";
import type { AgentOverview, AgentPlan } from "@/types";

export interface PlanAgentRouteArgs {
  task: string;
  policy?: string;
  target?: string;
  mode?: string;
}

export const agentApi = {
  async getOverview(): Promise<AgentOverview> {
    return await invoke("get_agent_overview");
  },

  async planRoute(args: PlanAgentRouteArgs): Promise<AgentPlan> {
    return await invoke("plan_agent_route", { args });
  },
};
