import { useEffect, useState, type ComponentProps } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import {
  copilotGetModels,
  copilotGetModelsForAccount,
  type CopilotModel,
} from "@/lib/api/copilot";
import type { CodexCatalogModel } from "@/types";

vi.mock("@/lib/api/copilot", () => ({
  copilotGetModels: vi.fn(),
  copilotGetModelsForAccount: vi.fn(),
}));
vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() },
}));
vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => null,
}));

type Props = ComponentProps<typeof CodexFormFields>;
function props(overrides: Partial<Props> = {}): Props {
  return {
    isCopilotAuthenticated: true,
    selectedGitHubAccountId: "account-a",
    copilotApiFormat: "auto",
    onCopilotApiFormatChange: vi.fn(),
    catalogModels: [],
    onCatalogModelsChange: vi.fn(),
    ...overrides,
  };
}
function Harness(input: Props) {
  const [models, setModels] = useState<CodexCatalogModel[]>(
    input.catalogModels ?? [],
  );
  useEffect(() => {
    setModels(input.catalogModels ?? []);
  }, [input.catalogModels]);
  return (
    <>
      <CodexFormFields
        {...input}
        catalogModels={models}
        onCatalogModelsChange={(next) => {
          input.onCatalogModelsChange?.(next);
          setModels(next);
        }}
      />
    </>
  );
}
function model(id: string, endpoint = "/responses"): CopilotModel {
  return {
    id,
    name: id,
    vendor: "OpenAI",
    model_picker_enabled: true,
    supported_endpoints: [endpoint],
    context_window: 1048576,
  };
}
const fetchButton = () =>
  screen.getAllByRole("button", { name: "Refresh models" })[0];

