import { useMutation } from "@tanstack/react-query";
import { Activity, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { streamCheckProvider } from "@/lib/api/connectivity-check";
import { extractErrorMessage } from "@/utils/errorUtils";

export function HealthCheckButton({ providerId }: { providerId?: string }) {
  const healthCheck = useMutation({
    mutationFn: (id: string) => streamCheckProvider("codex", id),
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

  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      aria-busy={healthCheck.isPending}
      disabled={!providerId || healthCheck.isPending}
      onClick={() => {
        if (providerId) healthCheck.mutate(providerId);
      }}
    >
      {healthCheck.isPending ? (
        <Loader2 aria-hidden className="mr-2 h-4 w-4 animate-spin" />
      ) : (
        <Activity aria-hidden className="mr-2 h-4 w-4" />
      )}
      Health check
    </Button>
  );
}
