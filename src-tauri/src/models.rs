use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ApiSurface {
    Anthropic,
    OpenAi,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ServiceStatusKind {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Error,
    Info,
    Debug,
}

impl Default for LogLevel {
    fn default() -> Self {
        Self::Info
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureFlags {
    pub normalize_adaptive_thinking: bool,
    pub map_effort: bool,
    pub preserve_sse: bool,
    #[serde(default = "default_true")]
    pub smooth_streaming_text: bool,
    pub reasoning_replay: bool,
    pub one_m_context_defaults: bool,
    pub redact_sensitive_logs: bool,
}

fn default_true() -> bool {
    true
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            normalize_adaptive_thinking: true,
            map_effort: true,
            preserve_sse: true,
            smooth_streaming_text: true,
            reasoning_replay: true,
            one_m_context_defaults: true,
            redact_sensitive_logs: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMapping {
    pub main: String,
    pub opus: String,
    pub sonnet: String,
    pub haiku: String,
    pub subagent: String,
}

impl Default for ModelMapping {
    fn default() -> Self {
        Self {
            main: "deepseek-v4-pro[1m]".into(),
            opus: "deepseek-v4-pro[1m]".into(),
            sonnet: "deepseek-v4-pro[1m]".into(),
            haiku: "deepseek-v4-flash".into(),
            subagent: "deepseek-v4-flash".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayProfile {
    pub id: String,
    pub name: String,
    pub port: u16,
    pub upstream_base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    pub enabled_surfaces: Vec<ApiSurface>,
    pub model_mapping: ModelMapping,
    pub timeout_seconds: u64,
    pub log_level: LogLevel,
    pub features: FeatureFlags,
}

impl GatewayProfile {
    pub fn new_default(id: String, name: String, port: u16) -> Self {
        Self {
            id,
            name,
            port,
            upstream_base_url: "https://api.deepseek.com".into(),
            api_key: None,
            enabled_surfaces: vec![ApiSurface::Anthropic, ApiSurface::OpenAi],
            model_mapping: ModelMapping::default(),
            timeout_seconds: 120,
            log_level: LogLevel::default(),
            features: FeatureFlags::default(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let trimmed_name = self.name.trim();
        if trimmed_name.is_empty() {
            return Err("Profile name is required".into());
        }
        if self.port < 1024 {
            return Err("Port must be 1024 or higher".into());
        }
        if self.enabled_surfaces.is_empty() {
            return Err("At least one API surface must be enabled".into());
        }
        if !(self.upstream_base_url.starts_with("https://")
            || self.upstream_base_url.starts_with("http://"))
        {
            return Err("Upstream base URL must start with http:// or https://".into());
        }
        if self.timeout_seconds == 0 || self.timeout_seconds > 600 {
            return Err("Timeout must be between 1 and 600 seconds".into());
        }
        Ok(())
    }

    pub fn normalized(mut self) -> Self {
        self.name = self.name.trim().to_string();
        self.upstream_base_url = self
            .upstream_base_url
            .trim()
            .trim_end_matches('/')
            .to_string();
        self.api_key = self.api_key.and_then(|key| {
            let trimmed = key.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        self
    }

    pub fn fallback_api_key(&self) -> Option<&str> {
        self.api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
    }

    pub fn proxy_origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn anthropic_base_url(&self) -> String {
        format!("{}/anthropic", self.proxy_origin())
    }

    pub fn openai_base_url(&self) -> String {
        self.proxy_origin()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_mcp_config")]
    pub mcp_config: Value,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mcp_services: Vec<McpServiceConfig>,
    #[serde(default)]
    pub skills: Vec<SkillConfig>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            mcp_config: default_mcp_config(),
            mcp_services: Vec::new(),
            skills: Vec::new(),
        }
    }
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        let legacy_services = self
            .mcp_services
            .into_iter()
            .map(McpServiceConfig::normalized)
            .filter(|item| !item.name.is_empty() || !item.command.is_empty())
            .collect::<Vec<_>>();
        self.mcp_config = normalize_mcp_config(self.mcp_config, &legacy_services);
        self.mcp_services = Vec::new();
        self.skills = self
            .skills
            .into_iter()
            .map(SkillConfig::normalized)
            .filter(|item| !item.name.is_empty() || !item.instructions.is_empty())
            .collect();
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.mcp_config.is_object() {
            return Err("MCP config must be a JSON object".into());
        }
        for skill in &self.skills {
            if skill.enabled && skill.name.trim().is_empty() {
                return Err("Enabled skills must have a name".into());
            }
            if skill.enabled && skill.instructions.trim().is_empty() {
                return Err("Enabled skills must have instructions".into());
            }
        }
        Ok(())
    }

    pub fn network_request_mcp_enabled(&self) -> bool {
        mcp_server_enabled(&self.mcp_config, "network-request")
            || mcp_server_enabled(&self.mcp_config, "network_request")
            || self
                .mcp_config
                .pointer("/builtin/networkRequest/enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false)
    }

    pub fn request_context(&self) -> Option<String> {
        let services = mcp_server_summaries(&self.mcp_config);
        let skills = self
            .skills
            .iter()
            .filter(|item| item.enabled)
            .collect::<Vec<_>>();
        if services.is_empty() && skills.is_empty() {
            return None;
        }

        let mut lines = Vec::from([
            "DSP-deepseekPartner configured context.".to_string(),
            "Use these entries as user-configured guidance for DeepSeek-compatible clients."
                .to_string(),
        ]);
        if !skills.is_empty() {
            lines.push("Skills:".to_string());
            for skill in skills {
                let description = if skill.description.is_empty() {
                    String::new()
                } else {
                    format!(" - {}", skill.description)
                };
                lines.push(format!("- {}{description}", skill.name));
                lines.push(format!("  Instructions: {}", skill.instructions));
            }
        }
        if !services.is_empty() {
            lines.push("MCP services:".to_string());
            lines.push(
                "Enabled MCP definitions are available to DSP-deepseekPartner where supported."
                    .to_string(),
            );
            for service in &services {
                lines.push(format!("- {service}"));
            }
        }
        Some(lines.join("\n"))
    }
}

fn default_mcp_config() -> Value {
    json!({
        "mcpServers": {
            "network-request": {
                "type": "builtin",
                "enabled": true,
                "tool": "network_request",
                "description": "HTTP/HTTPS request helper executed by DSP-deepseekPartner"
            }
        }
    })
}

fn normalize_mcp_config(config: Value, legacy_services: &[McpServiceConfig]) -> Value {
    let mut config = if config.is_null() {
        default_mcp_config()
    } else {
        config
    };
    if !config.is_object() {
        return config;
    }
    if !legacy_services.is_empty() {
        let root = config.as_object_mut().expect("object checked above");
        let servers = root
            .entry("mcpServers")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(servers) = servers.as_object_mut() {
            for service in legacy_services {
                servers.insert(
                    service.name.clone(),
                    json!({
                        "command": service.command,
                        "args": split_legacy_args(&service.args),
                        "env": parse_legacy_env(&service.env),
                        "description": service.description,
                        "enabled": service.enabled
                    }),
                );
            }
        }
    }
    config
}

fn split_legacy_args(args: &str) -> Vec<String> {
    args.split_whitespace().map(str::to_string).collect()
}

fn parse_legacy_env(env: &str) -> Map<String, Value> {
    env.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| {
            (
                key.trim().to_string(),
                Value::String(value.trim().to_string()),
            )
        })
        .collect()
}

fn mcp_server_enabled(config: &Value, name: &str) -> bool {
    config
        .pointer(&format!("/mcpServers/{name}/enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn mcp_server_summaries(config: &Value) -> Vec<String> {
    let Some(servers) = config.get("mcpServers").and_then(Value::as_object) else {
        return Vec::new();
    };
    servers
        .iter()
        .filter(|(_, value)| {
            value
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true)
        })
        .map(|(name, value)| {
            let description = value
                .get("description")
                .and_then(Value::as_str)
                .filter(|item| !item.trim().is_empty())
                .map(|item| format!(" - {}", item.trim()))
                .unwrap_or_default();
            let command = value
                .get("command")
                .and_then(Value::as_str)
                .filter(|item| !item.trim().is_empty())
                .or_else(|| value.get("type").and_then(Value::as_str))
                .unwrap_or("configured");
            format!("{name}: {command}{description}")
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServiceConfig {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub env: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl McpServiceConfig {
    fn normalized(mut self) -> Self {
        self.id = self.id.trim().to_string();
        self.name = self.name.trim().to_string();
        self.command = self.command.trim().to_string();
        self.args = self.args.trim().to_string();
        self.env = self.env.trim().to_string();
        self.description = self.description.trim().to_string();
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub instructions: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl SkillConfig {
    fn normalized(mut self) -> Self {
        self.id = self.id.trim().to_string();
        self.name = self.name.trim().to_string();
        self.description = self.description.trim().to_string();
        self.instructions = self.instructions.trim().to_string();
        self
    }
}

pub fn canonical_model_id(model: &str) -> String {
    match model {
        "deepseek-v4-pro[1m]" => "deepseek-v4-pro".to_string(),
        "deepseek-v4-flash[1m]" => "deepseek-v4-flash".to_string(),
        other => other.to_string(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileStatus {
    pub id: String,
    pub status: ServiceStatusKind,
    pub port: u16,
    pub proxy_origin: String,
    pub last_error: Option<String>,
    pub started_at: Option<String>,
    pub request_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub profile_id: String,
    pub request_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySnippets {
    pub anthropic_url: String,
    pub openai_url: String,
    pub claude_code_env_unix: String,
    pub claude_code_env_windows: String,
}

pub fn snippets_for(profile: &GatewayProfile) -> ProxySnippets {
    let anthropic_url = profile.anthropic_base_url();
    let openai_url = profile.openai_base_url();
    let m = &profile.model_mapping;
    ProxySnippets {
        anthropic_url: anthropic_url.clone(),
        openai_url,
        claude_code_env_unix: format!(
            "export ANTHROPIC_BASE_URL={anthropic_url}\nexport ANTHROPIC_AUTH_TOKEN=<your DeepSeek API Key>\nexport ANTHROPIC_MODEL={main}\nexport ANTHROPIC_DEFAULT_OPUS_MODEL={opus}\nexport ANTHROPIC_DEFAULT_SONNET_MODEL={sonnet}\nexport ANTHROPIC_DEFAULT_HAIKU_MODEL={haiku}\nexport CLAUDE_CODE_SUBAGENT_MODEL={subagent}\nexport CLAUDE_CODE_EFFORT_LEVEL=max",
            main = m.main,
            opus = m.opus,
            sonnet = m.sonnet,
            haiku = m.haiku,
            subagent = m.subagent
        ),
        claude_code_env_windows: format!(
            "$env:ANTHROPIC_BASE_URL=\"{anthropic_url}\"\n$env:ANTHROPIC_AUTH_TOKEN=\"<your DeepSeek API Key>\"\n$env:ANTHROPIC_MODEL=\"{main}\"\n$env:ANTHROPIC_DEFAULT_OPUS_MODEL=\"{opus}\"\n$env:ANTHROPIC_DEFAULT_SONNET_MODEL=\"{sonnet}\"\n$env:ANTHROPIC_DEFAULT_HAIKU_MODEL=\"{haiku}\"\n$env:CLAUDE_CODE_SUBAGENT_MODEL=\"{subagent}\"\n$env:CLAUDE_CODE_EFFORT_LEVEL=\"max\"",
            main = m.main,
            opus = m.opus,
            sonnet = m.sonnet,
            haiku = m.haiku,
            subagent = m.subagent
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_snippets_do_not_embed_real_api_keys() {
        let profile = GatewayProfile::new_default("p1".into(), "Local".into(), 17777);
        let snippets = snippets_for(&profile);
        assert!(snippets
            .claude_code_env_unix
            .contains("<your DeepSeek API Key>"));
        assert!(!snippets.claude_code_env_unix.contains("sk-"));
        assert!(snippets
            .claude_code_env_unix
            .contains("deepseek-v4-pro[1m]"));
    }
}
