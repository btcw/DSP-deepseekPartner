use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use tokio::{fs, sync::RwLock};

use crate::models::GatewayProfile;

#[derive(Clone)]
pub struct ProfileStore {
    path: PathBuf,
    lock: Arc<RwLock<()>>,
}

impl ProfileStore {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            path: config_dir.join("profiles.json"),
            lock: Arc::new(RwLock::new(())),
        }
    }

    pub async fn list(&self) -> anyhow::Result<Vec<GatewayProfile>> {
        let _guard = self.lock.read().await;
        self.read_without_lock().await
    }

    pub async fn save(&self, profile: GatewayProfile) -> anyhow::Result<Vec<GatewayProfile>> {
        profile.validate().map_err(anyhow::Error::msg)?;
        let _guard = self.lock.write().await;
        let mut profiles = self.read_without_lock().await?;
        if let Some(existing) = profiles.iter_mut().find(|item| item.id == profile.id) {
            *existing = profile;
        } else {
            profiles.push(profile);
        }
        profiles.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.write_without_lock(&profiles).await?;
        Ok(profiles)
    }

    pub async fn delete(&self, id: &str) -> anyhow::Result<Vec<GatewayProfile>> {
        let _guard = self.lock.write().await;
        let mut profiles = self.read_without_lock().await?;
        profiles.retain(|item| item.id != id);
        self.write_without_lock(&profiles).await?;
        Ok(profiles)
    }

    pub async fn find(&self, id: &str) -> anyhow::Result<Option<GatewayProfile>> {
        Ok(self.list().await?.into_iter().find(|item| item.id == id))
    }

    async fn read_without_lock(&self) -> anyhow::Result<Vec<GatewayProfile>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&self.path)
            .await
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", self.path.display()))
    }

    async fn write_without_lock(&self, profiles: &[GatewayProfile]) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let raw = serde_json::to_string_pretty(profiles)?;
        fs::write(&tmp, raw).await?;
        fs::rename(&tmp, &self.path).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::models::GatewayProfile;

    #[tokio::test]
    async fn saves_and_lists_profiles() {
        let dir = tempdir().unwrap();
        let store = ProfileStore::new(dir.path().into());
        store
            .save(GatewayProfile::new_default(
                "one".into(),
                "First".into(),
                17777,
            ))
            .await
            .unwrap();
        let profiles = store.list().await.unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "First");
    }
}
