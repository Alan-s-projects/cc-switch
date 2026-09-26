import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import { Card, CardContent } from "@/components/ui/card";
import { useUsageSummary } from "@/lib/query/usage";
import { cn } from "@/lib/utils";
import { CodexIcon } from "@/components/BrandIcons";
import {
  Activity,
  ArrowDownToLine,
  ArrowUpFromLine,
  Database,
  Info,
  Loader2,
  Sparkles,
} from "lucide-react";
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

  const titleTheme = {
    accent: "text-neutral-700 dark:text-neutral-300",
    iconBg: "bg-neutral-500/10",
  };
  const appLabel = "Codex";

  const input = summary?.totalInputTokens ?? 0;
  const output = summary?.totalOutputTokens ?? 0;
  const cacheRead = summary?.totalCacheReadTokens ?? 0;
  const realTotal = summary?.realTotalTokens ?? 0;
  const hitRate = summary?.cacheHitRate ?? 0;
  const totalCost = parseFiniteNumber(summary?.totalCost);
  const requests = summary?.totalRequests ?? 0;

  const cacheWriteDisplay = {
    value: "N/A",
    muted: true,
    tooltip: t("usage.cacheWriteNotReported"),
  };

  if (isLoading) {
    return (
      <Card className="border border-border/50 bg-card/40 backdrop-blur-sm">
        <CardContent className="flex items-center justify-center min-h-[200px]">
          <Loader2 className="h-6 w-6 animate-spin text-muted-foreground/50" />
        </CardContent>
      </Card>
    );
  }

  const hitPercent = Math.max(0, Math.min(100, hitRate * 100));
  const hitPercentLabel = hitPercent.toFixed(hitPercent >= 99.95 ? 0 : 1);

  return (
    <motion.div
      initial={{ opacity: 0, y: 5 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
    >
      <Card className="relative overflow-hidden border border-border/50 bg-card/60 backdrop-blur-xl shadow-sm">
        <CardContent className="p-4 md:p-5">
          <div className="flex flex-col gap-4">
            {/* Top row: Main Token Count, Requests, Cost */}
            <div className="flex flex-col md:flex-row md:items-center justify-between gap-4">
              <div className="flex items-center gap-3">
                <div
                  className={cn(
                    "p-2.5 rounded-xl bg-gradient-to-br shadow-sm",
                    titleTheme.iconBg,
                  )}
                >
                  <CodexIcon size={20} />
                </div>
                <div>
                  <div className="text-xs font-medium text-muted-foreground flex items-center gap-1.5 mb-0.5">
                    {appLabel && (
                      <>
                        <span
                          className={cn("font-semibold", titleTheme.accent)}
                        >
                          {appLabel}
                        </span>
                        <span className="text-muted-foreground/30">•</span>
                      </>
                    )}
                    {t("usage.realTotal", "Tokens Processed")}
                  </div>
                  <div className="flex items-baseline gap-2">
                    <span
                      className="text-2xl md:text-3xl font-bold tabular-nums tracking-tight leading-none"
                      title={realTotal.toLocaleString()}
                    >
                      {realTotal.toLocaleString()}
                    </span>
                    <span className="text-xs text-muted-foreground font-medium bg-muted/40 px-1.5 py-0.5 rounded-md">
                      ≈ {formatTokensShort(realTotal, 2)}
                    </span>
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-5 bg-background/50 px-4 py-2.5 rounded-xl border border-border/40 shadow-sm">
                <div className="flex flex-col">
                  <span className="text-[10px] text-muted-foreground uppercase tracking-wider font-medium">
                    {t("usage.totalRequests")}
                  </span>
                  <span className="font-semibold flex items-center gap-1.5 text-sm tabular-nums">
                    <Activity className="h-3.5 w-3.5 text-blue-500" />
                    {requests.toLocaleString()}
                  </span>
                </div>
                <div className="w-px h-8 bg-border/60" />
                <div className="flex flex-col">
                  <span className="text-[10px] text-muted-foreground uppercase tracking-wider font-medium">
                    {t("usage.totalCost")}
                  </span>
                  <span className="font-semibold text-green-500 text-sm tabular-nums">
                    {totalCost == null ? "--" : fmtUsd(totalCost, 4)}
                  </span>
                </div>
              </div>
            </div>

            {/* Bottom row: Breakdown and Hit Rate */}
            <div className="grid grid-cols-2 lg:grid-cols-5 gap-3">
              <MiniStat
                icon={<ArrowDownToLine className="h-3.5 w-3.5" />}
                label={t("usage.freshInput", "Fresh Input")}
                value={formatTokensShort(input)}
                accent="text-blue-500"
              />
              <MiniStat
                icon={<ArrowUpFromLine className="h-3.5 w-3.5" />}
                label={t("usage.output")}
                value={formatTokensShort(output)}
                accent="text-purple-500"
              />
              <MiniStat
                icon={<Database className="h-3.5 w-3.5" />}
                label={t("usage.cacheWrite", "Creation")}
                value={cacheWriteDisplay.value}
                accent="text-amber-500"
                muted={cacheWriteDisplay.muted}
                tooltip={cacheWriteDisplay.tooltip}
              />
              <MiniStat
                icon={<Sparkles className="h-3.5 w-3.5" />}
                label={t("usage.cacheRead", "Hit")}
                value={formatTokensShort(cacheRead)}
                accent="text-emerald-500"
              />

              <div className="col-span-2 lg:col-span-1 flex flex-col justify-center rounded-xl border border-border/40 bg-background/40 p-3 shadow-sm">
                <div className="flex items-center justify-between text-[11px] mb-2">
                  <span className="text-muted-foreground font-medium">
                    {t("usage.cacheHitRate", "Cache Hit Rate")}
                  </span>
                  <span className="font-bold text-emerald-500 tabular-nums">
                    {hitPercentLabel}%
                  </span>
                </div>
                <div className="relative h-1.5 rounded-full bg-muted/60 overflow-hidden">
                  <motion.div
                    className="absolute inset-y-0 left-0 bg-emerald-500 rounded-full"
                    initial={{ width: 0 }}
                    animate={{ width: `${hitPercent}%` }}
                    transition={{ duration: 0.8, ease: "easeOut" }}
                  />
                </div>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>
    </motion.div>
  );
}

interface MiniStatProps {
  icon: React.ReactNode;
  label: string;
  value: string;
  accent: string;
  /** Optional hover tooltip — used to flag protocol-level caveats. */
  tooltip?: string;
  /** Visually de-emphasize the value (e.g. for "N/A" cases). */
  muted?: boolean;
}

function MiniStat({
  icon,
  label,
  value,
  accent,
  tooltip,
  muted,
}: MiniStatProps) {
  return (
    <div
      className="flex flex-col gap-1 rounded-xl border border-border/40 bg-background/40 p-3 shadow-sm"
      title={tooltip}
    >
      <div
        className={`flex items-center gap-1.5 text-[11px] font-medium text-muted-foreground ${accent}`}
      >
        {icon}
        <span className="text-foreground/70 tracking-wide">{label}</span>
        {tooltip && (
          <Info className="h-3 w-3 text-muted-foreground/60 shrink-0 ml-auto" />
        )}
      </div>
      <div
        className={cn(
          "text-sm font-semibold tabular-nums",
          muted && "text-muted-foreground/70",
        )}
      >
        {value}
      </div>
    </div>
  );
}
