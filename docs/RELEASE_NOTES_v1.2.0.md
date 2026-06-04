# DSP-deepseekPartner v1.2.0

## Highlights

- MCP setup now uses JSON editing through `mcpConfig`, with the built-in `network-request` service enabled by default.
- Added executable `network_request` support for OpenAI-compatible non-streaming requests. When DeepSeek emits a `network_request` tool call, the gateway performs the HTTP/HTTPS request, sends the result back as a tool message, and returns the final model response.
- Legacy `mcpServices` settings are migrated into `mcpConfig.mcpServers` when loaded.

## Notes

- `network_request` currently prioritizes OpenAI-compatible non-streaming tool-call loops. Streaming requests keep the existing SSE and reasoning compatibility behavior.
- The built-in network tool supports `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, and `HEAD`, with a 1-30 second timeout and HTTP/HTTPS URLs only.
- Logs continue to redact sensitive headers and token-like values.

## Assets

- macOS Apple Silicon / ARM64 DMG: `DSP-deepseekPartner_1.2.0_macos-arm64.dmg`
- Windows x64 installer: `DSP-deepseekPartner_1.2.0_windows-x64-setup.exe`
- Windows x64 MSI: `DSP-deepseekPartner_1.2.0_windows-x64.msi`
