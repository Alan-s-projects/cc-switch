Atlas 4.1.1 simplifies Copilot setup and makes the read-only Codex configuration comparison easier to review.

- Removes custom User-Agent/presets, manual header/body overrides, and the section's manual Chat cache/thinking switches.
- Removes the Bedrock Request Optimizer, its thinking/cache controls, backend settings, and request-processing code.
- Removes per-provider pricing controls and overrides. New usage uses the global Codex pricing settings; historical usage records are preserved.
- Deletes the supporting frontend and backend code and obsolete tests. Previously saved override fields are ignored.
- Shows current TOML on the left and proposed TOML on the right by default, with aligned changes and line numbers. Switch to an inline comparison or the complete proposed TOML.
- Uses the same comparison for Copilot setup and restoring OpenAI sign-in. Copy and refresh remain available; configuration files stay read-only.
- Restores the compact green on/off switch in the header for starting and stopping the local proxy.
- Restores the heartbeat health-check button beside Edit on the Copilot card, with reachability, latency, and HTTP status results. Copilot endpoint discovery respects the configured check timeout.
- Displays **CC Switch Atlas** in the application header.
- Always minimizes the main window to the system tray when closed, keeping the proxy running. Removes the close-behavior setting; use the tray menu's Quit action to exit.
- Retains protocol selection, model catalog and per-model reasoning controls, automatic Copilot compatibility, and usage statistics.

Download **CC-Switch-atlas-4.1.1-Windows-x64.msi** and its SHA256 file. Close CC Switch before installing. This is an unsigned Windows x64 installer and upgrades Atlas 4.1.0. No Codex TOML changes are required for this update.

Validation: TypeScript checks, all 196 frontend tests, all 1,622 Rust tests, and the production renderer build passed. Four optional platform/performance checks remain skipped. Coverage includes complete file comparisons, read-only copy/refresh, proxy controls, legacy settings, exit handling, global pricing, health checks and timeouts, and automatic Copilot protocol compatibility. The Windows pipeline also builds the MSI before release.
