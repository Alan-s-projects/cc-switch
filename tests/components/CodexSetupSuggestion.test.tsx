import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexSetupSuggestion } from "@/components/providers/CodexSetupSuggestion";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), copy: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@/lib/clipboard", () => ({ copyText: mocks.copy }));

const suggestion =
  'model_provider = "copilot"\n[model_providers.copilot]\nbase_url = "http://127.0.0.1:15721/v1"\n';
const renderPanel = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <CodexSetupSuggestion />
    </QueryClientProvider>,
  );

describe("read-only Codex connection suggestions", () => {
  beforeEach(() => {
    mocks.invoke.mockReset().mockResolvedValue({
      configPath: "C:/Users/test/.codex/config.toml",
      suggestion,
      endpoint: "http://127.0.0.1:15721/v1",
      configured: false,
      currentProvider: "Copilot Bridge",
      copilotConfig: suggestion,
      copilotDiff:
        "--- a/config.toml\n+++ b/config.toml\n-old proxy\n+Atlas proxy\n",
      openaiConfig:
        'model_provider = "openai"\nforced_login_method = "chatgpt"\n',
      openaiDiff: "--- a/config.toml\n+++ b/config.toml\n-old proxy\n+OpenAI\n",
    });
    mocks.copy.mockReset().mockResolvedValue(undefined);
  });

  it("reads suggestions and copies them without invoking a writer", async () => {
    renderPanel();
    expect(
      await screen.findByText("C:/Users/test/.codex/config.toml"),
    ).toBeVisible();
    expect(
      screen.getByText(/Atlas reads your configuration and never writes it/),
    ).toBeVisible();
    expect(screen.getByLabelText("Configuration diff")).toHaveTextContent(
      "+Atlas proxy",
    );
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy proposed TOML" }));
    await waitFor(() => expect(mocks.copy).toHaveBeenCalledWith(suggestion));
    fireEvent.click(
      screen.getByRole("button", { name: "Return to OpenAI sign-in" }),
    );
    expect(screen.getByLabelText("Configuration diff")).toHaveTextContent(
      "+OpenAI",
    );
    fireEvent.click(screen.getByRole("button", { name: "Proposed TOML" }));
    expect(screen.getByLabelText("Proposed TOML")).toHaveTextContent(
      'model_provider = "openai"',
    );
    fireEvent.click(screen.getByRole("button", { name: "Copy proposed TOML" }));
    await waitFor(() =>
      expect(mocks.copy).toHaveBeenLastCalledWith(
        'model_provider = "openai"\nforced_login_method = "chatgpt"\n',
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() =>
      expect(mocks.invoke.mock.calls.length).toBeGreaterThan(1),
    );
    expect(
      mocks.invoke.mock.calls.every(
        ([command]) => command === "get_codex_setup_suggestion",
      ),
    ).toBe(true);
  });

  it("reports invalid configuration without offering a write or repair action", async () => {
    mocks.invoke.mockRejectedValue(new Error("Codex config.toml is invalid"));
    renderPanel();
    expect(await screen.findByRole("alert")).toHaveTextContent("invalid");
    expect(
      screen.queryByRole("button", { name: "Copy proposed TOML" }),
    ).not.toBeInTheDocument();
    expect(mocks.copy).not.toHaveBeenCalled();
    expect(mocks.invoke).toHaveBeenCalledWith("get_codex_setup_suggestion");
  });
});
