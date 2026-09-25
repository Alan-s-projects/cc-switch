import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import type { ProviderFormProps } from "@/components/providers/forms/ProviderForm";

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    children,
  }: {
    isOpen: boolean;
    children: React.ReactNode;
  }) => (isOpen ? children : null),
}));
vi.mock("@/components/providers/AuthSettingsPanel", () => ({
  AuthSettingsPanel: () => null,
}));
vi.mock("@/components/providers/forms/ProviderForm", () => ({
  ProviderForm: ({ appId, onSubmit }: ProviderFormProps) => (
    <button
      onClick={() =>
        onSubmit({
          name: "GitHub Copilot",
          settingsConfig: '{"auth":{},"config":"stored template"}',
          meta: { providerType: "github_copilot" },
        })
      }
    >
      {appId}
    </button>
  ),
}));

describe("Copilot provider dialog", () => {
  it("creates a Codex Copilot provider without client configuration import", async () => {
    const submit = vi.fn().mockResolvedValue(undefined);
    const close = vi.fn();
    render(
      <AddProviderDialog
        open
        appId="codex"
        onSubmit={submit}
        onOpenChange={close}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "codex" }));
    await waitFor(() =>
      expect(submit).toHaveBeenCalledWith(
        expect.objectContaining({
          category: "third_party",
          meta: { providerType: "github_copilot" },
          settingsConfig: { auth: {}, config: "stored template" },
        }),
      ),
    );
    expect(close).toHaveBeenCalledWith(false);
  });
});
