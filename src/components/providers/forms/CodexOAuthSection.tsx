import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import type { CodexOAuthStatus } from "@/lib/api";
import { Loader2, LogOut, RefreshCw, ShieldCheck } from "lucide-react";

interface CodexOAuthSectionProps {
  authMode: "manual" | "oauth";
  status?: CodexOAuthStatus;
  error?: string | null;
  isPolling: boolean;
  isRefreshing: boolean;
  onAuthModeChange: (mode: "manual" | "oauth") => void;
  onLogin: () => void;
  onRefresh: () => void;
  onLogout: () => void;
}

const STATUS_LABELS: Record<string, string> = {
  not_logged_in: "未登录",
  active: "已登录",
  expiring: "即将过期",
  expired: "已失效",
  invalid: "需重新登录",
};

export function CodexOAuthSection({
  authMode,
  status,
  error,
  isPolling,
  isRefreshing,
  onAuthModeChange,
  onLogin,
  onRefresh,
  onLogout,
}: CodexOAuthSectionProps) {
  const statusLabel = STATUS_LABELS[status?.status ?? "not_logged_in"] ?? status?.status ?? "未登录";
  const isLoggedIn = Boolean(status?.authenticated && status?.accountId);

  return (
    <div className="space-y-3 rounded-xl border border-border bg-muted/20 p-4">
      <div className="flex items-center justify-between gap-3">
        <div>
          <Label>Codex OAuth</Label>
          <p className="text-xs text-muted-foreground mt-1">
            一个 Codex Provider 绑定一个 ChatGPT OAuth 账号。切换账号就是切换 Provider。
          </p>
        </div>
        <Badge variant={isLoggedIn ? "default" : "secondary"}>{statusLabel}</Badge>
      </div>

      <div className="flex gap-2">
        <Button
          type="button"
          size="sm"
          variant={authMode === "oauth" ? "default" : "outline"}
          onClick={() => onAuthModeChange("oauth")}
        >
          OAuth 登录
        </Button>
        <Button
          type="button"
          size="sm"
          variant={authMode === "manual" ? "default" : "outline"}
          onClick={() => onAuthModeChange("manual")}
        >
          手填 auth.json
        </Button>
      </div>

      {authMode === "oauth" && (
        <div className="space-y-3">
          {isLoggedIn ? (
            <div className="rounded-lg border bg-background p-3 text-sm">
                <div className="flex items-center gap-2 font-medium">
                <ShieldCheck className="h-4 w-4 text-emerald-500" />
                <span>{status?.name || status?.email || status?.accountId}</span>
              </div>
              <div className="mt-2 space-y-1 text-xs text-muted-foreground">
                {status?.email && <p>Email: {status.email}</p>}
                {status?.planType && <p>Plan: {status.planType}</p>}
                {status?.accountId && <p>Account ID: {status.accountId}</p>}
              </div>
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">
              使用浏览器登录 ChatGPT，登录结果保存到当前 Codex Provider，不会共享给其他 Provider。
            </p>
          )}

          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={onLogin}
              disabled={isPolling}
            >
              {isPolling ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  等待登录中...
                </>
              ) : isLoggedIn ? (
                "重新登录"
              ) : (
                "浏览器登录"
              )}
            </Button>
            <Button
              type="button"
              variant="outline"
              onClick={onRefresh}
              disabled={isRefreshing || isPolling || !isLoggedIn}
            >
              {isRefreshing ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  刷新中...
                </>
              ) : (
                <>
                  <RefreshCw className="mr-2 h-4 w-4" />
                  刷新状态
                </>
              )}
            </Button>
            <Button
              type="button"
              variant="ghost"
              onClick={onLogout}
              disabled={isPolling}
            >
              <LogOut className="mr-2 h-4 w-4" />
              注销当前账号
            </Button>
          </div>

          {status?.lastError && (
            <p className="text-sm text-destructive">{status.lastError}</p>
          )}
          {error && <p className="text-sm text-destructive">{error}</p>}
        </div>
      )}
    </div>
  );
}
