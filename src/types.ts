export interface Provider {
  id: string;
  name: string;
  settingsConfig: Record<string, any>;
  meta?: ProviderMeta;
}

export interface AuthBinding {
  source: "provider_config" | "managed_account";
  authProvider?: string;
  accountId?: string;
}

export interface ProviderMeta {
  apiFormat?: "openai_chat" | "openai_responses";
  // An omitted selection uses Copilot's live endpoint capabilities.
  codexCopilotApiFormat?: CodexCopilotApiFormat;
  authBinding?: AuthBinding;
  providerType?: string;
  // Accept the saved account binding used by earlier provider records.
  githubAccountId?: string;
}

export type CodexCopilotApiFormat = "auto" | "openai_responses" | "openai_chat";

export interface CodexCatalogModel {
  model: string;
  displayName?: string;
  contextWindow?: string | number;
  supportsParallelToolCalls?: boolean;
  inputModalities?: string[];
  baseInstructions?: string;
  reasoningLevels?: string[];
  defaultReasoningLevel?: string;
}

// Application preferences belong to ~/.copilot-bridge-atlas/settings.json.
export interface Settings {
  showInTray: boolean;
  launchOnStartup?: boolean;
  usageDashboardRefreshIntervalMs?: number;
  language?: "en";
  currentProviderCodex?: string;
  backupIntervalHours?: number;
  backupRetainCount?: number;
}
