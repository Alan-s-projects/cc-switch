import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexSetupSuggestion } from "@/components/providers/CodexSetupSuggestion";
import type { ConfigDiffLine } from "@/components/providers/ConfigDiff";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), copy: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@/lib/clipboard", () => ({ copyText: mocks.copy }));

const oldUrl = 'base_url = "http://127.0.0.1:4142/v1"';
const newUrl = 'base_url = "http://127.0.0.1:15721/v1"';
const currentLines = [
  'model_provider = "copilot-bridge"',
  "model_auto_compact_token_limit = 900000",
  "",
  "[model_providers.copilot-bridge]",
  'name = "Copilot Bridge"',
  oldUrl,
  'env_key = "COPILOT_BRIDGE_TOKEN"',
  'wire_api = "responses"',
];
const currentConfig = currentLines.join("\n") + "\n";
const context = (
  oldLineNumber: number,
  newLineNumber = oldLineNumber,
): ConfigDiffLine => ({
  kind: "context",
  oldLineNumber,
  newLineNumber,
  text: currentLines[oldLineNumber - 1] + "\n",
});
const removed = (oldLineNumber: number): ConfigDiffLine => ({
  kind: "removed",
  oldLineNumber,
  newLineNumber: null,
  text: currentLines[oldLineNumber - 1] + "\n",
});
const added = (newLineNumber: number, text: string): ConfigDiffLine => ({
  kind: "added",
  oldLineNumber: null,
  newLineNumber,
  text: text + "\n",
});
const copilotLines = [
  context(1),
  context(2),
  context(3),
  context(4),
  removed(5),
  removed(6),
  removed(7),
  added(5, 'name = "GitHub Copilot"'),
  added(6, newUrl),
  context(8, 7),
  added(8, "requires_openai_auth = false"),
  added(9, 'experimental_bearer_token = "PROXY_MANAGED"'),
  added(10, "supports_websockets = false"),
];
const openaiLines = [
  removed(1),
  added(1, 'model_provider = "openai"'),
  context(2),
  added(3, 'forced_login_method = "chatgpt"'),
  ...[3, 4, 5, 6, 7, 8].map((number) => context(number, number + 1)),
];
const proposedText = (lines: ConfigDiffLine[]) =>
  lines
    .filter((line) => line.kind !== "removed")
    .map((line) => line.text)
    .join("");
const unifiedDiff = (lines: ConfigDiffLine[], newCount: number) =>
  `--- a/config.toml\n+++ b/config.toml\n@@ -1,8 +1,${newCount} @@\n` +
  lines
    .map(
      (line) =>
        ({ context: " ", removed: "-", added: "+" })[line.kind] + line.text,
    )
    .join("");
const preview = {
  configPath: "C:/Users/test/.codex/config.toml",
  endpoint: "http://127.0.0.1:15721/v1",
  currentProvider: "Copilot Bridge",
  copilotConfig: proposedText(copilotLines),
  copilotDiff: unifiedDiff(copilotLines, 10),
  copilotLines,
  openaiConfig: proposedText(openaiLines),
  openaiDiff: unifiedDiff(openaiLines, 9),
  openaiLines,
};
const renderPanel = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <CodexSetupSuggestion />
    </QueryClientProvider>,
  );
const comparison = () =>
  screen.getByRole("region", { name: "Configuration diff" });
const rowFor = (text: string) => {
  const row = within(comparison()).getByText(text).closest("tr");
  expect(row).not.toBeNull();
  return within(row!);
};

