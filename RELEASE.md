Atlas 4.1.3 adds a read-only home overview, clearer navigation, and explicit TOML choices, while removing request rectifiers and image fallback.

- Adds a home overview with Copilot account/quota, proxy address and active requests, today's request count, estimated cost, success/cache rates, and the latest ten requests with HTTP status codes.
- Adds warnings when the proxy is stopped or the detected Codex TOML points elsewhere. **Refresh overview** manually refreshes status, usage, recent requests, quota, and the read-only configuration check.
- Keeps home usage updates event-driven, coalescing bursts over five seconds. A day-boundary update keeps today's totals correct without idle usage polling. Status and quota polling pause while the window is inactive; an unused animation timer is removed.
- Moves provider editing into **Settings → Copilot**, immediately after Auth. Account management preserves unsaved provider edits.
- Moves **Connect** and **Health check** to the top bar alongside labeled **Usage**, **Settings**, and **Proxy** controls. The original green proxy switch remains.
- Removes the Request compatibility section, its settings commands, the Anthropic thinking signature/budget rectifiers, image stripping, and image-specific retries.
- Keeps image payloads intact. If the upstream rejects image input, Atlas returns that error instead of retrying without the image. Chat conversion also preserves image detail and explicitly rejects file-ID-only images it cannot represent.
- Replaces the previous context-default checkboxes with **Use 1M context**, proposing `model_context_window = 1000000` and `model_auto_compact_token_limit = 900000`.
- Caps the Copilot preset to the selected model's saved Atlas catalog limit, including a model selected by a profile in the same file. For example, an 872,000-token limit produces an 872,000-token context and compaction at 784,800. The OpenAI preview uses the uncapped preset; the selected model and provider must support it.
- Adds **Compare with Codex defaults** for explicit approval-policy, sandbox, and reasoning overrides. Each row shows the current value and documented default, with a separate optional checkbox. Choices affect the proposal only and reset when selecting another file.
- Moves **Copy proposed TOML** and **Copy diff** to the comparison toolbar. Side-by-side, inline, and full TOML views scroll inside the code panel while the page controls and copy actions stay visible.
- Moves **Usage Statistics** out of Settings into its own page. The chart button opens it directly; its dashboard, filters, history, and pricing controls are preserved.
- Removes obsolete code and tests while preserving model capability declarations, response IDs, reasoning accounting, usage history, and all Codex files.

Download **CC-Switch-atlas-4.1.3-Windows-x64.msi** and its SHA256 file. Use the tray menu's **Quit** action before installing. This unsigned Windows x64 installer upgrades existing Atlas installations.

Validation: TypeScript, all 224 frontend tests, and all 1,535 Rust tests passed. Four optional platform/performance checks remain skipped. Windows x64 packaging is checked by the release workflow. Local mock-upstream tests verify intact images, the original HTTP 400, one attempt, and failure accounting for Responses and Chat. Regression coverage includes read-only files, context caps, selected profiles, stale-copy protection, navigation, home warnings, manual refresh, and bounded background activity. Synthetic browser checks covered the minimum 900×600 window, larger windows, home warnings, all navigation destinations, all three code views, current/default comparisons, and scrolling confined to the code panel.
