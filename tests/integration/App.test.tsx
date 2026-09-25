import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import App from "@/App";
import { createTestQueryClient } from "../utils/testQueryClient";
const mocks = vi.hoisted(() => ({
  autoSaveSettings: vi.fn().mockResolvedValue({ requiresRestart: false }),
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
vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => ({
    settings: { usageDashboardRefreshIntervalMs: 10000 },
    isLoading: false,
    autoSaveSettings: mocks.autoSaveSettings,
  }),
}));
vi.mock("@/components/providers/CopilotCard", () => ({
  CopilotCard: () => <div>GitHub Copilot</div>,
}));
vi.mock("@/components/overview/BridgeOverview", () => ({
  BridgeOverview: () => <div data-testid="bridge-overview">Overview</div>,
}));
vi.mock("@/components/providers/HealthCheckButton", () => ({
  HealthCheckButton: ({ providerId }: { providerId?: string }) => (
    <button disabled={!providerId}>Health check</button>
  ),
}));
vi.mock("@/components/proxy/ProxyToggle", () => ({ ProxyToggle: () => null }));
vi.mock("@/components/proxy/RoutingActivationBrand", () => ({
  RoutingActivationBrand: () => null,
}));
vi.mock("@/components/settings/SettingsPage", () => ({
  SettingsPage: () => <div data-testid="settings-page">settings</div>,
}));
vi.mock("@/components/usage/UsageDashboard", () => ({
  UsageDashboard: ({
    refreshIntervalMs,
    onRefreshIntervalChange,
  }: {
    refreshIntervalMs: number;
    onRefreshIntervalChange: (next: number) => Promise<boolean>;
  }) => (
    <div data-testid="usage-dashboard">
      Refresh every {refreshIntervalMs}
      <button onClick={() => void onRefreshIntervalChange(5000)}>
        Change refresh interval
      </button>
    </div>
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
    expect(screen.getByTestId("bridge-overview")).toBeVisible();
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
  it("opens usage as its own page, preserves its refresh preference, and navigates through settings and back", async () => {
    renderApp();
    fireEvent.click(screen.getByTitle("usage.title"));
    expect(await screen.findByTestId("usage-dashboard")).toHaveTextContent(
      "Refresh every 10000",
    );
    expect(
      screen.getByRole("heading", { level: 1, name: "usage.title" }),
    ).toBeVisible();
    expect(screen.queryByTestId("settings-page")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Change refresh interval" }),
    );
    await waitFor(() =>
      expect(mocks.autoSaveSettings).toHaveBeenCalledWith({
        usageDashboardRefreshIntervalMs: 5000,
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "common.back" }));
    expect(screen.getByText("GitHub Copilot")).toBeVisible();
    expect(screen.queryByTestId("usage-dashboard")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTitle("usage.title"));
    expect(screen.getByTestId("usage-dashboard")).toBeVisible();
    fireEvent.click(screen.getByTitle("common.settings"));
    expect(screen.getByTestId("settings-page")).toBeVisible();
    expect(
      screen.getByRole("heading", { level: 1, name: "settings.title" }),
    ).toBeVisible();
    expect(screen.queryByTestId("usage-dashboard")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.back" }));
    expect(screen.getByText("GitHub Copilot")).toBeVisible();
  });
  it("opens read-only connection previews from the labeled header action", async () => {
    renderApp();
    expect(screen.getByRole("button", { name: "Usage" })).toBeVisible();
    expect(
      screen.getByRole("button", { name: "common.settings" }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Health check" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    expect(await screen.findByText("read-only-suggestions")).toBeVisible();
  });
});
