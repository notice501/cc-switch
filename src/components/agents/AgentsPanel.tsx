import { useMemo, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Bot, Route, TerminalSquare, TimerReset } from "lucide-react";
import { agentApi } from "@/lib/api";
import type { AgentRunRecord } from "@/types";
import { Button } from "@/components/ui/button";

interface AgentsPanelProps {
  onOpenChange: (open: boolean) => void;
}

function formatTimestamp(value?: number | null) {
  if (!value) return "unknown";
  return new Date(value * 1000).toLocaleString();
}

function compactDuration(value?: number | null) {
  if (!value) return "n/a";
  const seconds = value / 1000;
  return seconds >= 10 ? `${seconds.toFixed(1)}s` : `${seconds.toFixed(2)}s`;
}

function RunRow({ run }: { run: AgentRunRecord }) {
  return (
    <div className="rounded-xl border border-border/60 bg-card/70 p-4">
      <div className="flex items-start justify-between gap-4">
        <div className="space-y-1">
          <div className="font-medium text-sm">{run.target}</div>
          <div className="text-xs text-muted-foreground">
            {run.providerName} · {run.runtimeMode} · {run.taskKind}
          </div>
        </div>
        <div className="rounded-full bg-muted px-2 py-1 text-xs font-medium uppercase tracking-wide">
          {run.status}
        </div>
      </div>
      <div className="mt-3 text-xs text-muted-foreground">
        started {formatTimestamp(run.startedAt)} · duration {compactDuration(run.durationMs)}
      </div>
      {run.taskPreview && (
        <div className="mt-3 text-sm leading-6 text-foreground/90">
          {run.taskPreview}
        </div>
      )}
    </div>
  );
}

export function AgentsPanel({}: AgentsPanelProps) {
  const [task, setTask] = useState("");
  const [mode, setMode] = useState("");
  const [policy, setPolicy] = useState("");

  const overviewQuery = useQuery({
    queryKey: ["agentOverview"],
    queryFn: () => agentApi.getOverview(),
    refetchInterval: 2000,
  });

  const planMutation = useMutation({
    mutationFn: () =>
      agentApi.planRoute({
        task,
        mode: mode || undefined,
        policy: policy || undefined,
      }),
  });

  const status = overviewQuery.data?.status;
  const runs = overviewQuery.data?.runs ?? [];
  const runningCount = useMemo(
    () => runs.filter((run) => run.status === "running" || run.status === "queued").length,
    [runs],
  );

  return (
    <div className="px-6 py-4 flex flex-col gap-4">
      <div className="grid gap-4 md:grid-cols-3">
        <div className="rounded-2xl border border-border/60 bg-card/80 p-5">
          <div className="flex items-center gap-3">
            <Bot className="h-5 w-5 text-orange-500" />
            <div>
              <div className="text-sm font-medium">Agent Runtime</div>
              <div className="text-xs text-muted-foreground">
                terminal-first execution control plane
              </div>
            </div>
          </div>
          <div className="mt-4 text-2xl font-semibold">{status?.state ?? "idle"}</div>
          <div className="mt-2 text-sm text-muted-foreground">
            {runningCount} active run{runningCount === 1 ? "" : "s"}
          </div>
        </div>

        <div className="rounded-2xl border border-border/60 bg-card/80 p-5">
          <div className="flex items-center gap-3">
            <Route className="h-5 w-5 text-blue-500" />
            <div>
              <div className="text-sm font-medium">Latest Result</div>
              <div className="text-xs text-muted-foreground">
                most recent finished child execution
              </div>
            </div>
          </div>
          <div className="mt-4 text-sm font-medium">
            {status?.lastRun?.target ?? "No runs yet"}
          </div>
          <div className="mt-2 text-xs text-muted-foreground">
            {status?.lastRun
              ? `${status.lastRun.status} · ${compactDuration(status.lastRun.durationMs)}`
              : "Launch an agent from the terminal to populate runtime history."}
          </div>
        </div>

        <div className="rounded-2xl border border-border/60 bg-card/80 p-5">
          <div className="flex items-center gap-3">
            <TerminalSquare className="h-5 w-5 text-emerald-500" />
            <div>
              <div className="text-sm font-medium">CLI Surface</div>
              <div className="text-xs text-muted-foreground">
                `agent plan`, `agent run`, `agent watch`
              </div>
            </div>
          </div>
          <div className="mt-4 text-xs leading-6 text-muted-foreground">
            Use `agent run --task "..." --mode pane` inside tmux to open a real child pane.
          </div>
        </div>
      </div>

      <div className="rounded-2xl border border-border/60 bg-card/80 p-5">
        <div className="flex items-center justify-between gap-4">
          <div>
            <div className="text-sm font-medium">Route Coach</div>
            <div className="text-xs text-muted-foreground">
              test the new suggestion-based routing before dispatching from the terminal
            </div>
          </div>
          <Button
            onClick={() => planMutation.mutate()}
            disabled={!task.trim() || planMutation.isPending}
          >
            {planMutation.isPending ? "Planning..." : "Plan Route"}
          </Button>
        </div>
        <div className="mt-4 grid gap-3 md:grid-cols-[1fr_160px_180px]">
          <textarea
            value={task}
            onChange={(event) => setTask(event.target.value)}
            placeholder="Describe the subtask you want the runtime to route."
            className="min-h-28 rounded-xl border border-border bg-background px-3 py-2 text-sm outline-none"
          />
          <input
            value={mode}
            onChange={(event) => setMode(event.target.value)}
            placeholder="mode: pane/background/inline"
            className="rounded-xl border border-border bg-background px-3 py-2 text-sm outline-none"
          />
          <input
            value={policy}
            onChange={(event) => setPolicy(event.target.value)}
            placeholder="policy override"
            className="rounded-xl border border-border bg-background px-3 py-2 text-sm outline-none"
          />
        </div>

        {planMutation.data && (
          <div className="mt-4 rounded-xl border border-border/60 bg-background/70 p-4 text-sm">
            <div className="font-medium">
              {planMutation.data.recommendedTarget} · {planMutation.data.preferredRuntime}
            </div>
            <div className="mt-2 text-muted-foreground">
              {planMutation.data.taskKind} · {planMutation.data.reasoningLevel} ·{" "}
              {planMutation.data.costTier}
            </div>
            <div className="mt-3 leading-6">{planMutation.data.explanation}</div>
            {planMutation.data.fallbackChain.length > 0 && (
              <div className="mt-3 text-xs text-muted-foreground">
                fallback: {planMutation.data.fallbackChain.join(", ")}
              </div>
            )}
          </div>
        )}
      </div>

      <div className="rounded-2xl border border-border/60 bg-card/80 p-5">
        <div className="flex items-center justify-between gap-4">
          <div>
            <div className="text-sm font-medium">Recent Runs</div>
            <div className="text-xs text-muted-foreground">
              the runtime registry behind both `agent` and legacy `/dispatch-task`
            </div>
          </div>
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <TimerReset className="h-4 w-4" />
            refreshes every 2s
          </div>
        </div>
        <div className="mt-4 space-y-3">
          {runs.length === 0 ? (
            <div className="rounded-xl border border-dashed border-border/70 p-6 text-sm text-muted-foreground">
              No runtime activity yet.
            </div>
          ) : (
            runs.map((run) => <RunRow key={run.runId} run={run} />)
          )}
        </div>
      </div>
    </div>
  );
}
