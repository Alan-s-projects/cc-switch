import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import App from "@/App";
import { createTestQueryClient } from "../utils/testQueryClient";
const mocks = vi.hoisted(() => ({
  providers: vi.fn(() => ({
    data: {
      providers: {
        copilot: { id: "copilot", name: "GitHub Copilot", settingsConfig: {} },
      },
      currentProviderId: "copilot",
    },
    isLoading: false,
    refetch: vi.fn(),
  })),
}));
vi.mock("@/lib/query", () => ({
  useProvidersQuery: mocks.providers,
  useUpdateProviderMutation: () => ({ mutateAsync: vi.fn() }),
}));
vi.mock("@/lib/api", () => ({
  providersApi: {
    onSwitched: vi.fn().mockResolvedValue(() => {}),
    updateTrayMenu: vi.fn(),
  },
}));
vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({ isRunning: false, status: {} }),
}));
vi.mock("@/components/providers/CopilotCard", () => ({
  CopilotCard: () => <div>GitHub Copilot</div>,
}));
vi.mock("@/components/providers/EditProviderDialog", () => ({
  EditProviderDialog: () => null,
}));
vi.mock("@/components/proxy/ProxyToggle", () => ({ ProxyToggle: () => null }));
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
describe("Atlas application scope", () => {
  it("opens the one Copilot entry and ignores retired navigation preferences", () => {
    localStorage.setItem("cc-switch-last-app", "claude");
    localStorage.setItem("cc-switch-last-view", "mcp");
    renderApp();
    expect(screen.getByText("GitHub Copilot")).toBeVisible();
    expect(mocks.providers).toHaveBeenCalledWith("codex");
    for (const title of [
      "skills.manage",
      "mcp.title",
      "prompts.manage",
      "sessionManager.title",
    ]) {
      expect(screen.queryByTitle(title)).not.toBeInTheDocument();
    }
  });
  it("keeps usage directly accessible", async () => {
    renderApp();
    fireEvent.click(screen.getByTitle("usage.title"));
    expect(await screen.findByTestId("settings-tab")).toHaveTextContent(
      "usage",
    );
  });
  it("opens read-only connection previews", async () => {
    renderApp();
    fireEvent.click(screen.getByTitle("bridge.setup"));
    expect(await screen.findByText("read-only-suggestions")).toBeVisible();
  });
});
