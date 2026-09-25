import { useMutation } from "@tanstack/react-query";
import { Activity, Github, Loader2, Pencil } from "lucide-react";
import { toast } from "sonner";
import type { Provider } from "@/types";
import { useCopilotAuth } from "./forms/hooks/useCopilotAuth";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import CopilotQuotaFooter from "@/components/CopilotQuotaFooter";
import { streamCheckProvider } from "@/lib/api/connectivity-check";
import { extractErrorMessage } from "@/utils/errorUtils";

export function CopilotCard({
  provider,
  onEdit,
}: {
  provider: Provider;
  onEdit: () => void;
}) {
  const auth = useCopilotAuth();
  const healthCheck = useMutation({
    mutationFn: () => streamCheckProvider("codex", provider.id),
    onSuccess: (result) => {
      const details = [
        result.responseTimeMs != null ? `${result.responseTimeMs} ms` : "",
        result.httpStatus != null ? `HTTP ${result.httpStatus}` : "",
      ]
        .filter(Boolean)
        .join(" · ");
      const options = {
        description:
          result.status === "failed"
            ? result.message
            : [
                details,
                "Connectivity only; sign-in and model requests are not tested.",
              ]
                .filter(Boolean)
                .join(". "),
        duration: 8000,
        closeButton: true,
      };
      if (result.status === "failed") {
        toast.error("GitHub Copilot is unreachable", options);
      } else if (result.status === "degraded") {
        toast.warning("GitHub Copilot is reachable but slow", options);
      } else {
        toast.success("GitHub Copilot is reachable", options);
      }
    },
    onError: (error) =>
      toast.error("Health check failed", {
        description: extractErrorMessage(error) || String(error),
        duration: 8000,
        closeButton: true,
      }),
  });
  const accountId =
    provider.meta?.authBinding?.accountId ??
    provider.meta?.githubAccountId ??
    auth.defaultAccountId;
  const account = accountId
    ? auth.accounts.find((item) => item.id === accountId)
    : auth.accounts[0];
  const models =
    (provider.settingsConfig.modelCatalog as { models?: unknown[] } | undefined)
      ?.models ?? [];
  const needsSetup = !account || models.length === 0;
  return (
    <section className="space-y-5 rounded-xl border border-border bg-card p-6 shadow-sm">
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-4">
          <div className="rounded-xl bg-muted p-3">
            <Github className="h-7 w-7" />
          </div>
          <div className="space-y-1.5">
            <div className="flex items-center gap-3">
              <h2 className="text-lg font-semibold">GitHub Copilot</h2>
              {needsSetup && (
                <Badge variant="secondary">
                  {auth.isLoadingStatus ? "Checking account" : "Needs setup"}
                </Badge>
              )}
            </div>
            <p className="text-sm text-muted-foreground">
              {needsSetup
                ? "Edit to sign in, fetch your models, and save the bridge settings."
                : `${account.login} · ${models.length} models available to Codex`}
            </p>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            title="Health check"
            aria-label="Health check"
            aria-busy={healthCheck.isPending}
            disabled={healthCheck.isPending}
            onClick={() => healthCheck.mutate()}
          >
            {healthCheck.isPending ? (
              <Loader2 aria-hidden className="h-4 w-4 animate-spin" />
            ) : (
              <Activity aria-hidden className="h-4 w-4" />
            )}
          </Button>
          <Button onClick={onEdit}>
            <Pencil className="mr-2 h-4 w-4" />
            Edit
          </Button>
        </div>
      </div>
      {account && <CopilotQuotaFooter meta={provider.meta} />}
    </section>
  );
}
