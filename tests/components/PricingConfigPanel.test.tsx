import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PricingConfigPanel } from "@/components/usage/PricingConfigPanel";
import type { ModelPricing } from "@/types/usage";

const mocks = vi.hoisted(() => ({
  pricing: vi.fn(),
  remove: vi.fn(),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));
vi.mock("@/lib/query/usage", () => ({
  useModelPricing: mocks.pricing,
  useDeleteModelPricing: () => ({ mutate: mocks.remove, isPending: false }),
}));
vi.mock("@/components/usage/PricingEditModal", () => ({
  PricingEditModal: ({
    model,
    isNew,
    onClose,
  }: {
    model: ModelPricing;
    isNew: boolean;
    onClose: () => void;
  }) => (
    <div role="dialog" aria-label="Pricing editor">
      {isNew ? "New model" : model.modelId}
      <button onClick={onClose}>Close editor</button>
    </div>
  ),
}));

const model: ModelPricing = {
  modelId: "gpt-6-astra",
  displayName: "GPT-6 Astra",
  inputCostPerMillion: "1.25",
  outputCostPerMillion: "5",
  cacheReadCostPerMillion: "0.125",
  cacheCreationCostPerMillion: "0",
};

beforeEach(() => {
  mocks.pricing
    .mockReset()
    .mockReturnValue({ data: [model], isLoading: false });
  mocks.remove
    .mockReset()
    .mockImplementation((_id, options) => options.onSuccess());
});

describe("Manual pricing configuration", () => {
  it("keeps model prices without pricing defaults or sync controls", () => {
    render(<PricingConfigPanel />);
    expect(screen.getByText("gpt-6-astra")).toBeVisible();
    expect(screen.getByText("$1.25")).toBeVisible();
    expect(screen.getByText("$0.125")).toBeVisible();
    expect(screen.getAllByRole("table")).toHaveLength(1);
    expect(
      screen.queryByText(/models\.dev|pricingDefaults/),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "common.save" }),
    ).not.toBeInTheDocument();
  });

  it("still opens editors for existing and new model prices", () => {
    render(<PricingConfigPanel />);
    fireEvent.click(screen.getByTitle("common.edit"));
    expect(
      screen.getByRole("dialog", { name: "Pricing editor" }),
    ).toHaveTextContent(model.modelId);
    fireEvent.click(screen.getByRole("button", { name: "Close editor" }));
    fireEvent.click(screen.getByRole("button", { name: "common.add" }));
    expect(
      screen.getByRole("dialog", { name: "Pricing editor" }),
    ).toHaveTextContent("New model");
  });

  it("still deletes a model only after confirmation", () => {
    render(<PricingConfigPanel />);
    fireEvent.click(screen.getByTitle("common.delete"));
    expect(mocks.remove).not.toHaveBeenCalled();
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.delete",
      }),
    );
    expect(mocks.remove).toHaveBeenCalledWith(model.modelId, expect.anything());
  });
});
