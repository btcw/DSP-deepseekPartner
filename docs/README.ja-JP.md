# DSP-deepseekPartner

言語: [简体中文](../README.md) | [English](README.en-US.md) | **日本語**

DSP-deepseekPartner は、**Android Studio AI で DeepSeek を使えるようにすること**を重視した、独立した macOS / Windows デスクトップアプリです。Claude Code、Cline、Roo、Kilo など、custom OpenAI / Anthropic API source に対応したツール向けのローカル DeepSeek proxy としても利用できます。

これは DeepSeek 公式アプリではありません。デフォルトでは API Key はクライアント側のリクエストで送信されます。プロファイルに fallback API Key を明示的に設定した場合のみ、アプリはそれをローカルに保存し、元のリクエストに有効な Key がないときだけ注入します。

## 解決する問題

多くの IDE アシスタントや Agent クライアントは OpenAI-compatible / Anthropic-compatible API に対応しています。一方で DeepSeek には thinking、reasoning、tool call、streaming response に関する互換要件があります。

DSP-deepseekPartner は `127.0.0.1:<port>` のローカルプロキシを起動し、クライアントからのリクエストを DeepSeek に転送しながら、次の差分を吸収します。

- DeepSeek thinking mode では、tool call の後に `reasoning_content` を返す必要がある場合があります。
- Claude Code は `thinking.type = "adaptive"` を送ることがありますが、DeepSeek はこの値を直接サポートしていません。
- 一部のクライアントでは、SSE の `thinking` / `reasoning_content` が細かく表示され、不自然な空白や改行になることがあります。
- プラグインごとに route、model name、effort parameter の形式が異なります。

## ローカル API

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

`/anthropic/chat/completions` は、Anthropic base path と OpenAI chat completions を組み合わせて使うクライアント向けです。

## クライアント例

- Android Studio AI: Anthropic-compatible custom source としてローカルプロキシを指定する構成を検証済みです。Android Studio から DeepSeek を利用できます。
- Claude Code: `thinking.type=adaptive`、effort mapping、reasoning replay、streamed thinking の互換処理を行います。
- Cline / Roo / Kilo / その他 Agent クライアント: custom OpenAI/Anthropic API endpoint に対応していれば利用できます。
- Copilot plugin: 現時点では安定して利用できる構成を確認できていないため、サポート対象としては推奨していません。

## 機能

- 複数のローカルプロキシプロファイルを GUI で管理。
- プロファイルごとに `127.0.0.1:<port>` のサービスを起動。
- 単一プロファイルまたは全プロファイルの start/stop。
- Anthropic/OpenAI proxy URL と Claude Code 用 env snippet のコピー。
- Key を安定して送れないプラグイン向けの任意 DeepSeek API Key fallback。
- JSON ベースの MCP configuration と Skill instruction を管理する独立 Settings ページ。
- OpenAI-compatible の non-streaming request 向けに、HTTP/HTTPS request を実行して最終応答まで継続する内蔵 `network_request` MCP tool。
- request ID、status、latency、upstream error body を含む live log。
- `Authorization`、`x-api-key`、token-like value のログマスク。
- 起動中でも name、upstream URL、model mapping、timeout、log level、feature toggle を編集可能。
- port 変更には停止が必要です。
- 実行中のプロファイル削除時は、停止してから削除します。

## Quick Start

1. [Release page](https://github.com/btcw/DSP-deepseekPartner/releases) からインストーラーをダウンロードします。
2. DSP-deepseekPartner を開きます。
3. プロファイルを追加します。
   - Port: `17777`
   - Upstream URL: `https://api.deepseek.com`
   - API key fallback: 任意。クライアントが Key を送れない場合だけ設定します
   - API Surfaces: 必要に応じて Anthropic / OpenAI を有効化
4. Start をクリックします。
5. proxy URL または env snippet を IDE / plugin / agent client に設定します。

## Android Studio AI

これは DSP-deepseekPartner で現在重点的に検証している利用シーンです。以下のように設定すると、Android Studio AI からローカル proxy 経由で DeepSeek を利用できます。

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

## Copilot 系プラグイン

Copilot plugin は、現時点では DSP-deepseekPartner の推奨対象ではありません。OpenAI-compatible / Anthropic-compatible の third-party source 設定では、ローカル検証で安定して動作する構成を確認できていません。

ここでの Copilot plugin とは IDE の Copilot 関連 plugin / extension を指します。公式 GitHub Copilot が DeepSeek に直接切り替えられるという意味ではありません。この利用シーンが検証できた時点で、明確な設定手順を追加します。

## MCP JSON

Settings ページの MCP configuration は JSON で保存します。デフォルトでは内蔵 network request tool が含まれます。

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

現在の `network_request` は、まず OpenAI-compatible の non-streaming request を対象にしています。モデルがこの tool を呼び出すと、gateway が HTTP/HTTPS request を実行し、tool result を DeepSeek に返してから最終応答を返します。streaming request は既存の SSE と reasoning compatibility behavior を維持します。

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

## 配布について

- macOS Apple Silicon / ARM64 package には `macos-arm64` が含まれます。
- Windows x64 package には `windows-x64` が含まれます。
- Windows release build は GUI subsystem を使用するため、ダブルクリック起動時に先に terminal window は開きません。
- macOS build は Apple Developer ID signing / notarization がない場合、ダウンロード後に Gatekeeper warning が表示されることがあります。

## Safety

- proxy はデフォルトで `127.0.0.1` のみに bind します。
- デフォルトでは DeepSeek API key をアプリに保存する必要はありません。fallback Key を設定した場合はローカルのアプリ設定ディレクトリに保存され、有効な Key がないリクエストでのみ使用されます。
- 内蔵 `network_request` は model が tool を呼び出したときに HTTP/HTTPS URL へアクセスします。信頼できるローカルクライアント設定でのみ有効化してください。
- log では sensitive header と token-like value をマスクします。
- client setup は copy-only です。Android Studio、Claude Code、Cline、Roo、Kilo、各種 plugin の設定ファイルは自動変更しません。

## Changelog

### 1.2.0

- MCP setup を JSON editing に変更し、内蔵 `network-request` service をデフォルトで有効化。
- OpenAI-compatible non-streaming tool-call loop 向けに、実行可能な `network_request` tool support を追加。
- 旧 `mcpServices` settings は読み込み時に `mcpConfig.mcpServers` へ移行されます。

### 1.1.0

- 有効な client key がない、空の `Bearer`、または一般的な placeholder key のリクエスト向けに、任意の DeepSeek API key fallback を追加。
- MCP service definition と Skill instruction を管理する独立 Settings ページを追加。
- Windows release build を GUI subsystem に切り替え、起動時に terminal が先に開かないように変更。

### 1.0.0

- 初回 macOS ARM64 / Windows x64 デスクトップリリース。
