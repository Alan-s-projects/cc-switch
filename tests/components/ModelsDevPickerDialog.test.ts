import { describe, expect, it } from "vitest";
import {
  flattenModels,
  formatPrice,
  normalizeModelIdForPricing,
} from "@/components/usage/ModelsDevPickerDialog";
import {
  getCommonModelKeys,
  resolveModelsDevSelection,
  toModelPricing,
} from "@/lib/modelsDevPricing";

describe("GPT pricing IDs", () => {
  it("normalizes provider prefixes and transport/context suffixes", () => {
    expect(normalizeModelIdForPricing("gpt-6-astra")).toBe("gpt-6-astra");
    expect(normalizeModelIdForPricing("openai/GPT-6-ASTRA")).toBe(
      "gpt-6-astra",
    );
    expect(normalizeModelIdForPricing("gpt-6-astra:priority")).toBe(
      "gpt-6-astra",
    );
    expect(normalizeModelIdForPricing("gpt-6-astra@2026")).toBe(
      "gpt-6-astra-2026",
    );
    expect(normalizeModelIdForPricing("gpt-6-astra[1m]")).toBe("gpt-6-astra");
  });
});

describe("formatPrice", () => {
  it("formats finite positive prices without exponent notation", () => {
    expect(formatPrice(5)).toBe("5");
    expect(formatPrice(0.5)).toBe("0.5");
    expect(formatPrice(1.0395)).toBe("1.0395");
    expect(formatPrice(0.000001)).toBe("0.000001");
    for (const price of [0, -1, NaN, Infinity, 1e21, 1e-8])
      expect(formatPrice(price)).toBe("0");
  });
});

describe("GPT pricing selection", () => {
  it("imports only canonical OpenAI GPT prices, preserving cache prices and date order", () => {
    const entries = flattenModels({
      openai: {
        name: "OpenAI",
        models: {
          "gpt-old": { release_date: "2025-01-01", cost: { input: 1 } },
          "gpt-new": {
            name: "GPT New",
            release_date: "2026-01-01",
            cost: { input: 3, output: 6, cache_read: 0.3 },
          },
          "gpt-unpriced": { name: "No price" },
          "other-model": { cost: { input: 1, output: 2 } },
        },
      },
      relay: { models: { "gpt-new": { cost: { input: 90, output: 180 } } } },
    });
    expect(entries.map((entry) => entry.key)).toEqual([
      "openai/gpt-new",
      "openai/gpt-old",
    ]);
    expect(entries[0]).toMatchObject({
      providerName: "OpenAI",
      cacheRead: 0.3,
      cacheWrite: 0,
    });
    expect(entries[1]).toMatchObject({ output: 0, cacheRead: 0 });
  });

  it("keeps image-capable GPT input models but excludes deprecated or non-text outputs", () => {
    const entries = flattenModels({
      openai: {
        models: {
          "GPT-6-ASTRA": {
            modalities: { input: ["text", "image"], output: ["text"] },
            cost: { input: 1, output: 2 },
          },
          "gpt-legacy": { status: "deprecated", cost: { input: 1, output: 2 } },
          "gpt-speech": {
            modalities: { output: ["audio"] },
            cost: { input: 1, output: 2 },
          },
          "gpt-mixed": {
            modalities: { output: ["text", "audio"] },
            cost: { input: 1, output: 2 },
          },
          "gpt-image-1": { cost: { input: 1, output: 2 } },
        },
      },
    });
    expect(entries.map((entry) => entry.normalizedId)).toEqual(["gpt-6-astra"]);
  });

  it("bounds common models while allowing an explicit older GPT and excluding removed providers", () => {
    const entries = flattenModels({
      openai: {
        models: Object.fromEntries(
          Array.from({ length: 7 }, (_, index) => [
            `gpt-${index + 1}`,
            {
              release_date: `2025-0${index + 1}-01`,
              cost: { input: index + 1, output: 2 },
            },
          ]),
        ),
      },
    });
    const common = getCommonModelKeys(entries);
    expect(common.size).toBe(6);
    expect(common.has("openai/gpt-7")).toBe(true);
    expect(common.has("openai/gpt-1")).toBe(false);
    const selected = resolveModelsDevSelection(entries, {
      autoSyncEnabled: true,
      includeCommonModels: true,
      selectedModelKeys: ["openai/gpt-1", "relay/other-model"],
      excludedCommonModelKeys: ["openai/gpt-7"],
      lastSyncAt: null,
      lastSyncError: null,
    });
    expect(selected.map((entry) => entry.modelId)).toEqual([
      "gpt-6",
      "gpt-5",
      "gpt-4",
      "gpt-3",
      "gpt-2",
      "gpt-1",
    ]);
    const canonical = entries[0];
    expect(toModelPricing([canonical, { ...canonical, input: 999 }])).toEqual([
      expect.objectContaining({ modelId: "gpt-7", inputCostPerMillion: "7" }),
    ]);
  });
});
