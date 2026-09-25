import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { Copy, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { copyText } from "@/lib/clipboard";

interface SetupSuggestion {
  configPath: string;
  suggestion: string;
  endpoint: string;
  configured: boolean;
}

export function CodexSetupSuggestion() {
  const { t } = useTranslation();
  const { data, error, isFetching, refetch } = useQuery({
    queryKey: ["codex-setup-suggestion"],
    queryFn: () => invoke<SetupSuggestion>("get_codex_setup_suggestion"),
    staleTime: 0,
  });
  return (
    <section className="space-y-4 overflow-y-auto px-6 pb-6">
      <p className="text-sm text-muted-foreground">{t("bridge.readOnly")}</p>
      {error && (
        <p role="alert" className="text-destructive">
          {String(error)}
        </p>
      )}
      {data && (
        <>
          <p className="break-all text-sm">{data.configPath}</p>
          <p className="text-sm">
            {data.configured
              ? t("bridge.connectedConfig")
              : t("bridge.manualSetup")}
          </p>
          <p className="text-sm text-muted-foreground">
            {t("bridge.mergeHint")}
          </p>
          <pre
            aria-label={t("bridge.suggestion")}
            className="overflow-auto rounded-lg border bg-muted/40 p-4 text-sm"
          >
            {data.suggestion}
          </pre>
          <Button
            onClick={async () => {
              try {
                await copyText(data.suggestion);
                toast.success(t("common.copied"));
              } catch (error) {
                toast.error(String(error));
              }
            }}
          >
            <Copy className="mr-2 h-4 w-4" />
            {t("bridge.copySuggestion")}
          </Button>
        </>
      )}
      <Button
        className="ml-2"
        variant="outline"
        disabled={isFetching}
        onClick={() => void refetch()}
      >
        <RefreshCw
          className={`mr-2 h-4 w-4 ${isFetching ? "animate-spin" : ""}`}
        />
        {t("common.refresh")}
      </Button>
    </section>
  );
}
