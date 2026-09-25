Atlas 4.1.0 focuses CC Switch on Codex with GitHub Copilot.

- Retains Copilot accounts, models, routing and the usage dashboard.
- Replaces native configuration takeover with read-only Codex connection suggestions.
- Removes other client/provider interfaces and MCP, skill, instruction, profile and session management.
- Ships an English-only Windows x64 MSI.

Download **CC-Switch-atlas-4.1.0-Windows-x64.msi**. The adjacent `.sha256` file
contains its checksum. This installer is unsigned.

Exit the previous CC Switch version before installing. After starting Atlas,
open **Codex connection** and manually apply any suggested connection changes.
Atlas never edits your Codex TOML, auth, instructions, skills or session files.
Existing app data and usage history are retained.

Validation: TypeScript checks, 432 frontend tests and 2,120 Rust tests passed.
Six optional or environment-dependent Rust tests are skipped.
