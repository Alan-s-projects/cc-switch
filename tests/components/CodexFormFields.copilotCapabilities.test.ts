import { describe, expect, it } from "vitest";
import {
  isCopilotModelSupportedByCodex,
  resolveCopilotCatalogContextWindow,
  mergeCopilotModelCapabilities,
} from "@/components/providers/forms/CodexFormFields";
import type { CopilotModel } from "@/lib/api/copilot";
import type { CodexCopilotApiFormat } from "@/types";
import endpointCases from "../fixtures/copilot-endpoint-cases.json";

function model(supportedEndpoints?: string[]): CopilotModel {
  return {
    id: "model",
    name: "Model",
    vendor: "vendor",
    model_picker_enabled: true,
    supported_endpoints: supportedEndpoints,
  };
}

describe("Codex Copilot capabilities", () => {
  it.each(endpointCases)(
    "matches the backend endpoint contract: $name",
    ({ endpoints, formats }) => {
      const selections: CodexCopilotApiFormat[] = [
        "auto",
        "openai_responses",
        "openai_chat",
      ];
      for (const format of selections) {
        expect(isCopilotModelSupportedByCodex(model(endpoints), format)).toBe(
          formats.includes(format),
        );
      }
    },
  );

  it("fills an empty catalog context window without overwriting an explicit value", () => {
    expect(resolveCopilotCatalogContextWindow("", 400_000)).toBe(400_000);
    expect(resolveCopilotCatalogContextWindow(undefined, 1_000_000)).toBe(
      1_000_000,
    );
    expect(resolveCopilotCatalogContextWindow(200_000, 400_000)).toBe(200_000);
    expect(resolveCopilotCatalogContextWindow(1_000_000, 872_000)).toBe(
      872_000,
    );
  });

  it("replaces stale capability guesses with live declarations, including explicit false", () => {
    const saved = {
      model: "gpt-6-luna",
      supportsParallelToolCalls: false,
      inputModalities: ["text"],
      reasoningLevels: ["ultra"],
    };
    const live: CopilotModel = {
      ...model(["/responses"]),
      id: "gpt-6-luna",
      supports_parallel_tool_calls: true,
      supports_vision: true,
      reasoning_efforts: ["low", "medium", "high", "xhigh", "max"],
    };
    expect(mergeCopilotModelCapabilities(live, saved)).toMatchObject({
      supportsParallelToolCalls: true,
      inputModalities: ["text", "image"],
      reasoningLevels: live.reasoning_efforts,
    });
    expect(
      mergeCopilotModelCapabilities(
        {
          ...live,
          supports_parallel_tool_calls: false,
          supports_vision: false,
        },
        {
          ...saved,
          supportsParallelToolCalls: true,
          inputModalities: ["text", "image"],
        },
      ),
    ).toMatchObject({
      supportsParallelToolCalls: false,
      inputModalities: ["text"],
    });
    expect(mergeCopilotModelCapabilities(model(), saved)).toMatchObject({
      supportsParallelToolCalls: false,
      inputModalities: ["text"],
    });
  });

  it("rejects an absent capabilities field", () => {
    expect(isCopilotModelSupportedByCodex(model())).toBe(false);
  });
});
