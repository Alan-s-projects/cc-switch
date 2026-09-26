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
    id: "gpt-test",
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

  it("refreshes live capabilities while preserving saved reasoning choices", () => {
    const saved = {
      model: "gpt-6-luna",
      contextWindow: 1_000_000,
      supportsParallelToolCalls: false,
      inputModalities: ["text"],
      reasoningLevels: ["low", "high", "ultra"],
      defaultReasoningLevel: "ultra",
    };
    const live: CopilotModel = {
      ...model(["/responses"]),
      id: "gpt-6-luna",
      context_window: 872_000,
      supports_parallel_tool_calls: true,
      supports_vision: true,
      reasoning_efforts: ["low", "medium", "high", "xhigh", "max"],
    };
    const refreshed = mergeCopilotModelCapabilities(live, saved);
    expect(refreshed).toMatchObject({
      contextWindow: 872_000,
      supportsParallelToolCalls: true,
      inputModalities: ["text", "image"],
      reasoningLevels: saved.reasoningLevels,
      defaultReasoningLevel: "ultra",
    });
    expect(mergeCopilotModelCapabilities(live, refreshed)).toEqual(refreshed);
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
      reasoningLevels: saved.reasoningLevels,
      defaultReasoningLevel: "ultra",
    });
  });

  it.each([{ reasoningLevels: undefined }, { reasoningLevels: [] }])(
    "initializes an unset reasoning list from Copilot without forcing a default",
    ({ reasoningLevels }) => {
      const live = {
        ...model(["/responses"]),
        reasoning_efforts: ["low", "medium", "high"],
      };
      const refreshed = mergeCopilotModelCapabilities(live, {
        model: live.id,
        reasoningLevels,
      });
      expect(refreshed.reasoningLevels).toEqual(live.reasoning_efforts);
      expect(refreshed.defaultReasoningLevel).toBeUndefined();
      expect(mergeCopilotModelCapabilities(live).reasoningLevels).toEqual(
        live.reasoning_efforts,
      );
    },
  );

  it("defaults a new model without advertised reasoning efforts to all standard levels and Auto", () => {
    const refreshed = mergeCopilotModelCapabilities(model(["/responses"]));
    expect(refreshed.reasoningLevels).toEqual([
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
      "ultra",
    ]);
    expect(refreshed.defaultReasoningLevel).toBeUndefined();
  });

  it("keeps an automatic default when the saved levels differ from Copilot", () => {
    const refreshed = mergeCopilotModelCapabilities(
      { ...model(["/responses"]), reasoning_efforts: ["low", "medium"] },
      { model: "gpt-test", reasoningLevels: ["high", "ultra"] },
    );
    expect(refreshed.reasoningLevels).toEqual(["high", "ultra"]);
    expect(refreshed.defaultReasoningLevel).toBeUndefined();
  });

  it("rejects an absent capabilities field", () => {
    expect(isCopilotModelSupportedByCodex(model())).toBe(false);
  });

  it("accepts GPT IDs without case sensitivity and rejects other models", () => {
    const compatible = model(["/responses"]);
    expect(
      isCopilotModelSupportedByCodex({ ...compatible, id: " GPT-6-ASTRA " }),
    ).toBe(true);
    expect(
      isCopilotModelSupportedByCodex({ ...compatible, id: "other-model" }),
    ).toBe(false);
    expect(isCopilotModelSupportedByCodex({ ...compatible, id: "gpt-" })).toBe(
      false,
    );
  });
});
