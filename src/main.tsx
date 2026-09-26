import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { DatabaseUpgrade } from "./components/DatabaseUpgrade";
import "./index.css";
// 导入国际化配置
import i18n from "./i18n";
import { QueryClientProvider } from "@tanstack/react-query";
import { ThemeProvider } from "@/components/theme-provider";
import { queryClient } from "@/lib/query";
import { Toaster } from "@/components/ui/sonner";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { message } from "@tauri-apps/plugin-dialog";
import { exit } from "@tauri-apps/plugin-process";
import { FrontendErrorBoundary } from "./components/FrontendErrorBoundary";
import {
  installGlobalErrorHandlers,
  reportFrontendError,
} from "./lib/frontendLogger";
import { initializeWindowActivity } from "@/lib/windowActivity";

installGlobalErrorHandlers();

// 配置加载错误payload类型
interface ConfigLoadErrorPayload {
  path?: string;
  error?: string;
  /** "db_version_too_new" 表示数据库版本过新，渲染应用内升级恢复界面 */
  kind?: string;
}

/**
 * 处理配置加载失败：显示错误消息并强制退出应用
 * 不给用户"取消"选项，因为配置损坏时应用无法正常运行
 */
async function handleConfigLoadError(
  payload: ConfigLoadErrorPayload | null,
): Promise<void> {
  const path = payload?.path ?? "~/.copilot-bridge-atlas/config.json";
  const detail = payload?.error ?? "Unknown error";

  await message(
    i18n.t("errors.configLoadFailedMessage", {
      path,
      detail,
      defaultValue:
        "Unable to read configuration file:\n{{path}}\n\nError details:\n{{detail}}\n\nPlease check if the JSON is valid, or restore from a backup file (e.g., config.json.bak) in the same directory.\n\nThe app will exit so you can fix this.",
    }),
    {
      title: i18n.t("errors.configLoadFailedTitle", {
        defaultValue: "Configuration Load Failed",
      }),
      kind: "error",
    },
  );

  await exit(1);
}

// 监听后端的配置加载错误事件：仅提醒用户并强制退出，不修改任何配置文件
try {
  void listen("configLoadError", async (evt) => {
    await handleConfigLoadError(evt.payload as ConfigLoadErrorPayload | null);
  });
} catch (e) {
  // 忽略事件订阅异常（例如在非 Tauri 环境下）
  reportFrontendError("config_load_error_listener", e);
}

async function bootstrap() {
  // 启动早期主动查询后端初始化错误，避免事件竞态
  try {
    const initError = (await invoke(
      "get_init_error",
    )) as ConfigLoadErrorPayload | null;
    if (initError && initError.kind === "db_version_too_new") {
      // 数据库版本过新：渲染应用内「升级应用」恢复界面，不进入正常 App
      ReactDOM.createRoot(document.getElementById("root")!).render(
        <React.StrictMode>
          <FrontendErrorBoundary>
            <ThemeProvider
              defaultTheme="system"
              storageKey="copilot-bridge-atlas-theme"
            >
              <DatabaseUpgrade payload={initError} />
              <Toaster />
            </ThemeProvider>
          </FrontendErrorBoundary>
        </React.StrictMode>,
      );
      return;
    }
    if (initError && (initError.path || initError.error)) {
      await handleConfigLoadError(initError);
      // 注意：不会执行到这里，因为 exit(1) 会终止进程
      return;
    }
  } catch (e) {
    // 忽略拉取错误，继续渲染
    reportFrontendError("get_init_error", e);
  }

  initializeWindowActivity();

  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <FrontendErrorBoundary>
        <QueryClientProvider client={queryClient}>
          <ThemeProvider
            defaultTheme="system"
            storageKey="copilot-bridge-atlas-theme"
          >
            <App />
            <Toaster />
          </ThemeProvider>
        </QueryClientProvider>
      </FrontendErrorBoundary>
    </React.StrictMode>,
  );
}

void bootstrap();
