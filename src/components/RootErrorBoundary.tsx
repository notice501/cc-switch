import React from "react";
import {
  removeLocalStorage,
  removeSessionStorage,
} from "@/lib/storage";
import { APP_DISPLAY_NAME, appStorageKey } from "@/lib/appIdentity";

interface RootErrorBoundaryProps {
  children: React.ReactNode;
}

interface RootErrorBoundaryState {
  error: Error | null;
}

const UI_STATE_KEYS = [
  appStorageKey("last-app"),
  appStorageKey("last-view"),
  appStorageKey("theme"),
];

export class RootErrorBoundary extends React.Component<
  RootErrorBoundaryProps,
  RootErrorBoundaryState
> {
  state: RootErrorBoundaryState = {
    error: null,
  };

  static getDerivedStateFromError(error: Error): RootErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error("[RootErrorBoundary] Unhandled render error", error, errorInfo);
  }

  private handleReload = () => {
    window.location.reload();
  };

  private handleResetUiState = () => {
    for (const key of UI_STATE_KEYS) {
      removeLocalStorage(key);
    }
    removeSessionStorage(appStorageKey("env-banner-dismissed"));
    this.handleReload();
  };

  render() {
    if (!this.state.error) {
      return this.props.children;
    }

    const detail =
      this.state.error.stack || this.state.error.message || String(this.state.error);

    return (
      <div className="min-h-screen bg-background text-foreground flex items-center justify-center p-6">
        <div className="w-full max-w-2xl rounded-2xl border border-border bg-card shadow-xl p-6 space-y-4">
          <div className="space-y-1">
            <h1 className="text-xl font-semibold">
              {APP_DISPLAY_NAME} failed to render
            </h1>
            <p className="text-sm text-muted-foreground">
              The app hit a frontend error during startup. You can reset the
              saved UI state and reopen into the default providers page.
            </p>
          </div>

          <div className="rounded-xl bg-muted p-4">
            <pre className="text-xs whitespace-pre-wrap break-words">{detail}</pre>
          </div>

          <div className="flex flex-wrap gap-3">
            <button
              type="button"
              onClick={this.handleResetUiState}
              className="inline-flex items-center rounded-lg bg-orange-500 px-4 py-2 text-sm font-medium text-white hover:bg-orange-600"
            >
              Reset UI state and reload
            </button>
            <button
              type="button"
              onClick={this.handleReload}
              className="inline-flex items-center rounded-lg border border-border px-4 py-2 text-sm font-medium hover:bg-muted"
            >
              Reload
            </button>
          </div>
        </div>
      </div>
    );
  }
}
