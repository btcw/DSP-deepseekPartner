# DSP-deepseekPartner

Language: [简体中文](../README.md) | **English** | [日本語](README.ja-JP.md)

DSP-deepseekPartner is an independent macOS and Windows desktop app that provides local DeepSeek proxy profiles for Android Studio AI, Copilot-style third-party plugins, Claude Code, Cline, Roo, Kilo, and other tools that support custom AI sources.

It is not an official DeepSeek application. By default, API keys are supplied by your client requests. If you explicitly set a fallback API key in a profile, the app stores it locally and injects it only when the original request has no usable key.

## What It Solves

Many IDE assistants and agent clients support OpenAI-compatible or Anthropic-compatible APIs, while DeepSeek has specific requirements around thinking, reasoning, tool calls, and streaming responses.

DSP-deepseekPartner starts a local `127.0.0.1:<port>` proxy, forwards client requests to DeepSeek, and normalizes these differences:

- DeepSeek thinking mode may require `reasoning_content` to be sent back after tool calls.
- Claude Code may send `thinking.type = "adaptive"`, which DeepSeek does not support directly.
- Some clients render streamed `thinking` / `reasoning_content` chunks as awkward spaces or sudden line breaks.
- Different plugins use different routes, model names, and effort parameter formats.

## Local API Surfaces

Anthropic-compatible:

```text
POST /v1/messages
POST /anthropic/v1/messages
GET  /anthropic/v1/models
```

OpenAI-compatible:

```text
POST /v1/chat/completions
POST /chat/completions
POST /anthropic/chat/completions
```

`/anthropic/chat/completions` exists for clients that combine an Anthropic base path with OpenAI chat completions.

## Client Scenarios

- Android Studio AI: use an Anthropic-compatible custom source pointed at the local proxy.
- Copilot-style third-party plugins: plugins with custom OpenAI or Anthropic Base URL support can connect through the proxy.
- Claude Code: compatibility for `thinking.type=adaptive`, effort mapping, reasoning replay, and streamed thinking.
- Cline / Roo / Kilo / other agent clients: any client that supports custom OpenAI/Anthropic API endpoints can connect.

## Features

- GUI profile management for multiple local proxy services.
- One `127.0.0.1:<port>` service per profile.
- Start/stop one profile or all profiles.
- Copy Anthropic/OpenAI proxy URLs and Claude Code environment snippets.
- Optional DeepSeek API key fallback for plugins that cannot reliably send a key.
- Dedicated Settings page for JSON-based MCP configuration and Skill instructions.
- Built-in `network_request` MCP tool for OpenAI-compatible non-streaming requests, allowing the gateway to execute HTTP/HTTPS requests and continue to the final model response.
- Live logs with request ID, status, latency, and upstream error body.
- Redaction for `Authorization`, `x-api-key`, and token-like values.
- Runtime-editable name, upstream URL, model mapping, timeout, log level, and feature toggles.
- Port changes require stopping the service first.
- Deleting a running profile stops it before deletion.

## Quick Start

