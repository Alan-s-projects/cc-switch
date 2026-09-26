# Copilot Bridge Atlas

A Windows x64 desktop app connecting **Codex → GitHub Copilot → GPT**, with a
local OpenAI-compatible server, account management, usage statistics, and
read-only Codex configuration previews.

## Install and connect

Download `Copilot-Bridge-Atlas-4.2.1-Windows-x64.msi` from
[Releases](https://github.com/Alan-s-projects/copilot-bridge-atlas/releases).
The installer is per-user and unsigned.

1. Open **Settings → Copilot**, sign in to GitHub, refresh your GPT models, and save.
2. Turn on the proxy switch. The default address is `http://127.0.0.1:15722/v1`.
3. Open **Connect**, review the proposed TOML, copy it, and apply it yourself.
4. Reload Codex so it loads the selected provider and generated model catalog.

**Atlas never writes Codex configuration, authentication, MCP, skills,
instructions, or conversation files.** Connect offers side-by-side, inline, and
full-file previews for the bridge and for returning to OpenAI sign-in. A manual
TOML location changes the preview only.

Saved reasoning levels and their default survive app restarts and model refresh.
New models inherit Copilot's advertised reasoning defaults. Live image/parallel-tool
declarations continue to refresh, and saved context sizes are capped to the reported
input budget. Atlas forwards unsupported-image errors without retrying with images
removed.

The optional 1M preset proposes a 1,000,000-token context and compaction at 900,000,
capped for models with a smaller saved limit. Changes affect the proposal only.

## Pages

- **Overview:** Provider, Proxy, Today's usage, and Requests cards; endpoint, quota,
  estimated cost, cache reuse, and the latest ten requests. Health check measures
  endpoint reachability, without an inference or authentication test.
- **Usage:** history, trends, model/provider statistics, and global GPT pricing.
  Historical recorded costs are preserved. Imported conversation totals are excluded.
  Token costs are estimates, not a Copilot subscription bill.
- **Connect:** configuration detection, comparison, copying, and optional settings.
- **Settings:** appearance/startup, outbound networking, GitHub authentication,
  GPT catalog/protocol choices, logs, application-data location, and local backups.

Home usage updates are coalesced from request events instead of idle SQL polling.
Status and quota polling pause while the window is inactive. Close the window to
keep the bridge in the tray; use **Quit** to exit.

## Independent application identity

- Process: `copilot-bridge-atlas.exe`
- Application ID: `com.alansprojects.copilotbridgeatlas`
- Data directory: `%USERPROFILE%\.copilot-bridge-atlas`
- Database: `copilot-bridge-atlas.db`
- Generated catalog: `copilot-model-catalog.json` inside the data directory

The app uses its own installer identity, settings, logs, startup entry, and WebView
profile. It does not discover or reuse another application's data directory.
Local SQL and database backups can be imported through Settings.
An application-data directory change is saved for the next launch; choosing
Restart Later keeps settings, backups and the open database in their current folder.

## Develop and release

Requires Windows x64, Node.js, pnpm, the pinned Rust toolchain, Visual Studio C++
Build Tools, and WebView2.

```powershell
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test:unit --maxWorkers=4 --minWorkers=1
./scripts/build-msi.ps1
```

Run Rust tests with isolated application data:

```powershell
$atlasTestHome = Join-Path $env:TEMP ("atlas-tests-" + [guid]::NewGuid().ToString("N"))
$env:COPILOT_BRIDGE_ATLAS_TEST_HOME = $atlasTestHome
cargo test --manifest-path src-tauri/Cargo.toml --locked --offline -- --test-threads=1
```

The local build writes the MSI and SHA256 file to `release/`. Use a reviewed PR
and squash merge into `atlas`, verify that the built source matches the merged
tree, then publish an `atlas-<version>` tag and upload those two explicit files.
There is no cloud build workflow. Only `atlas` remains after temporary PR branches
are removed.

[MIT license and copyright notice](LICENSE).
