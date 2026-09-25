import type { CodexCatalogModel } from "@/types";

export const mapCodexCatalogModelForForm = (item: any): CodexCatalogModel => {
  // 隐藏字段（原生 Responses profile 用）不在行 UI 暴露，但必须 load→save
  // 原样保留，否则编辑保存 MiMo/MiniMax 等会丢官方 base_instructions、
  // 并行工具、图像模态。DB SSOT 为 camelCase、live 反解兜底可能为 snake_case，
  // 双格式兼容（与 displayName/contextWindow 一致）。
  const supportsParallelToolCalls =
    typeof item?.supportsParallelToolCalls === "boolean"
      ? item.supportsParallelToolCalls
      : typeof item?.supports_parallel_tool_calls === "boolean"
        ? item.supports_parallel_tool_calls
        : undefined;
  const inputModalities = Array.isArray(item?.inputModalities)
    ? item.inputModalities
    : Array.isArray(item?.input_modalities)
      ? item.input_modalities
      : undefined;
  const baseInstructions =
    typeof item?.baseInstructions === "string"
      ? item.baseInstructions
      : typeof item?.base_instructions === "string"
        ? item.base_instructions
        : undefined;
  const reasoningLevels = Array.isArray(item?.reasoningLevels)
    ? item.reasoningLevels
    : Array.isArray(item?.reasoning_levels)
      ? item.reasoning_levels
      : undefined;
  const defaultReasoningLevel =
    typeof item?.defaultReasoningLevel === "string"
      ? item.defaultReasoningLevel
      : typeof item?.default_reasoning_level === "string"
        ? item.default_reasoning_level
        : undefined;
  return {
    model: typeof item?.model === "string" ? item.model : "",
    displayName:
      typeof item?.displayName === "string"
        ? item.displayName
        : typeof item?.display_name === "string"
          ? item.display_name
          : "",
    contextWindow:
      typeof item?.contextWindow === "string" ||
      typeof item?.contextWindow === "number"
        ? item.contextWindow
        : typeof item?.context_window === "string" ||
            typeof item?.context_window === "number"
          ? item.context_window
          : "",
    ...(supportsParallelToolCalls !== undefined
      ? { supportsParallelToolCalls }
      : {}),
    ...(inputModalities ? { inputModalities } : {}),
    ...(baseInstructions ? { baseInstructions } : {}),
    ...(reasoningLevels && reasoningLevels.length > 0
      ? { reasoningLevels }
      : {}),
    ...(defaultReasoningLevel ? { defaultReasoningLevel } : {}),
  };
};
