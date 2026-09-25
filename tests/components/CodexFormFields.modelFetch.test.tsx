import { useEffect, useState, type ComponentProps } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { FormProvider, useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import {
  copilotGetModels,
  copilotGetModelsForAccount,
  type CopilotModel,
} from "@/lib/api/copilot";
import { showFetchModelsError } from "@/lib/api/model-fetch";
import type { CodexCatalogModel } from "@/types";

vi.mock("@/lib/api/copilot", () => ({
  copilotGetModels: vi.fn(),
  copilotGetModelsForAccount: vi.fn(),
}));
vi.mock("@/lib/api/model-fetch", () => ({ showFetchModelsError: vi.fn() }));
vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() },
}));
vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => null,
}));

type Props = ComponentProps<typeof CodexFormFields>;
function props(overrides: Partial<Props> = {}): Props {
  return {
    isCopilotPreset: true,
    isCopilotAuthenticated: true,
    selectedGitHubAccountId: "account-a",
    codexApiKey: "",
    onApiKeyChange: vi.fn(),
    category: "third_party",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    shouldShowSpeedTest: false,
    codexBaseUrl: "https://api.githubcopilot.com",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    apiFormat: "openai_chat",
    onApiFormatChange: vi.fn(),
    copilotApiFormat: "auto",
    anthropicAuthField: "ANTHROPIC_AUTH_TOKEN",
    onAnthropicAuthFieldChange: vi.fn(),
    impersonateClaudeCode: false,
    onImpersonateClaudeCodeChange: vi.fn(),
    maxOutputTokens: "",
    onMaxOutputTokensChange: vi.fn(),
    promptCacheRouting: "auto",
    onPromptCacheRoutingChange: vi.fn(),
    catalogModels: [],
    onCatalogModelsChange: vi.fn(),
    speedTestEndpoints: [],
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
    ...overrides,
  };
}
function Harness(input: Props) {
  const form = useForm();
  const [models, setModels] = useState<CodexCatalogModel[]>(
    input.catalogModels ?? [],
  );
  useEffect(() => {
    setModels(input.catalogModels ?? []);
  }, [input.catalogModels]);
  return (
    <FormProvider {...form}>
      <CodexFormFields
        {...input}
        catalogModels={models}
        onCatalogModelsChange={(next) => {
          input.onCatalogModelsChange?.(next);
          setModels(next);
        }}
      />
    </FormProvider>
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
  screen.getAllByRole("button", { name: "providerForm.fetchModels" })[0];

describe("Copilot model catalog import", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("filters models by Copilot transport and imports them into the bridge catalog", async () => {
    vi.mocked(copilotGetModelsForAccount).mockResolvedValue([
      model("gpt-6-astra"),
      model("chat-only", "/chat/completions"),
    ]);
    const input = props({ copilotApiFormat: "openai_responses" });
    render(<Harness {...input} />);
    fireEvent.click(fetchButton());
    await waitFor(() =>
      expect(screen.getAllByDisplayValue("gpt-6-astra")[0]).toBeVisible(),
    );
    expect(screen.queryByDisplayValue("chat-only")).not.toBeInTheDocument();
    expect(copilotGetModelsForAccount).toHaveBeenCalledWith("account-a");
  });

  it("keeps explicit model declarations and defaults new GPT imports to image input", async () => {
    vi.mocked(copilotGetModelsForAccount).mockResolvedValue([
      model("gpt-existing"),
      model("gpt-new"),
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
        }),
      ]),
    );
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

  it("reports failures and releases the loading state", async () => {
    const failure = new Error("offline");
    vi.mocked(copilotGetModelsForAccount).mockRejectedValue(failure);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    render(<Harness {...props()} />);
    fireEvent.click(fetchButton());
    await waitFor(() =>
      expect(showFetchModelsError).toHaveBeenCalledWith(
        failure,
        expect.any(Function),
      ),
    );
    await waitFor(() => expect(fetchButton()).not.toBeDisabled());
    warn.mockRestore();
  });

  it("reports an empty model list", async () => {
    vi.mocked(copilotGetModelsForAccount).mockResolvedValue([]);
    render(<Harness {...props()} />);
    fireEvent.click(fetchButton());
    await waitFor(() =>
      expect(toast.info).toHaveBeenCalledWith("providerForm.fetchModelsEmpty"),
    );
  });

  it("requires Copilot sign-in before fetching models", () => {
    render(<Harness {...props({ isCopilotAuthenticated: false })} />);
    fireEvent.click(fetchButton());
    expect(toast.error).toHaveBeenCalled();
    expect(copilotGetModelsForAccount).not.toHaveBeenCalled();
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
