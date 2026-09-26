import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { toast } from "sonner";
import { Check, Loader2, Search } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { useUpdateModelPricing } from "@/lib/query/usage";
import {
  fetchModelsDevPricing,
  flattenModels,
  formatPrice,
  MODELS_DEV_QUERY_KEY,
  MODELS_DEV_STALE_TIME_MS,
  type ModelsDevEntry,
} from "@/lib/modelsDevPricing";
import { isTextEditableTarget } from "@/utils/domUtils";

export {
  flattenModels,
  formatPrice,
  normalizeModelIdForPricing,
} from "@/lib/modelsDevPricing";

// Bound the rendered list; search includes every available GPT model.
const DEFAULT_VISIBLE_ROWS = 50;
const MAX_VISIBLE_ROWS = 200;

interface ModelsDevPickerDialogProps {
  open: boolean;
  onClose: () => void;
  /** 导入成功后调用（此时定价列表已刷新） */
  onImported: () => void;
}

export function ModelsDevPickerDialog({
  open,
  onClose,
  onImported,
}: ModelsDevPickerDialogProps) {
  const { t } = useTranslation();
  const updatePricing = useUpdateModelPricing();

  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<ModelsDevEntry | null>(null);

  // 每次打开时重置选择与过滤条件
  useEffect(() => {
    if (open) {
      setSearch("");
      setSelected(null);
    }
  }, [open]);

  const { data, isLoading, error, refetch } = useQuery({
    queryKey: MODELS_DEV_QUERY_KEY,
    queryFn: fetchModelsDevPricing,
    enabled: open,
    staleTime: MODELS_DEV_STALE_TIME_MS,
    retry: 1,
  });

  const entries = useMemo(() => (data ? flattenModels(data) : []), [data]);

  const isFiltering = search.trim() !== "";

  const filtered = useMemo(() => {
    const query = search.trim().toLowerCase();
    return entries.filter(
      (entry) =>
        !query ||
        entry.modelId.toLowerCase().includes(query) ||
        entry.normalizedId.includes(query) ||
        entry.modelName.toLowerCase().includes(query) ||
        entry.providerName.toLowerCase().includes(query),
    );
  }, [entries, search]);

  // 默认只展示最新发布的一批，搜索/筛选时展示全量匹配（设上限防卡顿）
  const visible = useMemo(
    () =>
      filtered.slice(0, isFiltering ? MAX_VISIBLE_ROWS : DEFAULT_VISIBLE_ROWS),
    [filtered, isFiltering],
  );

  // 单选：点击未选中的行替换选择，点击已选中的行取消选择。
  // 限制单选是为了避免批量导入时每条都触发一次全量零成本回填扫描（见 update_model_pricing）。
  const toggleEntry = (entry: ModelsDevEntry) => {
    setSelected((prev) => (prev?.key === entry.key ? null : entry));
  };

  const handleImport = async () => {
    if (!selected) return;

    try {
      await updatePricing.mutateAsync({
        modelId: selected.normalizedId,
        displayName: selected.modelName,
        inputCost: formatPrice(selected.input),
        outputCost: formatPrice(selected.output),
        cacheReadCost: formatPrice(selected.cacheRead),
        cacheCreationCost: formatPrice(selected.cacheWrite),
      });

      toast.success(
        t("usage.modelsDevImported", {
          name: selected.modelName,
          defaultValue: "Imported pricing for {{name}}",
        }),
        { closeButton: true },
      );
      onImported();
    } catch (error) {
      toast.error(String(error));
    }
  };

  const priceColumns = (entry: ModelsDevEntry) =>
    [
      { label: t("usage.inputCost", "Input Cost"), value: entry.input },
      { label: t("usage.outputCost", "Output Cost"), value: entry.output },
      { label: t("usage.cacheReadCost", "Cache Hit"), value: entry.cacheRead },
      {
        label: t("usage.cacheWriteCost", "Cache Creation"),
        value: entry.cacheWrite,
      },
    ] as const;

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !updatePricing.isPending) {
          onClose();
        }
      }}
    >
      <DialogContent
        zIndex="top"
        className="max-w-3xl h-[80vh]"
        onEscapeKeyDown={(e) => {
          // 在搜索框里按 ESC 不应关闭弹窗丢掉已选模型（与 FullScreenPanel 的约定一致）
          if (isTextEditableTarget(e.target)) {
            e.preventDefault();
          }
        }}
      >
        <DialogHeader>
          <DialogTitle>
            {t("usage.modelsDevPickerTitle", "Import Pricing from models.dev")}
          </DialogTitle>
          <DialogDescription>
            {t(
              "usage.modelsDevPickerDesc",
              "Select a model to import (prices in USD per million tokens). One model per import.",
            )}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-1 min-h-0 flex-col gap-3 px-6 py-4">
          {isLoading ? (
            <div className="flex flex-1 items-center justify-center">
              <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
            </div>
          ) : error ? (
            <Alert variant="destructive">
              <AlertDescription className="flex items-center justify-between gap-3">
                <span>
                  {t(
                    "usage.modelsDevLoadError",
                    "Failed to load models.dev data",
                  )}
                  : {error instanceof Error ? error.message : String(error)}
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => refetch()}
                  className="shrink-0"
                >
                  {t("usage.modelsDevRetry", "Retry")}
                </Button>
              </AlertDescription>
            </Alert>
          ) : (
            <>
              <div className="flex items-center gap-2">
                <div className="relative flex-1">
                  <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                    placeholder={t(
                      "usage.modelsDevSearchPlaceholder",
                      "Search GPT models...",
                    )}
                    className="pl-8"
                  />
                </div>
              </div>

              <div className="flex-1 min-h-0 overflow-y-auto rounded-md border border-border/50">
                {filtered.length === 0 ? (
                  <div className="flex h-full items-center justify-center py-8 text-sm text-muted-foreground">
                    {t("usage.modelsDevNoResults", "No matching models")}
                  </div>
                ) : (
                  <div className="divide-y divide-border/30">
                    {visible.map((entry) => (
                      <div
                        key={entry.key}
                        role="button"
                        aria-pressed={selected?.key === entry.key}
                        onClick={() => toggleEntry(entry)}
                        className={`flex cursor-pointer items-center gap-3 px-3 py-2 ${
                          selected?.key === entry.key
                            ? "bg-accent/50"
                            : "hover:bg-muted/40"
                        }`}
                      >
                        <Check
                          className={`h-4 w-4 shrink-0 text-primary ${
                            selected?.key === entry.key
                              ? "visible"
                              : "invisible"
                          }`}
                        />
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            <span className="truncate text-sm font-medium">
                              {entry.modelName}
                            </span>
                            <span className="shrink-0 text-xs text-muted-foreground">
                              {entry.providerName}
                            </span>
                            {entry.releaseDate && (
                              <span className="shrink-0 text-[10px] text-muted-foreground/70">
                                {entry.releaseDate}
                              </span>
                            )}
                          </div>
                          <div
                            className="truncate font-mono text-xs text-muted-foreground"
                            title={entry.modelId}
                          >
                            {entry.normalizedId}
                          </div>
                        </div>
                        <div className="flex shrink-0 gap-3 text-right">
                          {priceColumns(entry).map((column) => (
                            <div key={column.label} className="w-16">
                              <div className="text-[10px] text-muted-foreground">
                                {column.label}
                              </div>
                              <div className="font-mono text-xs">
                                ${formatPrice(column.value)}
                              </div>
                            </div>
                          ))}
                        </div>
                      </div>
                    ))}
                    {filtered.length > visible.length && (
                      <div className="px-3 py-2 text-center text-xs text-muted-foreground">
                        {isFiltering
                          ? t("usage.modelsDevTruncated", {
                              shown: visible.length,
                              total: filtered.length,
                              defaultValue:
                                "Showing first {{shown}} of {{total}} results — refine your search",
                            })
                          : t("usage.modelsDevDefaultHint", {
                              shown: visible.length,
                              total: filtered.length,
                              defaultValue:
                                "Showing the {{shown}} most recently released models (of {{total}}) — type to search all",
                            })}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </>
          )}
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={onClose}
            disabled={updatePricing.isPending}
          >
            {t("common.cancel", "Cancel")}
          </Button>
          <Button
            onClick={handleImport}
            disabled={!selected || updatePricing.isPending}
          >
            {updatePricing.isPending ? (
              <>
                <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
                {t("usage.modelsDevImporting", "Importing...")}
              </>
            ) : (
              t("usage.modelsDevImportButton", "Import")
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
