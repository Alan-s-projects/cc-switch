import { Github, Pencil } from "lucide-react";
import type { Provider } from "@/types";
import { useCopilotAuth } from "./forms/hooks/useCopilotAuth";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import CopilotQuotaFooter from "@/components/CopilotQuotaFooter";

export function CopilotCard({
  provider,
  onEdit,
}: {
  provider: Provider;
  onEdit: () => void;
}) {
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
        <Button onClick={onEdit}>
          <Pencil className="mr-2 h-4 w-4" />
          Edit
        </Button>
      </div>
      {account && <CopilotQuotaFooter meta={provider.meta} />}
    </section>
  );
}
