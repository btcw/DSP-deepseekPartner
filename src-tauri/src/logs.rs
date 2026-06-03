use std::{path::PathBuf, sync::Arc};

use chrono::Utc;
use tokio::{fs, io::AsyncWriteExt, sync::RwLock};

use crate::models::LogEntry;

#[derive(Clone)]
pub struct LogStore {
    dir: PathBuf,
    lock: Arc<RwLock<()>>,
}

impl LogStore {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            lock: Arc::new(RwLock::new(())),
        }
    }

    pub async fn append(
        &self,
        profile_id: &str,
        level: &str,
        request_id: Option<&str>,
        message: impl AsRef<str>,
    ) -> anyhow::Result<()> {
        let entry = LogEntry {
            timestamp: Utc::now().to_rfc3339(),
            level: level.to_string(),
            profile_id: profile_id.to_string(),
            request_id: request_id.map(str::to_string),
            message: redact(message.as_ref()),
        };
        let _guard = self.lock.write().await;
        fs::create_dir_all(&self.dir).await?;
        let path = self.path_for(profile_id);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        let raw = serde_json::to_string(&entry)?;
        file.write_all(raw.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }

    pub async fn read(&self, profile_id: &str, limit: usize) -> anyhow::Result<Vec<LogEntry>> {
        let _guard = self.lock.read().await;
        let path = self.path_for(profile_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(path).await?;
        let mut entries: Vec<LogEntry> = raw
            .lines()
            .filter_map(|line| serde_json::from_str::<LogEntry>(line).ok())
            .collect();
        if entries.len() > limit {
            entries = entries.split_off(entries.len() - limit);
        }
        Ok(entries)
    }

    pub async fn clear(&self, profile_id: &str) -> anyhow::Result<()> {
        let _guard = self.lock.write().await;
        let path = self.path_for(profile_id);
        if path.exists() {
            fs::write(path, "").await?;
        }
        Ok(())
    }

    fn path_for(&self, profile_id: &str) -> PathBuf {
        let safe_id = profile_id
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>();
        self.dir.join(format!("{safe_id}.jsonl"))
    }
}

pub fn redact(input: &str) -> String {
    input
        .split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if lower.starts_with("authorization:") && part.len() > "authorization:".len() {
                "Authorization:<redacted>".to_string()
            } else if lower.starts_with("x-api-key:") && part.len() > "x-api-key:".len() {
                "x-api-key:<redacted>".to_string()
            } else if lower.starts_with("x-api-key=") {
                "x-api-key=<redacted>".to_string()
            } else if lower.starts_with("api_key=") {
                "api_key=<redacted>".to_string()
            } else if part
                .trim_matches(|ch: char| ch == ',' || ch == '"' || ch == '\'')
                .starts_with("sk-")
                && part.len() > 12
            {
                "<redacted-token>".to_string()
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sensitive_values() {
        let raw = "Authorization: Bearer sk-123456789abcdef, x-api-key: sk-secret-token";
        let redacted = redact(raw);
        assert!(!redacted.contains("sk-123456"));
        assert!(!redacted.contains("sk-secret"));
        assert!(redacted.contains("<redacted>") || redacted.contains("<redacted-token>"));
    }
}
