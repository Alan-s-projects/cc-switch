import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { focusManager, useQueryClient } from "@tanstack/react-query";
import { usageKeys } from "@/lib/query/usage";
import { useWindowActive } from "@/lib/windowActivity";

/**
 * 监听后端 `usage-log-recorded` 事件，收到后立刻 invalidate 所有
 * UsageDashboard 相关查询，让用户无需等待 30s 轮询周期。
 *
 * 后端在 `proxy_request_logs` 写入新行时会 emit 该事件（200ms 防抖合并），
 * Proxy usage changes invalidate the active dashboard queries.
 *
 * 该 hook 只挂在 UsageDashboard 上，避免在主界面其他位置无意义触发。
 */
export function useUsageEventBridge() {
  const queryClient = useQueryClient();
  const active = useWindowActive();

  useEffect(() => {
    if (!active) return;
    let unlisten: UnlistenFn | undefined;
    let disposed = false;

    (async () => {
      const off = await listen("usage-log-recorded", () => {
        if (disposed || !focusManager.isFocused()) return;
        // invalidate 整个 usage 命名空间：summary / trends / providerStats /
        // modelStats / logs 全部跟着重拉
        queryClient.invalidateQueries({ queryKey: usageKeys.all });
      });

      if (disposed) {
        off();
      } else {
        unlisten = off;
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [active, queryClient]);
}
