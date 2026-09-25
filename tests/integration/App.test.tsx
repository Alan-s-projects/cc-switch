import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import App from "@/App";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({
  providers: vi.fn(() => ({
    data: { providers: {}, currentProviderId: "" },
    isLoading: false,
    refetch: vi.fn(),
  })),
}));
vi.mock("@/lib/query", () => ({
  useProvidersQuery: mocks.providers,
  useSettingsQuery: () => ({ data: {} }),
}));
vi.mock("@/lib/api", () => ({
  providersApi: {
    onSwitched: vi.fn().mockResolvedValue(() => {}),
    updateTrayMenu: vi.fn(),
  },
  settingsApi: { openExternal: vi.fn() },
}));
vi.mock("@/hooks/useProviderActions", () => ({
  useProviderActions: () => ({
    addProvider: vi.fn(),
    updateProvider: vi.fn(),
    switchProvider: vi.fn(),
    deleteProvider: vi.fn(),
    saveUsageScript: vi.fn(),
  }),
}));
vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({ isRunning: false, status: {} }),
}));
vi.mock("@/hooks/useUsageCacheBridge", () => ({
  useUsageCacheBridge: () => {},
}));
vi.mock("@/hooks/useTauriEvent", () => ({ useTauriEvent: () => {} }));
vi.mock("@/components/providers/ProviderList", () => ({
  ProviderList: ({ appId }: { appId: string }) => (
    <div data-testid="client">{appId}</div>
  ),
}));
vi.mock("@/components/providers/AddProviderDialog", () => ({
  AddProviderDialog: () => null,
}));
vi.mock("@/components/providers/EditProviderDialog", () => ({
  EditProviderDialog: () => null,
}));
vi.mock("@/components/UsageScriptModal", () => ({ default: () => null }));
vi.mock("@/components/UpdateBadge", () => ({ UpdateBadge: () => null }));
vi.mock("@/components/proxy/ProxyToggle", () => ({ ProxyToggle: () => null }));
vi.mock("@/components/proxy/FailoverToggle", () => ({
  FailoverToggle: () => null,
}));
vi.mock("@/components/proxy/RoutingActivationBrand", () => ({
  RoutingActivationBrand: () => null,
}));
vi.mock("@/components/settings/SettingsPage", () => ({
  SettingsPage: ({ defaultTab }: { defaultTab: string }) => (
    <div data-testid="settings-tab">{defaultTab}</div>
  ),
}));
vi.mock("@/components/providers/CodexSetupSuggestion", () => ({
  CodexSetupSuggestion: () => <div>read-only-suggestions</div>,
}));

function renderApp() {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <App />
    </QueryClientProvider>,
  );
}

describe("Codex Copilot application scope", () => {
  it("ignores retired client/view preferences and opens Codex", () => {
    localStorage.setItem("cc-switch-last-app", "claude");
    localStorage.setItem("cc-switch-last-view", "mcp");
    renderApp();
    expect(screen.getByTestId("client")).toHaveTextContent("codex");
    expect(mocks.providers).toHaveBeenCalledWith("codex", {
      isProxyRunning: false,
    });
    for (const title of [
      "skills.manage",
      "mcp.title",
      "prompts.manage",
      "sessionManager.title",
    ]) {
      expect(screen.queryByTitle(title)).not.toBeInTheDocument();
    }
  });

  it("keeps the usage dashboard directly accessible", async () => {
    renderApp();
    fireEvent.click(screen.getByTitle("usage.title"));
    expect(await screen.findByTestId("settings-tab")).toHaveTextContent(
      "usage",
    );
  });

  it("opens connection suggestions without entering a config editor", async () => {
    renderApp();
    fireEvent.click(screen.getByTitle("bridge.setup"));
    await waitFor(() =>
      expect(screen.getByText("read-only-suggestions")).toBeVisible(),
    );
  });
});
