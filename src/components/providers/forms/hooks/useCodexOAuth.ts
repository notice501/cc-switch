import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { codexAuthApi, settingsApi, type CodexOAuthStatus } from "@/lib/api";

type PollingState = "idle" | "polling" | "success" | "error";

interface UseCodexOAuthProps {
  providerId?: string;
  settingsConfig: Record<string, unknown>;
  onSettingsConfigChange: (settingsConfig: Record<string, unknown>) => void;
}

export function useCodexOAuth({
  providerId,
  settingsConfig,
  onSettingsConfigChange,
}: UseCodexOAuthProps) {
  const [pollingState, setPollingState] = useState<PollingState>("idle");
  const [error, setError] = useState<string | null>(null);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const pollingIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const settingsKey = useMemo(() => JSON.stringify(settingsConfig ?? {}), [settingsConfig]);

  const stopPolling = useCallback(() => {
    if (pollingIntervalRef.current) {
      clearInterval(pollingIntervalRef.current);
      pollingIntervalRef.current = null;
    }
  }, []);

  useEffect(() => () => stopPolling(), [stopPolling]);

  const { data: status, refetch: refetchStatus } = useQuery<CodexOAuthStatus>({
    queryKey: ["codex-oauth-status", providerId ?? "draft", settingsKey],
    queryFn: () => codexAuthApi.codexAuthGetStatus(settingsConfig),
    staleTime: 0,
  });

  const startLoginMutation = useMutation({
    mutationFn: () => codexAuthApi.codexAuthStartLogin(),
    onSuccess: async (session) => {
      setActiveSessionId(session.sessionId);
      setPollingState("polling");
      setError(null);

      try {
        await settingsApi.openExternal(session.authorizeUrl);
      } catch (openError) {
        console.error("[CodexOAuth] Failed to open browser", openError);
      }

      const pollOnce = async (): Promise<boolean> => {
        try {
          const result = await codexAuthApi.codexAuthPollLogin(
            session.sessionId,
            providerId,
          );
          if (!result.pending && result.result) {
            stopPolling();
            setPollingState("success");
            onSettingsConfigChange(result.result.settingsConfig);
            await refetchStatus();
            setPollingState("idle");
            setActiveSessionId(null);
            return true;
          }
          return false;
        } catch (pollError) {
          stopPolling();
          setPollingState("error");
          setError(
            pollError instanceof Error ? pollError.message : String(pollError),
          );
          setActiveSessionId(null);
          return true;
        }
      };

      const completed = await pollOnce();
      if (!completed) {
        pollingIntervalRef.current = setInterval(() => {
          void pollOnce();
        }, 2000);
      }
    },
    onError: (loginError) => {
      setPollingState("error");
      setError(loginError instanceof Error ? loginError.message : String(loginError));
    },
  });

  const refreshMutation = useMutation({
    mutationFn: () => codexAuthApi.codexAuthRefresh(settingsConfig, providerId),
    onSuccess: async (result) => {
      onSettingsConfigChange(result.settingsConfig);
      setError(null);
      await refetchStatus();
    },
    onError: (refreshError) => {
      setError(
        refreshError instanceof Error ? refreshError.message : String(refreshError),
      );
    },
  });

  const startLogin = useCallback(() => {
    stopPolling();
    setPollingState("idle");
    setError(null);
    startLoginMutation.mutate();
  }, [startLoginMutation, stopPolling]);

  const refresh = useCallback(() => {
    setError(null);
    refreshMutation.mutate();
  }, [refreshMutation]);

  return {
    status,
    error,
    pollingState,
    activeSessionId,
    isPolling: pollingState === "polling",
    isRefreshing: refreshMutation.isPending,
    startLogin,
    refresh,
    refetchStatus,
  };
}
