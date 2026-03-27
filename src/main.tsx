import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { UpdateProvider } from "./contexts/UpdateContext";
import "./index.css";
// 导入国际化配置
import i18n from "./i18n";
import { RootErrorBoundary } from "@/components/RootErrorBoundary";
import { QueryClientProvider } from "@tanstack/react-query";
import { ThemeProvider } from "@/components/theme-provider";
import { queryClient } from "@/lib/query";
import { Toaster } from "@/components/ui/sonner";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { message } from "@tauri-apps/plugin-dialog";
import { exit } from "@tauri-apps/plugin-process";

// 根据平台添加 body class，便于平台特定样式
try {
  const ua = navigator.userAgent || "";
  const plat = (navigator.platform || "").toLowerCase();
  const isMac = /mac/i.test(ua) || plat.includes("mac");
  if (isMac) {
    document.body.classList.add("is-mac");
  }
} catch {
  // 忽略平台检测失败
}

// 配置加载错误payload类型
interface ConfigLoadErrorPayload {
  path?: string;
  error?: string;
}

/**
 * 处理配置加载失败：显示错误消息并强制退出应用
 * 不给用户"取消"选项，因为配置损坏时应用无法正常运行
 */
async function handleConfigLoadError(
  payload: ConfigLoadErrorPayload | null,
): Promise<void> {
  const path = payload?.path ?? "~/.cc-switch/config.json";
  const detail = payload?.error ?? "Unknown error";

  await message(
    i18n.t("errors.configLoadFailedMessage", {
      path,
      detail,
      defaultValue:
        "无法读取配置文件：\n{{path}}\n\n错误详情：\n{{detail}}\n\n请手动检查 JSON 是否有效，或从同目录的备份文件（如 config.json.bak）恢复。\n\n应用将退出以便您进行修复。",
    }),
    {
      title: i18n.t("errors.configLoadFailedTitle", {
        defaultValue: "配置加载失败",
      }),
      kind: "error",
    },
  );

  await exit(1);
}

function BootGate() {
  React.useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    listen("configLoadError", async (evt) => {
      await handleConfigLoadError(evt.payload as ConfigLoadErrorPayload | null);
    })
      .then((off) => {
        if (disposed) {
          off();
          return;
        }
        unlisten = off;
      })
      .catch((error) => {
        console.error("订阅 configLoadError 事件失败", error);
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  React.useEffect(() => {
    let isCancelled = false;

    const checkInitError = async () => {
      try {
        const initError = (await Promise.race([
          invoke("get_init_error") as Promise<ConfigLoadErrorPayload | null>,
          new Promise<null>((resolve) => {
            window.setTimeout(() => resolve(null), 1500);
          }),
        ])) as ConfigLoadErrorPayload | null;

        if (
          !isCancelled &&
          initError &&
          (initError.path || initError.error)
        ) {
          await handleConfigLoadError(initError);
        }
      } catch (error) {
        console.error("拉取初始化错误失败", error);
      }
    };

    void checkInitError();

    return () => {
      isCancelled = true;
    };
  }, []);

  return (
    <RootErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <ThemeProvider defaultTheme="system" storageKey="cc-switch-theme">
          <UpdateProvider>
            <App />
            <Toaster />
          </UpdateProvider>
        </ThemeProvider>
      </QueryClientProvider>
    </RootErrorBoundary>
  );
}

async function bootstrap() {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <BootGate />
    </React.StrictMode>,
  );
}

void bootstrap();
