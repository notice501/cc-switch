import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Clock3, FolderOpen, Hash, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import { ProviderIcon } from "@/components/ProviderIcon";
import { cn } from "@/lib/utils";
import type {
  DispatchActiveRun,
  DispatchStatusRun,
  DispatchStatusSnapshot,
} from "@/types";
import { formatTimestamp, getBaseName, getProviderIconName } from "./utils";

interface DispatchStatusCardProps {
  snapshot?: DispatchStatusSnapshot | null;
  appFilter?: string;
}

const getTargetApp = (target?: string | null) =>
  (target?.split(":", 1)[0] ?? "").trim().toLowerCase();

const formatElapsed = (startedAt: number, now: number) => {
  const elapsedSeconds = Math.max(0, Math.floor(now / 1000 - startedAt));
  if (elapsedSeconds < 60) return `${elapsedSeconds}s`;
  const minutes = Math.floor(elapsedSeconds / 60);
  const seconds = elapsedSeconds % 60;
  if (minutes < 60) return `${minutes}m ${seconds}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
};

const formatDuration = (durationMs?: number | null) => {
  if (!durationMs || durationMs < 1000) return "<1s";
  const totalSeconds = Math.round(durationMs / 1000);
  if (totalSeconds < 60) return `${totalSeconds}s`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes < 60) return `${minutes}m ${seconds}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
};

const shortRunId = (runId: string) => (runId ? runId.slice(0, 8) : "legacy");

const matchesAppFilter = (
  target: string | undefined,
  appFilter: string | undefined,
) => {
  if (!appFilter || appFilter === "all") return true;
  return getTargetApp(target) === appFilter;
};

const getOutcomeTone = (run: DispatchStatusRun) => {
  if (run.status === "succeeded") {
    return {
      labelKey: "sessionManager.dispatchSucceeded",
      defaultLabel: "成功",
      className:
        "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
    };
  }
  if (run.status === "timed_out" || run.timedOut) {
    return {
      labelKey: "sessionManager.dispatchTimedOut",
      defaultLabel: "超时",
      className:
        "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300",
    };
  }
  return {
    labelKey: "sessionManager.dispatchFailed",
    defaultLabel: "失败",
    className:
      "border-rose-500/30 bg-rose-500/10 text-rose-700 dark:text-rose-300",
  };
};

export function DispatchStatusCard({
  snapshot,
  appFilter = "all",
}: DispatchStatusCardProps) {
  const { t } = useTranslation();
  const [now, setNow] = useState(() => Date.now());
  const currentRun =
    snapshot?.currentRun &&
    matchesAppFilter(snapshot.currentRun.target, appFilter)
      ? snapshot.currentRun
      : null;
  const lastRun =
    snapshot?.lastRun && matchesAppFilter(snapshot.lastRun.target, appFilter)
      ? snapshot.lastRun
      : null;

  useEffect(() => {
    if (!currentRun) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [currentRun?.runId]);

  if (!currentRun && !lastRun) {
    return null;
  }

  const renderMeta = (
    run: DispatchActiveRun | DispatchStatusRun,
    elapsedLabel: string,
    timeLabel: string,
  ) => (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
      <span className="inline-flex items-center gap-1">
        <Hash className="size-3" />
        {shortRunId(run.runId)}
      </span>
      <span className="inline-flex items-center gap-1">
        <Clock3 className="size-3" />
        {timeLabel}
      </span>
      <span>{elapsedLabel}</span>
      <span className="inline-flex items-center gap-1 min-w-0">
        <FolderOpen className="size-3 shrink-0" />
        <span className="truncate max-w-[220px]">
          {getBaseName(run.cwd) || run.cwd}
        </span>
      </span>
    </div>
  );

  if (currentRun) {
    const targetApp = getTargetApp(currentRun.target) || "apps";
    return (
      <Card className="border-sky-500/20 bg-sky-500/[0.04]">
        <CardContent className="px-4 py-3">
          <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-2">
                <Badge className="gap-1.5 bg-sky-600 text-white hover:bg-sky-600">
                  <RefreshCw className="size-3 animate-spin" />
                  {t("sessionManager.dispatchRunning", {
                    defaultValue: "Dispatch 运行中",
                  })}
                </Badge>
                <span className="inline-flex items-center gap-2 min-w-0">
                  <ProviderIcon
                    icon={getProviderIconName(targetApp)}
                    name={targetApp}
                    size={16}
                  />
                  <span className="text-sm font-medium truncate">
                    {currentRun.providerName}
                  </span>
                </span>
                <span className="text-xs text-muted-foreground font-mono truncate">
                  {currentRun.target}
                </span>
              </div>
              <div className="mt-2">
                {renderMeta(
                  currentRun,
                  t("sessionManager.dispatchElapsed", {
                    defaultValue: "已运行 {{value}}",
                    value: formatElapsed(currentRun.startedAt, now),
                  }),
                  t("sessionManager.dispatchStartedAt", {
                    defaultValue: "开始于 {{value}}",
                    value: formatTimestamp(currentRun.startedAt * 1000),
                  }),
                )}
              </div>
            </div>
            <div className="text-xs text-muted-foreground">
              {t("sessionManager.dispatchBackgroundHint", {
                defaultValue: "后台执行中，主会话可以继续工作。",
              })}
            </div>
          </div>
        </CardContent>
      </Card>
    );
  }

  if (!lastRun) {
    return null;
  }

  const outcome = getOutcomeTone(lastRun);
  const targetApp = getTargetApp(lastRun.target) || "apps";

  return (
    <Card className="border-border/60 bg-muted/20">
      <CardContent className="px-4 py-3">
        <div className="flex flex-col gap-3 md:flex-row md:items-start md:justify-between">
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <Badge
                variant="outline"
                className={cn("gap-1.5", outcome.className)}
              >
                {t("sessionManager.dispatchLastRun", {
                  defaultValue: "最近一次 Dispatch",
                })}
                <span className="font-medium">
                  {t(outcome.labelKey, {
                    defaultValue: outcome.defaultLabel,
                  })}
                </span>
              </Badge>
              <span className="inline-flex items-center gap-2 min-w-0">
                <ProviderIcon
                  icon={getProviderIconName(targetApp)}
                  name={targetApp}
                  size={16}
                />
                <span className="text-sm font-medium truncate">
                  {lastRun.providerName}
                </span>
              </span>
              <span className="text-xs text-muted-foreground font-mono truncate">
                {lastRun.target}
              </span>
            </div>
            <div className="mt-2">
              {renderMeta(
                lastRun,
                t("sessionManager.dispatchDuration", {
                  defaultValue: "耗时 {{value}}",
                  value: formatDuration(lastRun.durationMs),
                }),
                t("sessionManager.dispatchFinishedAt", {
                  defaultValue: "完成于 {{value}}",
                  value: formatTimestamp(lastRun.timestamp * 1000),
                }),
              )}
            </div>
            {lastRun.resultPreview ? (
              <p className="mt-2 text-xs text-muted-foreground truncate">
                {lastRun.resultPreview}
              </p>
            ) : null}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
