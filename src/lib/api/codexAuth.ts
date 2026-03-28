import { invoke } from "@tauri-apps/api/core";

export interface CodexOAuthStatus {
  authenticated: boolean;
  status: "not_logged_in" | "active" | "expiring" | "expired" | "invalid" | string;
  accountId: string | null;
  email: string | null;
  name: string | null;
  authProvider: string | null;
  planType: string | null;
  expiresAt: number | null;
  idTokenExpiresAt: number | null;
  lastRefresh: string | null;
  lastError: string | null;
}

export interface CodexOAuthLoginStart {
  sessionId: string;
  authorizeUrl: string;
  expiresAt: number;
}

export interface CodexOAuthLoginComplete {
  settingsConfig: Record<string, unknown>;
  status: CodexOAuthStatus;
}

export interface CodexOAuthLoginPoll {
  pending: boolean;
  result?: CodexOAuthLoginComplete | null;
}

export interface CodexOAuthRefreshResponse {
  settingsConfig: Record<string, unknown>;
  status: CodexOAuthStatus;
}

export async function codexAuthStartLogin(): Promise<CodexOAuthLoginStart> {
  return invoke("codex_auth_start_login");
}

export async function codexAuthPollLogin(
  sessionId: string,
  providerId?: string | null,
): Promise<CodexOAuthLoginPoll> {
  return invoke("codex_auth_poll_login", {
    sessionId,
    providerId: providerId ?? null,
  });
}

export async function codexAuthGetStatus(
  settingsConfig: Record<string, unknown>,
): Promise<CodexOAuthStatus> {
  return invoke("codex_auth_get_status", { settingsConfig });
}

export async function codexAuthRefresh(
  settingsConfig: Record<string, unknown>,
  providerId?: string | null,
): Promise<CodexOAuthRefreshResponse> {
  return invoke("codex_auth_refresh", {
    settingsConfig,
    providerId: providerId ?? null,
  });
}

export const codexAuthApi = {
  codexAuthStartLogin,
  codexAuthPollLogin,
  codexAuthGetStatus,
  codexAuthRefresh,
};