1. Download the installer from the [Release page](https://github.com/btcw/DSP-deepseekPartner/releases).
2. Open DSP-deepseekPartner.
3. Add a profile:
   - Port: `17777`
   - Upstream URL: `https://api.deepseek.com`
   - API key fallback: optional; use it only when your client cannot send a key
   - API Surfaces: enable Anthropic and/or OpenAI as needed
4. Click Start.
5. Copy the proxy URL or environment snippet into your IDE, plugin, or agent client.

## Android Studio AI

If Android Studio AI supports an Anthropic-compatible custom source:

```text
Schema: Anthropic-compatible
Base URL: http://127.0.0.1:17777/anthropic
API Key: your DeepSeek API key
Model: deepseek-v4-pro[1m]
```

Model refresh endpoint:

```text
http://127.0.0.1:17777/anthropic/v1/models
```

## Copilot-Style Plugins

For plugins that support an OpenAI-compatible custom source:

```text
Schema: OpenAI-compatible
Base URL: http://127.0.0.1:17777/v1
API Key: your DeepSeek API key
Model: deepseek-v4-pro[1m]
```

For plugins that support an Anthropic-compatible custom source:

```text
Schema: Anthropic-compatible
Base URL: http://127.0.0.1:17777/anthropic
API Key: your DeepSeek API key
Model: deepseek-v4-pro[1m]
```

Copilot-style plugins here means IDE plugins that support custom third-party AI sources. It does not imply that official GitHub Copilot can be pointed to a third-party source.

## MCP JSON

The Settings page stores MCP configuration as JSON. The default config includes the built-in network request tool:

```json
{
  "mcpServers": {
    "network-request": {
      "type": "builtin",
      "enabled": true,
      "tool": "network_request",
      "description": "HTTP/HTTPS request helper executed by DSP-deepseekPartner"
    }
  }
}
```

`network_request` currently targets OpenAI-compatible non-streaming requests first. When the model calls this tool, the gateway performs the HTTP/HTTPS request, sends the tool result back to DeepSeek, and returns the final answer. Streaming requests keep the existing SSE and reasoning compatibility behavior.

## Claude Code Snippet

macOS / Linux:

```sh
export ANTHROPIC_BASE_URL=http://127.0.0.1:17777/anthropic
export ANTHROPIC_AUTH_TOKEN=<your DeepSeek API Key>
export ANTHROPIC_MODEL=deepseek-v4-pro[1m]
export ANTHROPIC_DEFAULT_OPUS_MODEL=deepseek-v4-pro[1m]
export ANTHROPIC_DEFAULT_SONNET_MODEL=deepseek-v4-pro[1m]
export ANTHROPIC_DEFAULT_HAIKU_MODEL=deepseek-v4-flash
export CLAUDE_CODE_SUBAGENT_MODEL=deepseek-v4-flash
export CLAUDE_CODE_EFFORT_LEVEL=max
```

Windows PowerShell:

```powershell
$env:ANTHROPIC_BASE_URL="http://127.0.0.1:17777/anthropic"
$env:ANTHROPIC_AUTH_TOKEN="<your DeepSeek API Key>"
$env:ANTHROPIC_MODEL="deepseek-v4-pro[1m]"
$env:ANTHROPIC_DEFAULT_OPUS_MODEL="deepseek-v4-pro[1m]"
$env:ANTHROPIC_DEFAULT_SONNET_MODEL="deepseek-v4-pro[1m]"
$env:ANTHROPIC_DEFAULT_HAIKU_MODEL="deepseek-v4-flash"
$env:CLAUDE_CODE_SUBAGENT_MODEL="deepseek-v4-flash"
$env:CLAUDE_CODE_EFFORT_LEVEL="max"
```

## Development

```sh
npm install
npm run tauri:dev
```

Frontend checks:

```sh
npm test
npm run build
```

Rust tests:

```sh
cd src-tauri
cargo test
```

Build installers:

```sh
npm run tauri:build:mac
npm run tauri:build:windows
```

Debug binary only:

```sh
npm run tauri:build:binary
```

## Distribution

- macOS Apple Silicon / ARM64 packages include `macos-arm64` in the filename.
- Windows x64 packages include `windows-x64` in the filename.
- Windows release builds use the GUI subsystem, so double-click launch does not open a terminal first.
- Unsigned and unnotarized macOS builds may still trigger Apple Gatekeeper warnings after download.

## Safety

- The proxy listens on `127.0.0.1` by default.
- The app does not require storing DeepSeek API keys by default. If you set a fallback key, it is written to the local app config directory and is used only when a request has no usable key.
- Built-in `network_request` accesses HTTP/HTTPS URLs when the model calls the tool; enable it only for trusted local client configurations.
- Logs redact sensitive headers and token-like values.
- Client setup is copy-only; the app does not automatically modify Android Studio, Claude Code, Cline, Roo, Kilo, or plugin configuration files.

## Changelog

### 1.2.0

- Switched MCP setup to JSON editing, with the built-in `network-request` service enabled by default.
- Added executable `network_request` tool support for OpenAI-compatible non-streaming tool-call loops.
- Legacy `mcpServices` settings are migrated into `mcpConfig.mcpServers` when loaded.

### 1.1.0

- Added optional DeepSeek API key fallback for requests with no usable client key, empty `Bearer`, or common placeholder key values.
- Added a dedicated Settings page for MCP service definitions and Skill instructions.
- Switched Windows release builds to the GUI subsystem so launch no longer opens a terminal first.

### 1.0.0

- First macOS ARM64 and Windows x64 desktop release.
