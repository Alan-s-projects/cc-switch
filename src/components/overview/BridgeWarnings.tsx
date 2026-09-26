import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle } from "lucide-react";
import { useWindowActive } from "@/lib/windowActivity";
import type { ProxyStatus } from "@/types/proxy";

export function BridgeWarnings({ status }: { status?: ProxyStatus }) {
  const active = useWindowActive();
  const connection = useQuery({
    queryKey: ["codex-setup-suggestion", "connection-check"],
    queryFn: () =>
      invoke<{
        configured: boolean;
        configExists: boolean;
        configPath: string;
      }>("get_codex_setup_suggestion", { configPath: null }),
    enabled: active,
    refetchInterval: false,
    refetchOnWindowFocus: true,
    retry: false,
  });
  const warnings: Array<{ title: string; message: string; path?: string }> = [];
  if (status && !status.running) {
    warnings.push({
      title: "Proxy is stopped",
      message:
        "Turn on the proxy switch in the top bar before using Codex through this Copilot bridge.",
    });
  }
  if (connection.error) {
    warnings.push({
      title: "Could not check Codex configuration",
      message:
        "Open Connect to check the TOML location and review its settings.",
    });
  } else if (connection.data && !connection.data.configured) {
    warnings.push({
      title: "Codex is not connected to Atlas",
      message:
        (connection.data.configExists
          ? "The detected TOML points elsewhere. "
          : "No TOML was found at the detected location. ") +
        "Open Connect, review the proposed TOML, and apply the changes yourself.",
      path: connection.data.configPath,
    });
  }
  if (warnings.length === 0) return null;

  return (
    <section aria-label="Connection warnings" className="space-y-3">
      {warnings.map(({ title, message, path }) => (
        <div
          key={title}
          role="alert"
          className="flex items-start gap-3 rounded-lg border border-amber-500/40 bg-amber-500/10 px-6 py-4"
        >
          <AlertTriangle
            aria-hidden
            className="mt-0.5 h-5 w-5 shrink-0 text-amber-600 dark:text-amber-400"
          />
          <div className="min-w-0 space-y-1">
            <h2 className="text-base font-semibold">{title}</h2>
            <p className="text-sm text-muted-foreground">{message}</p>
            {path && (
              <p className="break-all font-mono text-xs text-muted-foreground">
                {path}
              </p>
            )}
          </div>
        </div>
      ))}
    </section>
  );
}
