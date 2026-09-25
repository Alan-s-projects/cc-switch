import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { EditProviderDialog } from "@/components/providers/EditProviderDialog";
import type { ProviderFormProps } from "@/components/providers/forms/ProviderForm";
import type { Provider } from "@/types";

const mocks = vi.hoisted(() => ({ readLive: vi.fn() }));
vi.mock("@/lib/api", () => ({
  providersApi: { readLiveSettings: mocks.readLive },
}));
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
  AuthSettingsPanel: ({ target }: { target: string | null }) =>
    target ? <div>account-panel</div> : null,
}));
vi.mock("@/components/providers/forms/ProviderForm", () => ({
  ProviderForm: ({
    initialData,
    onSubmit,
    onManageAuthAccounts,
  }: ProviderFormProps) => (
    <>
      <output data-testid="settings">
        {JSON.stringify(initialData?.settingsConfig)}
      </output>
      <button onClick={() => onManageAuthAccounts?.("github_copilot")}>
        accounts
      </button>
      <button
        onClick={() =>
          onSubmit({
            name: "Renamed",
            settingsConfig: JSON.stringify(initialData?.settingsConfig),
            meta: initialData?.meta,
          })
        }
      >
        save
      </button>
    </>
  ),
}));

const provider: Provider = {
  id: "existing-copilot",
  name: "Copilot",
  settingsConfig: {
    config: 'model_provider = "custom"\n# stored template',
    modelCatalog: {
      models: [{ model: "gpt-6-astra", inputModalities: ["text", "image"] }],
    },
  },
  meta: { providerType: "github_copilot", githubAccountId: "account-a" },
};

describe("Copilot provider editing", () => {
  it("preserves stored models and account binding without importing live configuration", async () => {
    const submit = vi.fn().mockResolvedValue(undefined);
    const close = vi.fn();
    render(
      <EditProviderDialog
        open
        provider={provider}
        appId="codex"
        onSubmit={submit}
        onOpenChange={close}
      />,
    );
    expect(screen.getByTestId("settings")).toHaveTextContent("gpt-6-astra");
    fireEvent.click(screen.getByRole("button", { name: "save" }));
    await waitFor(() =>
      expect(submit).toHaveBeenCalledWith({
        provider: { ...provider, name: "Renamed", notes: undefined },
      }),
    );
    expect(mocks.readLive).not.toHaveBeenCalled();
    expect(close).toHaveBeenCalledWith(false);
  });

  it("closes account management when the provider dialog closes externally", async () => {
    const props = {
      provider,
      appId: "codex" as const,
      onSubmit: vi.fn(),
      onOpenChange: vi.fn(),
    };
    const { rerender } = render(<EditProviderDialog {...props} open />);
    fireEvent.click(screen.getByRole("button", { name: "accounts" }));
    expect(screen.getByText("account-panel")).toBeVisible();
    rerender(<EditProviderDialog {...props} open={false} />);
    await waitFor(() =>
      expect(screen.queryByText("account-panel")).not.toBeInTheDocument(),
    );
    rerender(<EditProviderDialog {...props} open />);
    expect(screen.queryByText("account-panel")).not.toBeInTheDocument();
  });
});
