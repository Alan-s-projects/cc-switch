# CC Switch Atlas

A Windows x64 desktop bridge from **Codex** to **GitHub Copilot**, with Copilot
sign-in, a local Responses-compatible server and a usage dashboard. Atlas starts
at **4.1.0**. Its only development branch is `atlas`.

## Install and connect

Download `CC-Switch-atlas-4.1.1-Windows-x64.msi` from
[GitHub Releases](https://github.com/Alan-s-projects/cc-switch/releases).
Exit the previous CC Switch app, install the MSI, and open Atlas.

1. The one **GitHub Copilot** entry starts with **Needs setup**. Choose **Edit**,
   sign in, refresh your models, and save.
2. Start the proxy and open **Codex connection**.
3. Review **Connect through Copilot** in **Side by side**, **Inline**, or **Proposed TOML**.
   Copy and apply the proposed changes yourself, then reload Codex.
4. To reconnect using your OpenAI account, review **Return to OpenAI sign-in**,
   apply that preview, and sign in through Codex.

**Atlas never writes Codex configuration, authentication, instructions, skills,
MCP or conversation files.** The previews replace an existing proxy connection,
including conflicting authentication and catalog settings, while keeping unrelated
preferences and comments. Atlas reuses the active custom provider ID when possible.
Its own generated catalog is `.cc-switch/copilot-model-catalog.json`; the preview
includes its absolute path. A port change requires applying a new preview manually.

When upgrading from an older CC Switch build, that older app may restore its saved
Codex configuration when it exits. Review the Atlas connection preview afterwards.
The MSI is unsigned, installs per user, and supports replacing the provisional
4.1.0 installer.

## Scope

- One Copilot provider and account management, with model and protocol controls.
- Automatic routing from each model's live Copilot endpoint declarations.
- Model capabilities, reasoning choices and input budgets from Copilot. Existing
  smaller context limits are retained; limits above the reported input budget are capped.
- Proxy start/stop, outbound networking and request compatibility controls.
- A manual health check beside **Edit** on the Copilot card shows endpoint
  reachability and latency without sending a model request.
- Usage trends, request history, cache statistics, latency and estimated costs;
  Copilot subscription quota is shown separately.
- Local application-data backups.

Atlas is English-only and builds only a Windows x64 MSI. Other clients, native
provider authentication, provider add/duplicate/delete actions, usage scripts,
manual request overrides, failover, cloud sync, CLI management, and MCP/skill/instruction/session management
are removed. Legacy database data stays available in backups. Dashboard statistics
count proxy traffic, excluding imported conversation totals and their historical
rollups. Conversation files are never scanned. Estimated token costs are not a
GitHub bill.

## Copilot compatibility findings

Copilot's live catalog confirmed parallel tools and image input for both GPT-6
Astra and Luna. Atlas now preserves those declarations and refreshes saved flags
at startup. Luna's reported input limit was 872,000 tokens despite a one-million
total context window; Atlas respects the smaller limit.

Astra and Luna count replayed encrypted reasoning in `usage.input_tokens`, but
Copilot omits the `x-reasoning-included` header expected by
[Codex's response reader](https://github.com/openai/codex/blob/86be5320b068ef67b56348b02aa8c33706955da6/codex-rs/codex-api/src/sse/responses.rs).
Without that marker, [Codex's context counter](https://github.com/openai/codex/blob/86be5320b068ef67b56348b02aa8c33706955da6/codex-rs/core/src/context_manager/history.rs)
adds another estimate from the encrypted history, causing premature compaction.
Paired live probes on 2026-09-25 reported 125 versus 60 input tokens for Astra,
and 167 versus 60 for Luna, with versus without the replayed reasoning item.
Atlas supplies the marker for these verified models while preserving all usage
values and encrypted reasoning. Other models keep their upstream accounting.
Actual context limits still trigger compaction; Atlas does not change the user's
[compaction settings](https://developers.openai.com/codex/config-reference/).

Copilot Responses item IDs are also stabilized per output index to avoid duplicate
messages. Tool call IDs, response cursors, cache keys and message content are preserved.

## Develop and build

Requires Windows x64, Node.js 22, pnpm 10, the pinned Rust toolchain, Visual Studio
C++ Build Tools and WebView2.

```powershell
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test:unit --maxWorkers=4 --minWorkers=1
./scripts/build-msi.ps1
```

The build creates the MSI and its SHA256 file in `release/`. The sole workflow
tests and builds Windows x64; an `atlas-<version>` tag publishes the installer.
All three manifests must use the same version.

Run backend tests with isolated application data:

```powershell
$atlasTestHome = Join-Path $env:TEMP ("atlas-tests-" + [guid]::NewGuid().ToString("N"))
$atlasTestData = Join-Path $atlasTestHome ".cc-switch"
New-Item -ItemType Directory -Path $atlasTestData -Force | Out-Null
[IO.File]::WriteAllBytes((Join-Path $atlasTestData "cc-switch.db"), [byte[]]@())
$env:CC_SWITCH_TEST_HOME = $atlasTestHome
cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1
```

The bridge tests verify client files remain byte-for-byte unchanged across startup,
provider edits, proxy changes and backup restoration, including legacy takeover data.

## Credits

[MIT](LICENSE). Based on [CC Switch](https://github.com/farion1231/cc-switch) and
[Copilot capability routing](https://github.com/ljie-PI/cc-switch/tree/feat/codex-copilot-capability-routing),
with protocol behavior compared against the user's existing Copilot Bridge implementation.
