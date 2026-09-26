import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { Card, CardContent } from "@/components/ui/card";
import { useUsageSummary } from "@/lib/query/usage";
import { Loader2 } from "lucide-react";
import { fmtUsd, formatTokensShort, parseFiniteNumber } from "./format";
import type { UsageRangeSelection } from "@/types/usage";

interface UsageHeroProps {
  range: UsageRangeSelection;
  appType?: "codex";
  providerName?: string;
  model?: string;
  refreshIntervalMs: number;
}

export function UsageHero({
  range,
  appType = "codex",
  providerName,
  model,
  refreshIntervalMs,
}: UsageHeroProps) {
  const { t } = useTranslation();

  const { data: summary, isLoading } = useUsageSummary(
    range,
    { appType, providerName, model },
    {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  );

  const input = summary?.totalInputTokens ?? 0;
  const output = summary?.totalOutputTokens ?? 0;
  const cacheRead = summary?.totalCacheReadTokens ?? 0;
  const realTotal = summary?.realTotalTokens ?? 0;
  const hitRate = summary?.cacheHitRate ?? 0;
  const totalCost = parseFiniteNumber(summary?.totalCost);
  const requests = summary?.totalRequests ?? 0;
  const latency = parseFiniteNumber(summary?.avgLatencyMs);

  const successRate =
    requests > 0 && summary ? `${summary.successRate.toFixed(1)}%` : "--";
  const averageLatency =
    requests > 0 && latency != null ? `${(latency / 1000).toFixed(2)}s` : "--";

  if (isLoading) {
    return (
      <div
        role="status"
        aria-label="Loading usage"
        className="flex min-h-24 items-center justify-center"
      >
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground/50" />
      </div>
    );
  }

  const hitPercent = Math.max(0, Math.min(100, hitRate * 100));
  const hitPercentLabel = hitPercent.toFixed(hitPercent >= 99.95 ? 0 : 1);
  const primaryMetrics: { label: string; value: string; title?: string }[] = [
    {
      label: t("usage.totalCost", "Total Cost"),
      value: fmtUsd(totalCost, 0),
    },
    {
      label: t("usage.realTotal", "Tokens Processed"),
      value: formatTokensShort(realTotal, 2),
      title: realTotal.toLocaleString("en-US"),
    },
    {
      label: t("usage.requests", "Requests"),
      value: requests.toLocaleString("en-US"),
    },
  ];

  return (
    <motion.div
      initial={{ opacity: 0, y: 5 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="min-w-0"
    >
      <Card
        role="region"
        aria-label={t("usage.summary", "Usage summary")}
        className="min-w-0"
      >
        <CardContent className="p-0">
          <h2 className="sr-only">{t("usage.summary", "Usage summary")}</h2>
          <dl className="grid grid-cols-[minmax(0,0.9fr)_minmax(0,1.2fr)_minmax(0,0.9fr)] divide-x divide-border border-b border-border px-1.5 py-3 sm:px-3 sm:py-4">
            {primaryMetrics.map(({ label, value, title }) => (
              <div key={label} className="min-w-0 px-1.5 sm:px-3">
                <dt className="mb-1 break-words text-xs leading-4 text-muted-foreground">
                  {label}
                </dt>
                <dd
                  title={title}
                  className="whitespace-nowrap text-lg font-medium leading-6 tabular-nums sm:text-2xl sm:leading-8"
                >
                  {value}
                </dd>
              </div>
            ))}
          </dl>
          <div className="grid min-w-0 grid-cols-1 gap-3 p-3 sm:p-4 md:grid-cols-[minmax(0,1.7fr)_minmax(0,1fr)] md:gap-4">
            <div
              role="group"
              aria-label={t("usage.tokenDetails", "Token details")}
              className="min-w-0"
            >
              <h3 className="mb-2 text-xs font-medium text-muted-foreground">
                {t("usage.tokenDetails", "Token details")}
              </h3>
              <dl className="grid min-w-0 grid-cols-2 gap-x-3 gap-y-2 md:grid-cols-4 md:gap-x-4">
                <SummaryRow
                  label={t("usage.freshInput", "Fresh Input")}
                  value={formatTokensShort(input)}
                />
                <SummaryRow
                  label={t("usage.output", "Output")}
                  value={formatTokensShort(output)}
                />
                <SummaryRow
                  label={t("usage.cacheRead", "Hit")}
                  value={formatTokensShort(cacheRead)}
                />
                <SummaryRow
                  label={t("usage.cacheHitRate", "Cache Hit Rate")}
                  value={`${hitPercentLabel}%`}
                  percentage
                />
              </dl>
            </div>
            <div
              role="group"
              aria-label={t("usage.requestDetails", "Request details")}
              className="min-w-0 border-t border-border pt-3 md:border-l md:border-t-0 md:pl-4 md:pt-0"
            >
              <h3 className="mb-2 text-xs font-medium text-muted-foreground">
                {t("usage.requestDetails", "Request details")}
              </h3>
              <dl className="grid min-w-0 grid-cols-2 gap-x-3 gap-y-2">
                <SummaryRow
                  label={t("usage.avgLatency", "Average Latency")}
                  value={averageLatency}
                />
                <SummaryRow
                  label={t("usage.successRate", "Success Rate")}
                  value={successRate}
                  percentage
                />
              </dl>
            </div>
          </div>
        </CardContent>
      </Card>
    </motion.div>
  );
}

interface SummaryRowProps {
  label: string;
  value: string;
  percentage?: boolean;
}

function SummaryRow({ label, value, percentage = false }: SummaryRowProps) {
  return (
    <div className="flex min-w-0 flex-col items-start gap-1">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd
        className={`text-sm font-medium tabular-nums ${
          percentage && value !== "--"
            ? "text-emerald-700 dark:text-emerald-400"
            : ""
        }`}
      >
        {value}
      </dd>
    </div>
  );
}
