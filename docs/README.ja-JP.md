# DSP-deepseekPartner

言語: [简体中文](../README.md) | [English](README.en-US.md) | **日本語**

DSP-deepseekPartner は、Android Studio AI、Copilot 系のサードパーティプラグイン、Claude Code、Cline、Roo、Kilo など、カスタム AI ソースに対応したツールを DeepSeek に接続するための、独立した macOS / Windows デスクトップアプリです。

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

- Android Studio AI: Anthropic-compatible custom source としてローカルプロキシを指定します。
- Copilot 系サードパーティプラグイン: custom OpenAI / Anthropic Base URL に対応したプラグインから利用できます。
- Claude Code: `thinking.type=adaptive`、effort mapping、reasoning replay、streamed thinking の互換処理を行います。
- Cline / Roo / Kilo / その他 Agent クライアント: custom OpenAI/Anthropic API endpoint に対応していれば利用できます。

## 機能

- 複数のローカルプロキシプロファイルを GUI で管理。
- プロファイルごとに `127.0.0.1:<port>` のサービスを起動。
- 単一プロファイルまたは全プロファイルの start/stop。
- Anthropic/OpenAI proxy URL と Claude Code 用 env snippet のコピー。
- Key を安定して送れないプラグイン向けの任意 DeepSeek API Key fallback。
- DeepSeek request context に注入する MCP service definition と Skill instruction を管理する独立 Settings ページ。
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

Android Studio AI が Anthropic-compatible custom source に対応している場合:

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

OpenAI-compatible custom source に対応したプラグイン:

```text
Schema: OpenAI-compatible
Base URL: http://127.0.0.1:17777/v1
API Key: your DeepSeek API key
Model: deepseek-v4-pro[1m]
```

Anthropic-compatible custom source に対応したプラグイン:

```text
Schema: Anthropic-compatible
Base URL: http://127.0.0.1:17777/anthropic
API Key: your DeepSeek API key
Model: deepseek-v4-pro[1m]
```

ここでの Copilot 系プラグインとは、サードパーティ AI source をカスタムできる IDE プラグインを指します。公式 GitHub Copilot がサードパーティソースを直接指定できるという意味ではありません。

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
- log では sensitive header と token-like value をマスクします。
- client setup は copy-only です。Android Studio、Claude Code、Cline、Roo、Kilo、各種 plugin の設定ファイルは自動変更しません。

## Changelog

### 1.1.0

- 有効な client key がない、空の `Bearer`、または一般的な placeholder key のリクエスト向けに、任意の DeepSeek API key fallback を追加。
- MCP service definition と Skill instruction を管理する独立 Settings ページを追加。
- Windows release build を GUI subsystem に切り替え、起動時に terminal が先に開かないように変更。

### 1.0.0

- 初回 macOS ARM64 / Windows x64 デスクトップリリース。
