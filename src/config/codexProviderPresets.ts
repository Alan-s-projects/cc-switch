/**
 * Codex 预设供应商配置模板
 */
import { ProviderCategory } from "../types";
import type {
  CodexApiFormat,
  CodexCatalogModel,
  CodexChatReasoning,
  PromptCacheRoutingMode,
} from "../types";
import type { PresetTheme } from "./claudeProviderPresets";

export interface CodexProviderPreset {
  name: string;
  nameKey?: string; // i18n key for localized display name
  websiteUrl: string;
  // 第三方供应商可提供单独的获取 API Key 链接
  apiKeyUrl?: string;
  auth: Record<string, any>; // 将写入 ~/.codex/auth.json
  config: string; // 将写入 ~/.codex/config.toml（TOML 字符串）
  isOfficial?: boolean; // 标识是否为官方预设
  isPartner?: boolean; // 标识是否为商业合作伙伴
  primePartner?: boolean; // 置顶合作伙伴（顶级）：徽章显示为心形
  partnerPromotionKey?: string; // 合作伙伴促销信息的 i18n key
  category?: ProviderCategory; // 新增：分类
  isCustomTemplate?: boolean; // 标识是否为自定义模板
  // 新增：请求地址候选列表（用于地址管理/测速）
  endpointCandidates?: string[];
  // 新增：视觉主题配置
  theme?: PresetTheme;
  // 图标配置
  icon?: string; // 图标名称
  iconColor?: string; // 图标颜色
  // Codex API 格式
  apiFormat?: CodexApiFormat;
  // 仅用于区分托管认证来源；各 OAuth provider 的认证流程彼此独立。
  providerType?: "codex_oauth" | "xai_oauth" | "github_copilot";
  // OAuth 预设：隐藏 API Key 输入，保存前要求已登录托管账号
  requiresOAuth?: boolean;
  // Codex Chat 本地路由模式下的模型目录
  modelCatalog?: CodexCatalogModel[];
  // Codex Responses -> Chat Completions reasoning capability defaults
  codexChatReasoning?: CodexChatReasoning;
  // Session-based prompt-cache routing override for Chat Completions upstreams
  promptCacheRouting?: PromptCacheRoutingMode;
}

/**
 * 生成第三方供应商的 auth.json
 */
export function generateThirdPartyAuth(apiKey: string): Record<string, any> {
  return {
    OPENAI_API_KEY: apiKey || "",
  };
}

/**
 * 生成第三方供应商的 config.toml
 */
export function generateThirdPartyConfig(
  providerName: string,
  baseUrl: string,
  modelName = "gpt-5.6-sol",
  options?: {
    // 托管 OAuth 预设（requiresOAuth 卡）必须传 false：这类卡无静态 key，
    // requires_openai_auth = true 会被后端 keyless 安全闸拒绝切换
    // （provider.codex.config.official_auth_fallback）。
    requiresOpenAiAuth?: boolean;
  },
): string {
  const tomlString = (value: string) => JSON.stringify(value);
  const requiresOpenAiAuth = options?.requiresOpenAiAuth ?? true;

  return `model_provider = "custom"
model = ${tomlString(modelName)}
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = ${tomlString(providerName)}
base_url = ${tomlString(baseUrl)}
wire_api = "responses"
requires_openai_auth = ${requiresOpenAiAuth}`;
}

function modelCatalog(
  models: Array<
    | string
    | {
        model: string;
        displayName?: string;
        contextWindow?: number;
        // Native Responses (direct) overrides for the generated
        // model-catalogs.json. Omitted input modalities are inferred by the
        // backend: confirmed text-only models stay text-only; everything else
        // defaults to text+image.
        supportsParallelToolCalls?: boolean;
        inputModalities?: string[];
        // Vendor's OFFICIAL base_instructions; omit to inherit the neutral
        // template default. Required by Codex, so the backend always emits one.
        baseInstructions?: string;
        // Reasoning efforts the vendor's endpoint actually accepts (subset of
        // none/minimal/low/medium/high/xhigh/max/ultra). Omit to keep the
        // template's conservative none/high default. Pre-filled from official
        // vendor docs; users can still edit per provider in the form.
        reasoningLevels?: string[];
        defaultReasoningLevel?: string;
      }
  >,
): CodexCatalogModel[] {
  return models.map((entry) =>
    typeof entry === "string"
      ? { model: entry }
      : {
          model: entry.model,
          displayName: entry.displayName,
          contextWindow: entry.contextWindow,
          supportsParallelToolCalls: entry.supportsParallelToolCalls,
          inputModalities: entry.inputModalities,
          baseInstructions: entry.baseInstructions,
          reasoningLevels: entry.reasoningLevels,
          defaultReasoningLevel: entry.defaultReasoningLevel,
        },
  );
}

export const codexProviderPresets: CodexProviderPreset[] = [
  {
    name: "GitHub Copilot",
    websiteUrl: "https://github.com/features/copilot",
    auth: {},
    // Codex talks Responses to the local proxy. The proxy selects Copilot's
    // native Responses or Chat Completions transport from model capabilities.
    config: generateThirdPartyConfig(
      "github-copilot",
      "https://api.githubcopilot.com",
      "gpt-6-astra",
      { requiresOpenAiAuth: false },
    ),
    endpointCandidates: ["https://api.githubcopilot.com"],
    apiFormat: "openai_chat",
    providerType: "github_copilot",
    requiresOAuth: true,
    modelCatalog: modelCatalog(
      [
        { model: "gpt-6-astra", displayName: "GPT-6 Astra" },
        { model: "gpt-5.6-sol", displayName: "GPT-5.6 Sol" },
        { model: "gpt-5.6-terra", displayName: "GPT-5.6 Terra" },
        { model: "gpt-5.6-luna", displayName: "GPT-5.6 Luna" },
        { model: "gpt-5.5", displayName: "GPT-5.5" },
      ].map((model) => ({
        ...model,
        contextWindow: 1048576,
        reasoningLevels: ["low", "medium", "high", "xhigh", "max"],
        supportsParallelToolCalls: false,
        inputModalities: ["text"],
      })),
    ),
    category: "third_party",
    icon: "github",
    iconColor: "#000000",
  },
];