describe("read-only Codex connection suggestions", () => {
  beforeEach(() => {
    mocks.invoke.mockReset().mockResolvedValue(preview);
    mocks.copy.mockReset().mockResolvedValue(undefined);
  });

  it("defaults to current TOML on the left and proposed TOML on the right", async () => {
    renderPanel();
    expect(
      await screen.findByRole("button", { name: "Side by side" }),
    ).toHaveAttribute("aria-pressed", "true");
    const headings = within(comparison()).getAllByRole("columnheader");
    expect(headings[0]).toHaveTextContent("Current TOML");
    expect(headings[1]).toHaveTextContent("Proposed TOML");
    const changedCells = rowFor(oldUrl).getAllByRole("cell");
    expect(changedCells[0]).toHaveTextContent(oldUrl);
    expect(changedCells[1]).toHaveTextContent(newUrl);
    const preferences = within(comparison()).getAllByText(currentLines[1]);
    expect(preferences).toHaveLength(2);
    expect(preferences[0].closest("tr")).toBe(preferences[1].closest("tr"));
    expect(
      within(preferences[0].closest("tr")!).getAllByText("2"),
    ).toHaveLength(2);
  });

  it("aligns unequal change blocks and preserves line numbers after deletions", async () => {
    renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    const deletedCells = rowFor(currentLines[6]).getAllByRole("cell");
    expect(deletedCells[0]).toHaveTextContent(currentLines[6]);
    expect(deletedCells[1]).toHaveTextContent(/^$/);
    const addedCells = rowFor("requires_openai_auth = false").getAllByRole(
      "cell",
    );
    expect(addedCells[0]).toHaveTextContent(/^$/);
    expect(addedCells[1]).toHaveTextContent("requires_openai_auth = false");
    const unchangedWire = within(comparison()).getAllByText(currentLines[7]);
    expect(unchangedWire).toHaveLength(2);
    const wireCells = within(unchangedWire[0].closest("tr")!).getAllByRole(
      "cell",
    );
    expect(within(wireCells[0]).getByText("8")).toBeVisible();
    expect(within(wireCells[1]).getByText("7")).toBeVisible();
  });

  it("switches to inline and keeps the chosen layout when changing targets", async () => {
    renderPanel();
    fireEvent.click(await screen.findByRole("button", { name: "Inline" }));
    expect(screen.getByRole("button", { name: "Inline" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    for (const name of ["Current line number", "Proposed line number"]) {
      expect(
        within(comparison()).getByRole("columnheader", { name }),
      ).toHaveAttribute("scope", "col");
    }
    expect(
      within(comparison()).getByRole("cell", { name: `Removed: ${oldUrl}` }),
    ).toBeVisible();
    expect(
      within(comparison()).getByRole("cell", { name: `Added: ${newUrl}` }),
    ).toBeVisible();
    expect(within(comparison()).getAllByText(currentLines[1])).toHaveLength(1);
    expect(comparison()).toHaveTextContent(oldUrl);
    expect(comparison()).toHaveTextContent(newUrl);
    fireEvent.click(
      screen.getByRole("button", { name: "Return to OpenAI sign-in" }),
    );
    expect(screen.getByRole("button", { name: "Inline" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(comparison()).toHaveTextContent('model_provider = "openai"');
    expect(comparison()).toHaveTextContent('forced_login_method = "chatgpt"');
    expect(comparison()).not.toHaveTextContent(newUrl);
    fireEvent.click(screen.getByRole("button", { name: "Side by side" }));
    const cells = rowFor('model_provider = "openai"').getAllByRole("cell");
    expect(cells[0]).toHaveTextContent(currentLines[0]);
    expect(cells[1]).toHaveTextContent('model_provider = "openai"');
    fireEvent.click(
      screen.getByRole("button", { name: "Connect through Copilot" }),
    );
    expect(
      screen.getByRole("button", { name: "Side by side" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(comparison()).toHaveTextContent(newUrl);
  });

  it("shows the complete current file even when no changes are needed", async () => {
    mocks.invoke.mockResolvedValue({
      ...preview,
      copilotConfig: currentConfig,
      copilotDiff: "",
      copilotLines: currentLines.map((_, index) => context(index + 1)),
    });
    renderPanel();
    expect(
      await screen.findByText("No changes needed in this file."),
    ).toBeVisible();
    for (const line of currentLines.filter(Boolean)) {
      expect(within(comparison()).getAllByText(line)).toHaveLength(2);
    }
    expect(screen.getByRole("button", { name: "Copy diff" })).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Copy proposed TOML" }),
    ).toBeEnabled();
  });

  it("shows additions against an empty current file without inventing left-side content", async () => {
    const text = 'model_provider = "cc-switch"';
    mocks.invoke.mockResolvedValue({
      ...preview,
      copilotConfig: text,
      copilotDiff: `--- a/config.toml\n+++ b/config.toml\n@@ -0,0 +1 @@\n+${text}\n\\ No newline at end of file\n`,
      copilotLines: [{ ...added(1, text), text }],
    });
    renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    const cells = rowFor(text).getAllByRole("cell");
    expect(cells[0]).toHaveTextContent(/^$/);
    expect(cells[1]).toHaveTextContent(text);
    expect(
      within(cells[1]).getByText(/No newline at end of file/),
    ).toBeVisible();
  });

  it("copies either proposal or its Git diff and refreshes using read-only commands", async () => {
    renderPanel();
    expect(await screen.findByText(preview.configPath)).toBeVisible();
    expect(
      screen.getByText(/Atlas reads your configuration and never writes it/),
    ).toBeVisible();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /^(Apply|Save|Repair)/ }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy proposed TOML" }));
    await waitFor(() =>
      expect(mocks.copy).toHaveBeenLastCalledWith(preview.copilotConfig),
    );
    fireEvent.click(screen.getByRole("button", { name: "Copy diff" }));
    await waitFor(() =>
      expect(mocks.copy).toHaveBeenLastCalledWith(preview.copilotDiff),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "Return to OpenAI sign-in" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Proposed TOML" }));
    expect(screen.getByLabelText("Proposed TOML")).toHaveTextContent(
      'model_provider = "openai"',
    );
    fireEvent.click(screen.getByRole("button", { name: "Copy proposed TOML" }));
    await waitFor(() =>
      expect(mocks.copy).toHaveBeenLastCalledWith(preview.openaiConfig),
    );
    fireEvent.click(screen.getByRole("button", { name: "Copy diff" }));
    await waitFor(() =>
      expect(mocks.copy).toHaveBeenLastCalledWith(preview.openaiDiff),
    );
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledTimes(2));
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
