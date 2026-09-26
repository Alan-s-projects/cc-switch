import { memo } from "react";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { useBridgeOverview } from "@/hooks/useBridgeOverview";
import { useGlobalProxyConfig } from "@/lib/query/proxy";
import type { ProxyStatus } from "@/types/proxy";
import type { RequestLog } from "@/types/usage";
import { fmtInt, fmtUsd, formatTokensShort } from "@/components/usage/format";
import { useWindowActive } from "@/lib/windowActivity";

const percent = (value: number) => `${(value * 100).toFixed(1)}%`;
const clock = (timestamp: number) =>
  new Date(timestamp * 1000).toLocaleTimeString("en-US", { hour12: false });

const RecentRequests = memo(function RecentRequests({
  logs,
}: {
  logs: RequestLog[];
}) {
  return (
    <div className="max-h-80 overflow-auto rounded-xl border">
      <table className="w-full text-left text-sm">
        <caption className="sr-only">Latest 10 completed requests</caption>
        <thead className="sticky top-0 bg-muted text-xs text-muted-foreground">
          <tr>
            {[
              "Time",
              "Model",
              "HTTP status",
              "Latency",
              "Tokens in / out",
              "Estimated cost",
            ].map((heading) => (
              <th
                key={heading}
                className="whitespace-nowrap px-4 py-2 font-medium"
              >
                {heading}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {logs.map((log) => (
            <tr key={log.requestId} className="border-t">
              <td
                className="whitespace-nowrap px-4 py-2 text-xs"
                title={new Date(log.createdAt * 1000).toLocaleString("en-US")}
              >
                {clock(log.createdAt)}
              </td>
              <td
                className="max-w-52 truncate px-4 py-2 font-mono text-xs"
                title={log.model}
              >
                {log.model}
              </td>
              <td
                className={`px-4 py-2 font-mono text-xs ${log.statusCode >= 200 && log.statusCode < 400 ? "text-emerald-600 dark:text-emerald-400" : "text-destructive"}`}
              >
                {log.statusCode || "Error"}
              </td>
              <td className="whitespace-nowrap px-4 py-2 text-xs">
                {(log.latencyMs / 1000).toFixed(2)} s
              </td>
              <td className="whitespace-nowrap px-4 py-2 text-xs">
                {formatTokensShort(log.inputTokens)} /{" "}
                {formatTokensShort(log.outputTokens)}
              </td>
              <td className="px-4 py-2 text-xs">
                {fmtUsd(log.totalCostUsd, 4)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {logs.length === 0 && (
        <p className="p-4 text-sm text-muted-foreground">
          No requests recorded yet.
        </p>
      )}
    </div>
  );
});

export function BridgeOverview({ status }: { status?: ProxyStatus }) {
  const active = useWindowActive();
  const { data: config } = useGlobalProxyConfig();
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
  const overview = useBridgeOverview();
  const summary = overview.data?.summary;
  const address = status?.running ? status.address : config?.listenAddress;
  const port = status?.running ? status.port : config?.listenPort;
  const localAddress =
    address === "0.0.0.0" || address === "::" ? "127.0.0.1" : address;
  const host =
    localAddress?.includes(":") && !localAddress.startsWith("[")
      ? `[${localAddress}]`
      : localAddress;
  const endpoint =
    host && port ? `http://${host}:${port}/v1` : "Loading proxy address…";
  const metrics = [
    ["Requests today", summary ? fmtInt(summary.totalRequests, "en-US") : "—"],
    ["Estimated cost today", summary ? fmtUsd(summary.totalCost, 4) : "—"],
    [
      "Success today",
      summary?.totalRequests ? `${summary.successRate.toFixed(1)}%` : "—",
    ],
    [
      "Cache reuse today",
      summary && summary.totalInputTokens + summary.totalCacheReadTokens > 0
        ? percent(summary.cacheHitRate)
        : "—",
    ],
  ];
  return (
    <section className="space-y-5" aria-label="Bridge overview">
      <section
        className="space-y-5 rounded-xl border border-border bg-card p-6 shadow-sm"
        aria-labelledby="bridge-proxy-title"
      >
        <h2 id="bridge-proxy-title" className="text-base font-semibold">
          Proxy
        </h2>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <p className="text-sm font-medium">
              {status
                ? status.running
                  ? "Proxy running"
                  : "Proxy stopped"
                : "Checking proxy"}
            </p>
            <p className="mt-1 break-all font-mono text-sm text-muted-foreground">
              {endpoint}
            </p>
          </div>
          <p className="text-sm text-muted-foreground">
            Active requests:{" "}
            <span className="font-medium text-foreground">
              {status?.active_connections ?? "—"}
            </span>
          </p>
        </div>
        {status && !status.running && (
          <div
            role="alert"
            className="rounded-xl border border-amber-500/40 bg-amber-500/10 px-5 py-3 text-sm"
          >
            <p className="font-medium">Proxy is stopped</p>
            <p className="mt-1 text-muted-foreground">
              Turn on the proxy switch in the top bar before using Codex through
              this Copilot bridge.
            </p>
          </div>
        )}
        {connection.error ? (
          <div
            role="alert"
            className="rounded-xl border border-amber-500/40 bg-amber-500/10 px-5 py-3 text-sm"
          >
            <p className="font-medium">Could not check Codex configuration</p>
            <p className="mt-1 text-muted-foreground">
              Open Connect to check the TOML location and review its settings.
            </p>
          </div>
        ) : connection.data && !connection.data.configured ? (
          <div
            role="alert"
            className="rounded-xl border border-amber-500/40 bg-amber-500/10 px-5 py-3 text-sm"
          >
            <p className="font-medium">Codex is not connected to Atlas</p>
            <p className="mt-1 text-muted-foreground">
              {connection.data.configExists
                ? "The detected TOML points elsewhere. "
                : "No TOML was found at the detected location. "}
              Open Connect, review the proposed TOML, and apply the changes
              yourself.
            </p>
            <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
              {connection.data.configPath}
            </p>
          </div>
        ) : null}
      </section>
      <section
        className="space-y-5 rounded-xl border border-border bg-card p-6 shadow-sm"
        aria-labelledby="bridge-usage-title"
      >
        <h2 id="bridge-usage-title" className="text-base font-semibold">
          Today's usage
        </h2>
        {overview.error && (
          <p role="alert" className="text-sm text-destructive">
            Usage could not be refreshed.{" "}
            {overview.data
              ? "Showing the last snapshot."
              : "Open Usage to retry."}
          </p>
        )}
        <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          {metrics.map(([label, value]) => (
            <div key={label} className="rounded-xl border px-5 py-4">
              <dt className="text-xs text-muted-foreground">{label}</dt>
              <dd className="mt-2 text-xl font-semibold tabular-nums">
                {value}
              </dd>
            </div>
          ))}
        </dl>
        <p className="text-xs text-muted-foreground">
          Token costs are estimates, not your Copilot bill. Cache reuse is the
          share of input tokens read from cache.
        </p>
      </section>
      <section
        className="space-y-5 rounded-xl border border-border bg-card p-6 shadow-sm"
        aria-labelledby="bridge-requests-title"
      >
        <div className="flex flex-wrap items-baseline justify-between gap-3">
          <div>
            <h2 id="bridge-requests-title" className="text-base font-semibold">
              Requests
            </h2>
            <p className="mt-1 text-xs text-muted-foreground">
              Latest 10 completed requests
            </p>
          </div>
          <p className="text-xs text-muted-foreground">
            {overview.dataUpdatedAt > 0 && (
              <>Updated {clock(overview.dataUpdatedAt / 1000)} · </>
            )}
            Updates while this window is active
          </p>
        </div>
        {overview.data ? (
          <RecentRequests logs={overview.data.recent.data} />
        ) : !overview.error ? (
          <p className="text-sm text-muted-foreground">
            Loading recent requests…
          </p>
        ) : null}
      </section>
    </section>
  );
}
