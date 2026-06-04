# DSP-deepseekPartner v1.1.0

## Highlights

- Added optional DeepSeek API key fallback per profile. The gateway preserves client-supplied keys and injects the configured fallback only when the request has no usable `Authorization` or `x-api-key` value, such as an empty `Bearer` or common placeholder key.
- Added a dedicated Settings page for MCP service definitions and Skill instructions. Enabled entries are injected as DeepSeek request context for Anthropic-compatible and OpenAI-compatible requests.
- Windows release builds now use the GUI subsystem, so double-clicking the app no longer opens a terminal first.

## Notes

- API key fallback is optional. Leave it empty if your IDE, plugin, or agent client already sends the DeepSeek key.
- Fallback keys are stored in the local app config directory when configured. Logs continue to redact sensitive headers and token-like values.
- MCP service entries in this version are request-context definitions. Actual MCP tool execution still depends on the client/runtime exposing those tools.

## Assets

- macOS Apple Silicon / ARM64 DMG: `DSP-deepseekPartner_1.1.0_macos-arm64.dmg`
- Windows x64 installer: `DSP-deepseekPartner_1.1.0_windows-x64-setup.exe`
- Windows x64 MSI: `DSP-deepseekPartner_1.1.0_windows-x64.msi`
