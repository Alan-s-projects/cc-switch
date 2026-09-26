import { Github } from "lucide-react";
import type { Provider } from "@/types";
import { useCopilotAuth } from "./forms/hooks/useCopilotAuth";
import { Badge } from "@/components/ui/badge";
import CopilotQuotaFooter from "@/components/CopilotQuotaFooter";
import { HealthCheckButton } from "./HealthCheckButton";

export function CopilotCard({ provider }: { provider: Provider }) {
  const auth = useCopilotAuth();
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
  const enabledModelCount = models.filter(
    (model) =>
      !model ||
      typeof model !== "object" ||
      (model as { enabled?: unknown }).enabled !== false,
  ).length;
  const needsSetup = !account || enabledModelCount === 0;
  return (
    <section className="space-y-5 rounded-xl border border-border bg-card p-6 shadow-sm">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h2 className="text-base font-semibold">Provider</h2>
        <HealthCheckButton providerId={provider.id} />
      </div>
      <div className="flex items-center gap-4">
        <div className="rounded-xl bg-muted p-3">
          <Github className="h-7 w-7" />
        </div>
        <div className="space-y-1.5">
          <div className="flex items-center gap-3">
            <h3 className="text-lg font-semibold">GitHub Copilot</h3>
            {needsSetup && (
              <Badge variant="secondary">
                {auth.isLoadingStatus ? "Checking account" : "Needs setup"}
              </Badge>
            )}
          </div>
          <p className="text-sm text-muted-foreground">
            {!account
              ? "GitHub Copilot is signed out."
              : enabledModelCount === 0
                ? "No models are enabled for Codex. Open Settings → Copilot to enable at least one."
                : `${account.login} · ${enabledModelCount} models available to Codex`}
          </p>
        </div>
      </div>
      {account && <CopilotQuotaFooter meta={provider.meta} />}
    </section>
  );
}
