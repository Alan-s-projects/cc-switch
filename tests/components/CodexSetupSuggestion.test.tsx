import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CodexSetupSuggestion } from "@/components/providers/CodexSetupSuggestion";
import type { ConfigDiffLine } from "@/components/providers/ConfigDiff";
import { createTestQueryClient } from "../utils/testQueryClient";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  copy: vi.fn(),
  open: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open }));
vi.mock("@/lib/clipboard", () => ({ copyText: mocks.copy }));

const previewPathKey = "cc-switch-codex-preview-path";
const profilePath = "D:/Codex profiles/work config.toml";
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
  configExists: true,
  endpoint: "http://127.0.0.1:15721/v1",
  currentProvider: "Copilot Bridge",
  copilotConfig: proposedText(copilotLines),
  copilotDiff: unifiedDiff(copilotLines, 10),
  copilotLines,
  openaiConfig: proposedText(openaiLines),
  openaiDiff: unifiedDiff(openaiLines, 9),
  openaiLines,
};
const recommendationControls = [
  ["modelContext", "Use model context defaults"],
  ["autoCompaction", "Use automatic compaction defaults"],
  ["reasoning", "Use Codex default reasoning"],
] as const;
const automaticCompaction = {
  modelContext: false,
  autoCompaction: true,
  reasoning: false,
};
const withoutFixedCompaction = (lines: ConfigDiffLine[]): ConfigDiffLine[] =>
  lines.map((line) =>
    line.oldLineNumber === 2
      ? removed(2)
      : {
          ...line,
          newLineNumber:
            line.newLineNumber === null
              ? null
              : line.newLineNumber - (line.newLineNumber > 2 ? 1 : 0),
        },
  );
const recommendedCopilotLines = withoutFixedCompaction(copilotLines);
const recommendedOpenaiLines = withoutFixedCompaction(openaiLines);
const recommendedPreview = {
  ...preview,
  copilotConfig: proposedText(recommendedCopilotLines),
  copilotDiff: unifiedDiff(recommendedCopilotLines, 9),
  copilotLines: recommendedCopilotLines,
  openaiConfig: proposedText(recommendedOpenaiLines),
  openaiDiff: unifiedDiff(recommendedOpenaiLines, 8),
  openaiLines: recommendedOpenaiLines,
};
const profilePreview = {
  ...preview,
  configPath: profilePath,
  copilotConfig: preview.copilotConfig.replace("900000", "150000"),
  copilotDiff: preview.copilotDiff.replace("900000", "150000"),
  copilotLines: copilotLines.map((line) => ({
    ...line,
    text: line.text.replace("900000", "150000"),
  })),
  openaiConfig: preview.openaiConfig.replace("900000", "150000"),
  openaiDiff: preview.openaiDiff.replace("900000", "150000"),
  openaiLines: openaiLines.map((line) => ({
    ...line,
    text: line.text.replace("900000", "150000"),
  })),
};
const renderPanel = () =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <CodexSetupSuggestion />
    </QueryClientProvider>,
  );
const comparison = () =>
  screen.getByRole("region", { name: "Configuration diff" });
const pathInput = () =>
  screen.getByRole("textbox", { name: "Codex TOML file" });
const refresh = () => screen.getByRole("button", { name: "Refresh" });
const copyProposal = () =>
  screen.getByRole("button", { name: "Copy proposed TOML" });
const expectNoPreview = () => {
  expect(
    screen.queryByRole("region", { name: "Configuration diff" }),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Copy proposed TOML" }),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Copy diff" }),
  ).not.toBeInTheDocument();
};
const rowFor = (text: string) => {
  const row = within(comparison()).getByText(text).closest("tr");
  expect(row).not.toBeNull();
  return within(row!);
};

