# DSP-deepseekPartner

语言切换：**简体中文** | [English](docs/README.en-US.md) | [日本語](docs/README.ja-JP.md)

DSP-deepseekPartner 是一个独立的 macOS / Windows 桌面应用，用来为 Android Studio AI、Copilot 类第三方插件、Claude Code、Cline、Roo、Kilo 等支持自定义 AI 源的工具提供本地 DeepSeek 代理。

它不是 DeepSeek 官方应用。默认仍由客户端请求携带 API Key；如果你主动在配置里填写 fallback API Key，应用只会在原请求没有有效 Key 时注入，并保存到本机配置中。

## 它解决什么问题

很多 IDE AI 工具和 Agent 客户端支持 OpenAI-compatible 或 Anthropic-compatible API，但 DeepSeek 在 thinking/reasoning、工具调用和流式返回上有一些兼容要求。

DSP-deepseekPartner 在本地启动 `127.0.0.1:<port>` 代理，把客户端请求转发到 DeepSeek，并处理这些差异：

- DeepSeek thinking mode 要求工具调用后回传 `reasoning_content`。
- Claude Code 可能发送 `thinking.type = "adaptive"`，DeepSeek 不支持这个参数。
- 某些客户端会把 SSE 里的 `thinking` / `reasoning_content` 渲染成很碎的文字、空格或突兀换行。
- 不同插件使用的路由、模型名、effort 参数格式不一致。

## 支持的本地接口

Anthropic-compatible：

```text
POST /v1/messages
POST /anthropic/v1/messages
GET  /anthropic/v1/models
```

OpenAI-compatible：

```text
POST /v1/chat/completions
POST /chat/completions
POST /anthropic/chat/completions
```

其中 `/anthropic/chat/completions` 用于兼容一些把 Anthropic base path 和 OpenAI chat completions 混用的客户端。

## 支持的客户端场景

- Android Studio AI：使用 Anthropic-compatible schema 指向本地代理。
- Copilot 类第三方插件：支持自定义 OpenAI 或 Anthropic Base URL 的插件可以接入。
- Claude Code：处理 `thinking.type=adaptive`、effort 映射、reasoning replay 等 DeepSeek 兼容问题。
- Cline / Roo / Kilo / 其它 Agent 客户端：只要支持自定义 OpenAI/Anthropic API 地址即可接入。

## 主要功能

- 图形化管理多个本地代理配置。
- 每个配置一个 `127.0.0.1:<port>` 本地服务。
- 一键启动/停止单个配置或全部配置。
- 复制 Anthropic/OpenAI 代理链接和 Claude Code 环境变量片段。
- 配置可选 DeepSeek API Key fallback，兼容不能稳定传 Key 的插件。
- 独立 Settings 页，使用 JSON 维护 MCP 配置和 Skill 指令。
- 内置 `network_request` MCP 工具，OpenAI-compatible 非流式请求可自动执行 HTTP/HTTPS 网络请求并继续生成最终回复。
- 查看实时日志，请求 ID、状态码、延迟和上游错误体。
- 自动脱敏 `Authorization`、`x-api-key` 和 token-like 内容。
- 启动后可动态修改名称、上游 URL、模型映射、超时、日志等级和特性开关。
- 端口修改需要先停止服务。
- 删除运行中的配置时会先停止服务再删除。

## 快速开始

1. 从 [Release 页面](https://github.com/btcw/DSP-deepseekPartner/releases) 下载对应系统的安装包。
2. 打开 DSP-deepseekPartner。
3. 新增配置：
   - Port: `17777`
   - Upstream URL: `https://api.deepseek.com`
   - API Key fallback: 可留空；只有客户端请求没有 Key 时才需要填写
   - API Surfaces: Anthropic / OpenAI 按需启用
4. 点击 Start。
5. 复制代理链接或环境变量片段到你的 IDE / 插件 / Agent 客户端。

## Android Studio AI 配置

如果 Android Studio AI 支持 Anthropic-compatible custom source，可以这样配置：

```text
Schema: Anthropic-compatible
Base URL: http://127.0.0.1:17777/anthropic
API Key: your DeepSeek API key
Model: deepseek-v4-pro[1m]
```

模型刷新接口：

```text
http://127.0.0.1:17777/anthropic/v1/models
```

## Copilot 类插件配置

如果插件支持 OpenAI-compatible custom source：

```text
Schema: OpenAI-compatible
Base URL: http://127.0.0.1:17777/v1
API Key: your DeepSeek API key
Model: deepseek-v4-pro[1m]
```

如果插件支持 Anthropic-compatible custom source：

```text
Schema: Anthropic-compatible
Base URL: http://127.0.0.1:17777/anthropic
API Key: your DeepSeek API key
Model: deepseek-v4-pro[1m]
```

这里的 Copilot 类插件指支持自定义三方 AI 源的 IDE 插件，不表示官方 GitHub Copilot 可以直接修改三方源。

## MCP JSON 配置

Settings 页里的 MCP 配置使用 JSON。默认会包含内置网络请求工具：

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

当前 `network_request` 优先支持 OpenAI-compatible 非流式请求。模型触发该工具时，网关会执行 HTTP/HTTPS 请求，把结果作为 tool message 回传给 DeepSeek，然后返回最终回答。流式请求仍保持原有 SSE 转发和 reasoning 兼容逻辑。

## Claude Code 片段

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

## 开发

```sh
npm install
npm run tauri:dev
```

前端检查：

```sh
npm test
npm run build
```

Rust 测试：

```sh
cd src-tauri
cargo test
```

构建安装包：

```sh
npm run tauri:build:mac
npm run tauri:build:windows
```

仅构建调试二进制：

```sh
npm run tauri:build:binary
```

## 发布说明

- macOS Apple Silicon / ARM64 包名包含 `macos-arm64`。
- Windows x64 包名包含 `windows-x64`。
- Windows release 版本使用 GUI 子系统，双击启动不会先弹出终端窗口。
- macOS 包如果没有 Apple Developer ID 签名和 notarization，从浏览器或云盘下载后仍可能出现 Gatekeeper 验证提示。

## 安全说明

- 默认只监听 `127.0.0.1`。
- 默认不需要在应用里保存 DeepSeek API Key；如果填写 fallback Key，会写入本机应用配置目录，仅在请求缺少有效 Key 时使用。
- 内置 `network_request` 会按模型工具调用访问 HTTP/HTTPS URL，只建议在信任的本地客户端配置中启用。
- 日志会脱敏敏感 header 和 token-like 内容。
- 客户端配置仅提供复制片段，不会自动改写 Android Studio、Claude Code、Cline、Roo、Kilo 或其它插件配置文件。

## 版本记录

### 1.2.0

- MCP 配置改为 JSON 编辑，默认包含内置 `network-request` 服务。
- 新增可执行的 `network_request` 工具，支持 OpenAI-compatible 非流式工具调用闭环。
- 旧版 `mcpServices` 配置会在读取时兼容迁移到 `mcpConfig.mcpServers`。

### 1.1.0

- 新增可选 DeepSeek API Key fallback，客户端请求为空、空 `Bearer` 或常见占位 Key 时自动补充。
- 新增独立 Settings 页，支持维护 MCP 服务定义和 Skill 指令，并注入到 DeepSeek 请求上下文。
- Windows release 构建切换为 GUI 子系统，双击启动不再先出现终端窗口。

### 1.0.0

- 首个 macOS ARM64 / Windows x64 桌面应用版本。
