CC Switch Atlas 4.1.0 is a Windows x64 bridge from Codex to GitHub Copilot.

- One default Copilot entry with **Needs setup**, sign-in, model controls and **Edit**.
- Read-only, Git-style TOML previews for migrating another proxy to Atlas or returning to OpenAI sign-in. Preferences and comments are preserved.
- Live capability import fixes Astra parallel tools and Luna image support, and respects Copilot's actual input limits and reasoning choices. Model discovery uses the Atlas-owned catalog without depending on Codex TOML.
- Fixes premature compaction for Astra and Luna by telling Codex that Copilot already counts replayed reasoning. Usage values and the user's compaction settings stay intact.
- Retains streaming message-ID compatibility, usage charts, cache statistics, Copilot quota, outbound networking and local app-data backups.
- Imported conversation totals no longer inflate proxy usage; historical database rows are preserved.
- Removes other clients/providers, configuration takeover, duplicate/delete/usage-script actions, failover, cloud sync, CLI tools, and MCP/skill/instruction/session management. English-only; Windows x64 MSI only.

Download **CC-Switch-atlas-4.1.0-Windows-x64.msi** and its adjacent SHA256 file.
Exit the previous CC Switch app before installing. This unsigned per-user installer
also replaces the provisional 4.1.0 build. Atlas does not modify Codex TOML, auth,
instructions, skills or sessions. Review **Codex connection**, apply its preview
manually and reload Codex to use the Atlas-owned model catalog.

Validation: TypeScript checks, all 201 frontend tests and 1,640 Rust tests passed.
Four optional platform/performance checks remain skipped. Live Copilot probes verified
Astra parallel tool calls, Luna image input, and reasoning-token accounting for both models.