describe("read-only Codex connection suggestions", () => {
  beforeEach(() => {
    localStorage.removeItem(previewPathKey);
    mocks.invoke.mockReset().mockResolvedValue(preview);
    mocks.copy.mockReset().mockResolvedValue(undefined);
    mocks.open.mockReset().mockResolvedValue(null);
  });

  afterEach(() => {
    localStorage.removeItem(previewPathKey);
  });

  it("defaults to current TOML on the left and proposed TOML on the right", async () => {
    renderPanel();
    expect(
      await screen.findByRole("button", { name: "Side by side" }),
    ).toHaveAttribute("aria-pressed", "true");
    const recommendations = screen.getByRole("group", {
      name: "Recommended settings",
    });
    for (const [, name] of recommendationControls) {
      expect(
        within(recommendations).getByRole("checkbox", { name }),
      ).not.toBeChecked();
    }
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

  it("explains a missing auto-detected file and previews additions without creating it", async () => {
    const text = 'model_provider = "cc-switch"';
    mocks.invoke.mockResolvedValue({
      ...preview,
      configExists: false,
      copilotConfig: text,
      copilotDiff: `--- a/config.toml\n+++ b/config.toml\n@@ -0,0 +1 @@\n+${text}\n\\ No newline at end of file\n`,
      copilotLines: [{ ...added(1, text), text }],
    });
    renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    expect(
      screen.getByText(/No config.toml was found at this location/),
    ).toBeVisible();
    expect(pathInput()).toHaveValue(preview.configPath);
    const cells = rowFor(text).getAllByRole("cell");
    expect(cells[0]).toHaveTextContent(/^$/);
    expect(cells[1]).toHaveTextContent(text);
    expect(
      within(cells[1]).getByText(/No newline at end of file/),
    ).toBeVisible();
    expect(mocks.invoke.mock.calls).toEqual([
      ["get_codex_setup_suggestion", { configPath: null }],
    ]);
    expect(localStorage.getItem(previewPathKey)).toBeNull();
  });

  it("copies either proposal or its Git diff and refreshes using read-only commands", async () => {
    renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    expect(pathInput()).toHaveValue(preview.configPath);
    expect(
      screen.getByText(/Atlas reads your configuration and never writes it/),
    ).toBeVisible();
    expect(screen.getAllByRole("textbox")).toEqual([pathInput()]);
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
    expect(mocks.invoke).toHaveBeenCalledWith("get_codex_setup_suggestion", {
      configPath: null,
    });
  });

  it("reports invalid configuration without offering a write or repair action", async () => {
    mocks.invoke.mockRejectedValue(new Error("Codex config.toml is invalid"));
    renderPanel();
    expect(await screen.findByRole("alert")).toHaveTextContent("invalid");
    expect(
      screen.queryByRole("button", { name: "Copy proposed TOML" }),
    ).not.toBeInTheDocument();
    expect(mocks.copy).not.toHaveBeenCalled();
    expect(mocks.invoke).toHaveBeenCalledWith("get_codex_setup_suggestion", {
      configPath: null,
    });
    expect(pathInput()).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Browse for TOML file" }),
    ).toBeEnabled();
  });

  it.each(["Refresh", "Enter"] as const)(
    "loads a typed path with %s without copying the previous file or changing the comparison target",
    async (action) => {
      let resolve!: (value: typeof profilePreview) => void;
      mocks.invoke.mockResolvedValueOnce(preview).mockImplementationOnce(
        () =>
          new Promise<typeof profilePreview>((done) => {
            resolve = done;
          }),
      );
      renderPanel();
      await screen.findByRole("region", { name: "Configuration diff" });
      fireEvent.click(
        screen.getByRole("button", { name: "Return to OpenAI sign-in" }),
      );
      fireEvent.click(screen.getByRole("button", { name: "Inline" }));
      fireEvent.change(pathInput(), {
        target: { value: ` "${profilePath}" ` },
      });
      expectNoPreview();
      expect(mocks.invoke).toHaveBeenCalledTimes(1);
      expect(localStorage.getItem(previewPathKey)).toBeNull();

      if (action === "Enter") {
        await userEvent.type(pathInput(), "{enter}");
      } else {
        fireEvent.click(refresh());
      }
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenLastCalledWith(
          "get_codex_setup_suggestion",
          { configPath: profilePath },
        ),
      );
      expectNoPreview();
      expect(localStorage.getItem(previewPathKey)).toBeNull();
      await act(async () => resolve(profilePreview));

      await screen.findByRole("region", { name: "Configuration diff" });
      expect(pathInput()).toHaveValue(profilePath);
      expect(comparison()).toHaveTextContent(
        "model_auto_compact_token_limit = 150000",
      );
      expect(comparison()).not.toHaveTextContent("900000");
      expect(screen.getByRole("button", { name: "Inline" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
      expect(
        screen.getByRole("button", { name: "Return to OpenAI sign-in" }),
      ).toHaveAttribute("aria-pressed", "true");
      fireEvent.click(copyProposal());
      await waitFor(() =>
        expect(mocks.copy).toHaveBeenLastCalledWith(
          profilePreview.openaiConfig,
        ),
      );
      fireEvent.click(screen.getByRole("button", { name: "Copy diff" }));
      await waitFor(() =>
        expect(mocks.copy).toHaveBeenLastCalledWith(profilePreview.openaiDiff),
      );
      expect(mocks.invoke.mock.calls).toEqual([
        ["get_codex_setup_suggestion", { configPath: null }],
        ["get_codex_setup_suggestion", { configPath: profilePath }],
      ]);
      await waitFor(() =>
        expect(localStorage.getItem(previewPathKey)).toBe(profilePath),
      );
    },
  );

  it("keeps the current file when browsing is cancelled and loads a chosen TOML immediately", async () => {
    mocks.open.mockResolvedValueOnce(null).mockResolvedValueOnce(profilePath);
    mocks.invoke
      .mockResolvedValueOnce(preview)
      .mockResolvedValueOnce(profilePreview);
    renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    const browse = screen.getByRole("button", {
      name: "Browse for TOML file",
    });
    await userEvent.click(browse);
    expect(mocks.open).toHaveBeenCalledWith({
      directory: false,
      multiple: false,
      filters: [{ name: "TOML", extensions: ["toml"] }],
      defaultPath: preview.configPath,
    });
    expect(mocks.invoke).toHaveBeenCalledTimes(1);
    expect(pathInput()).toHaveValue(preview.configPath);
    expect(copyProposal()).toBeEnabled();

    await userEvent.click(browse);
    await waitFor(() => expect(pathInput()).toHaveValue(profilePath));
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "get_codex_setup_suggestion",
      { configPath: profilePath },
    );
    await waitFor(() => expect(copyProposal()).toBeEnabled());
    fireEvent.click(copyProposal());
    await waitFor(() =>
      expect(mocks.copy).toHaveBeenCalledWith(profilePreview.copilotConfig),
    );
  });

  it("reopens the remembered file and clears that choice with Auto-detect", async () => {
    localStorage.setItem(previewPathKey, profilePath);
    mocks.invoke.mockImplementation((_command, { configPath }) =>
      Promise.resolve(configPath === profilePath ? profilePreview : preview),
    );
    const first = renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    expect(pathInput()).toHaveValue(profilePath);
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "get_codex_setup_suggestion",
      { configPath: profilePath },
    );
    first.unmount();

    const reopened = renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    expect(pathInput()).toHaveValue(profilePath);
    fireEvent.click(screen.getByRole("button", { name: "Auto-detect" }));
    await waitFor(() => expect(pathInput()).toHaveValue(preview.configPath));
    expect(localStorage.getItem(previewPathKey)).toBeNull();
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "get_codex_setup_suggestion",
      { configPath: null },
    );
    reopened.unmount();

    renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    expect(pathInput()).toHaveValue(preview.configPath);
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "get_codex_setup_suggestion",
      { configPath: null },
    );
  });

  it.each(["The selected file does not exist", "The selected TOML is invalid"])(
    "keeps the last successful path and allows recovery when %s",
    async (message) => {
      localStorage.setItem(previewPathKey, profilePath);
      mocks.invoke
        .mockResolvedValueOnce(profilePreview)
        .mockRejectedValueOnce(new Error(message))
        .mockResolvedValue(preview);
      renderPanel();
      await screen.findByRole("region", { name: "Configuration diff" });
      const badPath = "D:/Codex profiles/bad.toml";
      fireEvent.change(pathInput(), { target: { value: badPath } });
      fireEvent.click(refresh());
      expect(await screen.findByRole("alert")).toHaveTextContent(message);
      expect(pathInput()).toHaveValue(badPath);
      expectNoPreview();
      expect(localStorage.getItem(previewPathKey)).toBe(profilePath);
      expect(refresh()).toBeEnabled();
      expect(
        screen.getByRole("button", { name: "Browse for TOML file" }),
      ).toBeEnabled();
      fireEvent.click(screen.getByRole("button", { name: "Auto-detect" }));
      await screen.findByRole("region", { name: "Configuration diff" });
      expect(pathInput()).toHaveValue(preview.configPath);
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(localStorage.getItem(previewPathKey)).toBeNull();
      expect(mocks.copy).not.toHaveBeenCalled();
    },
  );

  it("disables copies while refreshing and hides cached content after a failed refresh", async () => {
    localStorage.setItem(previewPathKey, profilePath);
    let reject!: (error: Error) => void;
    mocks.invoke
      .mockResolvedValueOnce(profilePreview)
      .mockImplementationOnce(
        () =>
          new Promise((_resolve, fail) => {
            reject = fail;
          }),
      )
      .mockResolvedValue(profilePreview);
    renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    fireEvent.click(refresh());
    await waitFor(() => expect(copyProposal()).toBeDisabled());
    expect(screen.getByRole("button", { name: "Copy diff" })).toBeDisabled();
    await act(async () => reject(new Error("Cannot read this TOML file")));
    expect(await screen.findByRole("alert")).toHaveTextContent("Cannot read");
    expectNoPreview();
    expect(localStorage.getItem(previewPathKey)).toBe(profilePath);

    fireEvent.click(refresh());
    await screen.findByRole("region", { name: "Configuration diff" });
    expect(copyProposal()).toBeEnabled();
    expect(mocks.invoke.mock.calls).toEqual(
      Array.from({ length: 3 }, () => [
        "get_codex_setup_suggestion",
        { configPath: profilePath },
      ]),
    );
  });

  it("does not replace the auto-detected preview with a late result for another file", async () => {
    let resolve!: (value: typeof profilePreview) => void;
    mocks.invoke.mockImplementation((_command, { configPath }) =>
      configPath === profilePath
        ? new Promise<typeof profilePreview>((done) => {
            resolve = done;
          })
        : Promise.resolve(preview),
    );
    mocks.open.mockResolvedValue(profilePath);
    renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    await userEvent.click(
      screen.getByRole("button", { name: "Browse for TOML file" }),
    );
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenLastCalledWith(
        "get_codex_setup_suggestion",
        { configPath: profilePath },
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Auto-detect" }));
    await waitFor(() => expect(pathInput()).toHaveValue(preview.configPath));
    await act(async () => resolve(profilePreview));
    expect(pathInput()).toHaveValue(preview.configPath);
    expect(comparison()).toHaveTextContent(
      "model_auto_compact_token_limit = 900000",
    );
    expect(localStorage.getItem(previewPathKey)).toBeNull();
    fireEvent.click(copyProposal());
    await waitFor(() =>
      expect(mocks.copy).toHaveBeenCalledWith(preview.copilotConfig),
    );
  });

  it("uses selected recommendations for both targets and copies the matching proposal and diff", async () => {
    mocks.invoke.mockImplementation((_command, { recommendations }) =>
      Promise.resolve(
        recommendations?.autoCompaction ? recommendedPreview : preview,
      ),
    );
    renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    const options = {
      modelContext: false,
      autoCompaction: false,
      reasoning: false,
    };
    for (const [key, name] of recommendationControls) {
      fireEvent.click(screen.getByRole("checkbox", { name }));
      options[key] = true;
      await waitFor(() =>
        expect(mocks.invoke).toHaveBeenLastCalledWith(
          "get_codex_setup_suggestion",
          { configPath: null, recommendations: { ...options } },
        ),
      );
    }
    await waitFor(() => expect(copyProposal()).toBeEnabled());
    const compactCells = rowFor(currentLines[1]).getAllByRole("cell");
    expect(compactCells[0]).toHaveTextContent(currentLines[1]);
    expect(compactCells[1]).toHaveTextContent(/^$/);

    for (const [target, config, diff] of [
      [
        "Connect through Copilot",
        recommendedPreview.copilotConfig,
        recommendedPreview.copilotDiff,
      ],
      [
        "Return to OpenAI sign-in",
        recommendedPreview.openaiConfig,
        recommendedPreview.openaiDiff,
      ],
    ]) {
      fireEvent.click(screen.getByRole("button", { name: target }));
      fireEvent.click(screen.getByRole("button", { name: "Inline" }));
      expect(comparison()).toHaveTextContent(currentLines[1]);
      for (const [, name] of recommendationControls) {
        expect(screen.getByRole("checkbox", { name })).toBeChecked();
      }
      fireEvent.click(screen.getByRole("button", { name: "Proposed TOML" }));
      expect(screen.getByLabelText("Proposed TOML")).not.toHaveTextContent(
        currentLines[1],
      );
      fireEvent.click(copyProposal());
      await waitFor(() => expect(mocks.copy).toHaveBeenLastCalledWith(config));
      fireEvent.click(screen.getByRole("button", { name: "Copy diff" }));
      await waitFor(() => expect(mocks.copy).toHaveBeenLastCalledWith(diff));
    }

    for (const [, name] of recommendationControls) {
      fireEvent.click(screen.getByRole("checkbox", { name }));
    }
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenLastCalledWith(
        "get_codex_setup_suggestion",
        { configPath: null },
      ),
    );
    await waitFor(() => expect(copyProposal()).toBeEnabled());
    expect(screen.getByLabelText("Proposed TOML")).toHaveTextContent(
      currentLines[1],
    );
    fireEvent.click(copyProposal());
    await waitFor(() =>
      expect(mocks.copy).toHaveBeenLastCalledWith(preview.openaiConfig),
    );
  });

  it("cannot copy a stale recommendation while pending or after a late response", async () => {
    let resolve!: (value: typeof recommendedPreview) => void;
    mocks.invoke.mockImplementation((_command, { recommendations }) =>
      recommendations?.autoCompaction
        ? new Promise<typeof recommendedPreview>((done) => {
            resolve = done;
          })
        : Promise.resolve(preview),
    );
    renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    const checkbox = screen.getByRole("checkbox", {
      name: "Use automatic compaction defaults",
    });
    fireEvent.click(checkbox);
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenLastCalledWith(
        "get_codex_setup_suggestion",
        { configPath: null, recommendations: automaticCompaction },
      ),
    );
    expectNoPreview();
    expect(mocks.copy).not.toHaveBeenCalled();

    fireEvent.click(checkbox);
    await waitFor(() => expect(copyProposal()).toBeEnabled());
    await act(async () => resolve(recommendedPreview));
    expect(checkbox).not.toBeChecked();
    const compactLine = within(comparison()).getAllByText(currentLines[1])[0];
    const cells = within(compactLine.closest("tr")!).getAllByRole("cell");
    expect(cells[0]).toHaveTextContent(currentLines[1]);
    expect(cells[1]).toHaveTextContent(currentLines[1]);
    fireEvent.click(copyProposal());
    await waitFor(() =>
      expect(mocks.copy).toHaveBeenLastCalledWith(preview.copilotConfig),
    );
    fireEvent.click(screen.getByRole("button", { name: "Copy diff" }));
    await waitFor(() =>
      expect(mocks.copy).toHaveBeenLastCalledWith(preview.copilotDiff),
    );
  });

  it("keeps recommendation choices local to the open preview", async () => {
    const first = renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    for (const [, name] of recommendationControls) {
      fireEvent.click(screen.getByRole("checkbox", { name }));
    }
    await waitFor(() => expect(copyProposal()).toBeEnabled());
    first.unmount();

    renderPanel();
    await screen.findByRole("region", { name: "Configuration diff" });
    for (const [, name] of recommendationControls) {
      expect(screen.getByRole("checkbox", { name })).not.toBeChecked();
    }
    expect(mocks.invoke).toHaveBeenLastCalledWith(
      "get_codex_setup_suggestion",
      { configPath: null },
    );
  });
});
