import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import type { CodexCopilotApiFormat, ProviderMeta } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => null,
}));
vi.mock("@/components/providers/forms/hooks/useCopilotAuth", () => ({
  useCopilotAuth: () => ({ hasAnyAccount: true }),
}));
vi.mock("@/components/providers/forms/ProviderAdvancedConfig", () => ({
  ProviderAdvancedConfig: () => null,
}));
const config =
  'model_provider = "copilot"\n[model_providers.copilot]\nbase_url = "https://api.githubcopilot.com"\nwire_api = "responses"\n';
const models = [
  {
    model: "gpt-6-astra",
    contextWindow: 1048576,
    supportsParallelToolCalls: true,
    inputModalities: ["text", "image"],
  },
  {
    model: "gpt-6-luna",
    contextWindow: 872000,
    supportsParallelToolCalls: true,
    inputModalities: ["text", "image"],
  },
];

function renderForm(meta?: ProviderMeta) {
  const onSubmit = vi.fn<(values: ProviderFormValues) => void>();
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId="codex"
        submitLabel="save"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={{
          name: "GitHub Copilot",
          category: "third_party",
          settingsConfig: { auth: {}, config, modelCatalog: { models } },
          meta: meta ?? { providerType: "github_copilot" },
        }}
      />
    </QueryClientProvider>,
  );
  return onSubmit;
}

function formatControl() {
  return screen.getByRole("combobox", { name: "Upstream format" });
}

const formatLabels = {
  auto: "Automatic (recommended)",
  openai_chat: "Chat Completions",
  openai_responses: "Responses",
};

async function selectFormat(format: CodexCopilotApiFormat) {
  fireEvent.keyDown(formatControl(), { key: "ArrowDown" });
  fireEvent.click(
    await screen.findByRole("option", { name: formatLabels[format] }),
  );
}

describe("Codex Copilot provider form", () => {
  const scrollIntoViewDescriptor = Object.getOwnPropertyDescriptor(
    HTMLElement.prototype,
    "scrollIntoView",
  );
  beforeAll(() => {
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
  });
  afterAll(() => {
    if (scrollIntoViewDescriptor) {
      Object.defineProperty(
        HTMLElement.prototype,
        "scrollIntoView",
        scrollIntoViewDescriptor,
      );
    } else {
      Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
    }
  });

  it("shows the existing model catalog without a TOML or default-model editor", () => {
    renderForm();
    expect(screen.getAllByDisplayValue("gpt-6-astra")).toHaveLength(1);
    expect(
      screen.queryByLabelText("codexConfig.defaultModelLabel"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("codexConfig.writeCommonConfig"),
    ).not.toBeInTheDocument();
    expect(screen.getByDisplayValue("gpt-6-luna")).toBeVisible();
    expect(screen.getByDisplayValue("1048576")).toBeVisible();
    for (const label of ["Provider name", "Notes", "Website", "Icon"]) {
      expect(screen.queryByLabelText(label)).not.toBeInTheDocument();
    }
    expect(formatControl()).toHaveTextContent(formatLabels.auto);
    expect(screen.getByText("Model catalog")).toBeVisible();
  });

  it.each<CodexCopilotApiFormat>(["auto", "openai_chat", "openai_responses"])(
    "persists %s without changing the Codex client wire protocol",
    async (format) => {
      const onSubmit = renderForm();
      await selectFormat(format);
      expect(screen.getByText("Model catalog")).toBeVisible();
      fireEvent.click(screen.getByRole("button", { name: "save" }));
      await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
      const saved = onSubmit.mock.calls[0][0];
      expect(saved.meta?.codexCopilotApiFormat).toBe(
        format === "auto" ? undefined : format,
      );
      expect(saved.meta?.apiFormat).toBe(
        format === "auto" ? "openai_chat" : format,
      );
      expect(JSON.parse(saved.settingsConfig).config).toContain(
        'wire_api = "responses"',
      );
    },
  );

  it("keeps legacy cards automatic and shows mapping even with an empty catalog", () => {
    renderForm({ providerType: "github_copilot", apiFormat: "openai_chat" });
    expect(formatControl()).toHaveTextContent(formatLabels.auto);
    expect(screen.getByText("Model catalog")).toBeVisible();
  });

  it("does not offer preset switching while editing an existing Copilot card", () => {
    renderForm({ providerType: "github_copilot", apiFormat: "openai_chat" });
    expect(
      screen.queryByRole("button", { name: /DeepSeek/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /providerPreset.custom/ }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("providerPreset.label")).not.toBeInTheDocument();
  });

  it("loads a saved explicit protocol and can return it to automatic", async () => {
    const onSubmit = renderForm({
      providerType: "github_copilot",
      apiFormat: "openai_responses",
      codexCopilotApiFormat: "openai_responses",
    });
    expect(formatControl()).toHaveTextContent(formatLabels.openai_responses);
    await selectFormat("auto");
    fireEvent.click(screen.getByRole("button", { name: "save" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(
      onSubmit.mock.calls[0][0].meta?.codexCopilotApiFormat,
    ).toBeUndefined();
    expect(onSubmit.mock.calls[0][0].meta?.apiFormat).toBe("openai_chat");
  });

  it("offers only Copilot transports and no other provider presets", async () => {
    renderForm();
    fireEvent.keyDown(formatControl(), { key: "ArrowDown" });
    expect(await screen.findAllByRole("option")).toHaveLength(3);
    expect(
      screen.queryByRole("option", { name: /Anthropic Messages/ }),
    ).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("option", { name: formatLabels.openai_chat }),
    );
    expect(
      screen.queryByRole("button", { name: /DeepSeek/ }),
    ).not.toBeInTheDocument();
    expect(formatControl()).toHaveTextContent(formatLabels.openai_chat);
    expect(screen.getByText("Model catalog")).toBeVisible();
  });
});
