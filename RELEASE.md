Copilot Bridge Atlas 4.2.0 is an independent Windows x64 app for Codex, GitHub Copilot, and GPT.

- Uses its own product name, executable, application ID, installer upgrade identity, icon, data folder, database, logs, startup entry, and repository.
- Defaults to loopback port **15722**, allowing it to coexist with a different bridge.
- Fixes saved reasoning levels and defaults being overwritten by startup or **Refresh models**. Explicit saved choices remain intact; unconfigured models receive live defaults.
- Removes inactive provider protocols, other-client management, cloud-sync remnants, obsolete configuration migrations, unused API wrappers, foreign pricing rules, unused logos, and unsupported-platform assets.
- Keeps GPT Responses/Chat routing, GitHub accounts and enterprise endpoints, request/response compatibility, image/tool handling, stable streamed message IDs, reasoning accounting, and accurate cache usage.
- Preserves the four-page navigation, overview cards, health check, manual refresh, global pricing, local backups, and read-only Codex previews.
- Imported SQLite data retains recorded costs and opaque historical data. The app never rewrites Codex files.
- Keeps active settings, backup and database paths together until restart after an application-data directory change.

Download **Copilot-Bridge-Atlas-4.2.0-Windows-x64.msi** and its SHA256 file. This unsigned per-user installer installs as a separate application. Review and apply its Connect preview before pointing Codex at the new endpoint.

Local validation: TypeScript and all **208 frontend tests** passed; all **476 Rust tests** passed with one optional performance check skipped and no compiler warnings. Tests cover saved reasoning across restart, version-19 database import, historical costs, blank cache-key fallback, real loopback Responses/Chat forwarding, images/tools, compressed errors without retries, and application-data isolation. Synthetic browser checks covered the renamed overview, settings/catalog and connection previews, including the 900×600 minimum layout.
