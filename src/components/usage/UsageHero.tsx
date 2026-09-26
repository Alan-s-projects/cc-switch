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
        className="flex min-h-80 items-center justify-center"
      >
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground/50" />
      </div>
    );
  }

  const hitPercent = Math.max(0, Math.min(100, hitRate * 100));
  const hitPercentLabel = hitPercent.toFixed(hitPercent >= 99.95 ? 0 : 1);

  return (
    <motion.div
      initial={{ opacity: 0, y: 5 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="grid gap-3.5 md:grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)]"
    >
      <Card
        role="region"
        aria-label={t("usage.realTotal", "Tokens Processed")}
        className="min-w-0"
      >
        <CardContent className="p-5">
          <h2 className="mb-3 text-sm font-medium">
            {t("usage.realTotal", "Tokens Processed")}
          </h2>
          <p
            className="text-[28px] font-medium leading-9 tabular-nums"
            title={realTotal.toLocaleString("en-US")}
          >
            {formatTokensShort(realTotal, 2)}
          </p>
          <dl className="mt-6 grid gap-4 border-t border-border pt-5">
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
        </CardContent>
      </Card>
      <div className="grid content-start gap-3.5">
        <Card
          role="region"
          aria-label={t("usage.requests", "Requests")}
          className="min-w-0"
        >
          <CardContent className="p-5">
            <h2 className="mb-3 text-sm font-medium">
              {t("usage.requests", "Requests")}
            </h2>
            <p className="text-[28px] font-medium leading-9 tabular-nums">
              {requests.toLocaleString("en-US")}
            </p>
            <dl className="mt-5 grid gap-3 border-t border-border pt-4">
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
          </CardContent>
        </Card>
        <Card
          role="region"
          aria-label={t("usage.totalCost", "Total Cost")}
          className="min-w-0"
        >
          <CardContent className="p-5">
            <h2 className="mb-3 text-sm font-medium">
              {t("usage.totalCost", "Total Cost")}
            </h2>
            <p className="text-[28px] font-medium leading-9 tabular-nums">
              {fmtUsd(totalCost, 0)}
            </p>
            <p className="mt-1.5 text-xs text-muted-foreground">USD</p>
          </CardContent>
        </Card>
      </div>
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