describe("Copilot model catalog import", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    HTMLElement.prototype.scrollIntoView = vi.fn();
  });

  it("filters models by Copilot transport and imports them into the bridge catalog", async () => {
    vi.mocked(copilotGetModelsForAccount).mockResolvedValue([
      model("gpt-6-astra"),
      model("gpt-chat-only", "/chat/completions"),
      model("other-model"),
    ]);
    const input = props({ copilotApiFormat: "openai_responses" });
    render(<Harness {...input} />);
    fireEvent.click(fetchButton());
    await waitFor(() =>
      expect(screen.getAllByDisplayValue("gpt-6-astra")[0]).toBeVisible(),
    );
    expect(screen.queryByDisplayValue("gpt-chat-only")).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("other-model")).not.toBeInTheDocument();
    expect(copilotGetModelsForAccount).toHaveBeenCalledWith("account-a");
  });

  it("preserves explicit limits and imports live image support", async () => {
    vi.mocked(copilotGetModelsForAccount).mockResolvedValue([
      model("gpt-existing"),
      { ...model("gpt-new"), supports_vision: true },
    ]);
    const input = props({
      catalogModels: [
        {
          model: "gpt-existing",
          contextWindow: 400000,
          inputModalities: ["text"],
        },
      ],
    });
    render(<Harness {...input} />);
    fireEvent.click(fetchButton());
    await waitFor(() =>
      expect(input.onCatalogModelsChange).toHaveBeenCalledWith([
        expect.objectContaining({
          model: "gpt-existing",
          contextWindow: 400000,
          inputModalities: ["text"],
        }),
        expect.objectContaining({
          model: "gpt-new",
          inputModalities: ["text", "image"],
          reasoningLevels: ["low", "medium", "high", "xhigh", "max", "ultra"],
          defaultReasoningLevel: undefined,
        }),
      ]),
    );
  });

  it("keeps saved reasoning choices on refresh, matching model IDs without case sensitivity", async () => {
    vi.mocked(copilotGetModelsForAccount).mockResolvedValue([
      {
        ...model("gpt-6-astra"),
        supports_parallel_tool_calls: true,
        supports_vision: true,
        reasoning_efforts: ["low", "medium", "high"],
      },
      { ...model("gpt-6-luna"), reasoning_efforts: ["low", "high"] },
    ]);
    const input = props({
      catalogModels: [
        {
          model: "GPT-6-ASTRA",
          supportsParallelToolCalls: false,
          inputModalities: ["text"],
          reasoningLevels: ["low", "high", "ultra"],
          defaultReasoningLevel: "ultra",
        },
      ],
    });
    render(<Harness {...input} />);
    fireEvent.click(fetchButton());
    await waitFor(() =>
      expect(input.onCatalogModelsChange).toHaveBeenCalledWith([
        expect.objectContaining({
          model: "gpt-6-astra",
          supportsParallelToolCalls: true,
          inputModalities: ["text", "image"],
          reasoningLevels: ["low", "high", "ultra"],
          defaultReasoningLevel: "ultra",
        }),
        expect.objectContaining({
          model: "gpt-6-luna",
          reasoningLevels: ["low", "high"],
          defaultReasoningLevel: undefined,
        }),
      ]),
    );
    expect(
      screen.getAllByRole("combobox", { name: "Reasoning levels" })[0],
    ).toHaveTextContent("low, high, ultra");
  });

  it("discards results from an account that is no longer selected", async () => {
    let resolve!: (value: CopilotModel[]) => void;
    vi.mocked(copilotGetModelsForAccount).mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const input = props();
    const { rerender } = render(<Harness {...input} />);
    fireEvent.click(fetchButton());
    rerender(<Harness {...input} selectedGitHubAccountId="account-b" />);
    await act(async () => {
      resolve([model("stale-model")]);
    });
    expect(screen.queryByDisplayValue("stale-model")).not.toBeInTheDocument();
    expect(input.onCatalogModelsChange).not.toHaveBeenCalled();
  });

  it("clears a removed default reasoning level without restoring the old selection", async () => {
    const input = props({
      catalogModels: [
        {
          model: "gpt-6-astra",
          reasoningLevels: ["low", "medium"],
          defaultReasoningLevel: "medium",
        },
      ],
    });
    render(<Harness {...input} />);
    const reasoning = screen.getByRole("combobox", {
      name: "Reasoning levels",
    });
    expect(reasoning).toHaveTextContent("low, medium");
    fireEvent.click(reasoning);
    fireEvent.click(await screen.findByRole("option", { name: "medium" }));
    await waitFor(() =>
      expect(input.onCatalogModelsChange).toHaveBeenLastCalledWith([
        expect.objectContaining({
          reasoningLevels: ["low"],
          defaultReasoningLevel: undefined,
        }),
      ]),
    );
  });

  it("reports failures and releases the loading state", async () => {
    const failure = new Error("offline");
    vi.mocked(copilotGetModelsForAccount).mockRejectedValue(failure);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    render(<Harness {...props()} />);
    fireEvent.click(fetchButton());
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("offline"));
    await waitFor(() => expect(fetchButton()).not.toBeDisabled());
    warn.mockRestore();
  });

  it("reports an empty model list", async () => {
    vi.mocked(copilotGetModelsForAccount).mockResolvedValue([]);
    render(<Harness {...props()} />);
    fireEvent.click(fetchButton());
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "No GPT models support the selected protocol. Try Automatic.",
      ),
    );
  });

  it("explains the empty catalog when signed out and requires sign-in to fetch", () => {
    render(<Harness {...props({ isCopilotAuthenticated: false })} />);
    expect(screen.getByRole("status")).toHaveTextContent(
      /GitHub Copilot is signed out/,
    );
    fireEvent.click(fetchButton());
    expect(toast.error).toHaveBeenCalled();
    expect(copilotGetModelsForAccount).not.toHaveBeenCalled();
  });

  it("explains how to load models for a signed-in account with an empty catalog", () => {
    render(<Harness {...props()} />);
    expect(screen.getByRole("status")).toHaveTextContent(
      /No models are in this catalog yet/,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      /available to your GitHub Copilot account/,
    );
  });

  it("uses the default Copilot account when no account is pinned", async () => {
    vi.mocked(copilotGetModels).mockResolvedValue([
      model("gpt-default-account"),
    ]);
    render(<Harness {...props({ selectedGitHubAccountId: null })} />);
    fireEvent.click(fetchButton());
    await waitFor(() =>
      expect(
        screen.getAllByDisplayValue("gpt-default-account")[0],
      ).toBeVisible(),
    );
    expect(copilotGetModels).toHaveBeenCalledTimes(1);
  });
});
