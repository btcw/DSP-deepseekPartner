use tauri::State;
use uuid::Uuid;

use crate::{
    gateway::{stopped_status, GatewayRegistry},
    models::{snippets_for, GatewayProfile, LogEntry, ProfileStatus, ProxySnippets},
    AppState,
};

#[tauri::command]
pub async fn list_profiles(state: State<'_, AppState>) -> Result<Vec<GatewayProfile>, String> {
    state.profiles.list().await.map_err(to_string)
}

#[tauri::command]
pub async fn save_profile(
    state: State<'_, AppState>,
    mut profile: GatewayProfile,
) -> Result<Vec<GatewayProfile>, String> {
    if profile.id.trim().is_empty() {
        profile.id = Uuid::new_v4().to_string();
    }
    profile.validate()?;

    let existing = state.profiles.find(&profile.id).await.map_err(to_string)?;
    let running = {
        let gateways = state.gateways.lock().await;
        gateways.get(&profile.id).cloned()
    };

    if let (Some(existing), Some(registry)) = (existing, running) {
        if existing.port != profile.port {
            return Err("Stop this gateway before changing its port".into());
        }
        registry.update_profile(profile.clone()).await;
    }

    state.profiles.save(profile).await.map_err(to_string)
}

#[tauri::command]
pub async fn delete_profile(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<GatewayProfile>, String> {
    if let Some(registry) = {
        let mut gateways = state.gateways.lock().await;
        gateways.remove(&id)
    } {
        registry.stop().await;
    }
    state.profiles.delete(&id).await.map_err(to_string)
}

#[tauri::command]
pub async fn start_profile(
    state: State<'_, AppState>,
    id: String,
) -> Result<ProfileStatus, String> {
    let profile = state
        .profiles
        .find(&id)
        .await
        .map_err(to_string)?
        .ok_or_else(|| "Profile not found".to_string())?;

    if let Some(existing) = {
        let gateways = state.gateways.lock().await;
        gateways.get(&id).cloned()
    } {
        return Ok(existing.status().await);
    }

    let registry = GatewayRegistry::start(profile.clone(), state.logs.clone())
        .await
        .map_err(to_string)?;
    let status = registry.status().await;
    state.gateways.lock().await.insert(id, registry);
    Ok(status)
}

#[tauri::command]
pub async fn stop_profile(state: State<'_, AppState>, id: String) -> Result<ProfileStatus, String> {
    let registry = {
        let mut gateways = state.gateways.lock().await;
        gateways.remove(&id)
    };
    if let Some(registry) = registry {
        registry.stop().await;
    }
    let profile = state
        .profiles
        .find(&id)
        .await
        .map_err(to_string)?
        .ok_or_else(|| "Profile not found".to_string())?;
    Ok(stopped_status(&profile).await)
}

#[tauri::command]
pub async fn start_all(state: State<'_, AppState>) -> Result<Vec<ProfileStatus>, String> {
    let profiles = state.profiles.list().await.map_err(to_string)?;
    let mut statuses = Vec::with_capacity(profiles.len());
    for profile in profiles {
        statuses.push(start_profile(state.clone(), profile.id).await?);
    }
    Ok(statuses)
}

#[tauri::command]
pub async fn stop_all(state: State<'_, AppState>) -> Result<Vec<ProfileStatus>, String> {
    let ids = {
        let gateways = state.gateways.lock().await;
        gateways.keys().cloned().collect::<Vec<_>>()
    };
    for id in ids {
        let _ = stop_profile(state.clone(), id).await?;
    }
    profile_statuses(state).await
}

#[tauri::command]
pub async fn profile_statuses(state: State<'_, AppState>) -> Result<Vec<ProfileStatus>, String> {
    let profiles = state.profiles.list().await.map_err(to_string)?;
    let gateways = state.gateways.lock().await.clone();
    let mut statuses = Vec::with_capacity(profiles.len());
    for profile in profiles {
        if let Some(registry) = gateways.get(&profile.id) {
            statuses.push(registry.status().await);
        } else {
            statuses.push(stopped_status(&profile).await);
        }
    }
    Ok(statuses)
}

#[tauri::command]
pub async fn read_logs(
    state: State<'_, AppState>,
    profile_id: String,
    limit: Option<usize>,
) -> Result<Vec<LogEntry>, String> {
    state
        .logs
        .read(&profile_id, limit.unwrap_or(400))
        .await
        .map_err(to_string)
}

#[tauri::command]
pub async fn clear_logs(state: State<'_, AppState>, profile_id: String) -> Result<(), String> {
    state.logs.clear(&profile_id).await.map_err(to_string)
}

#[tauri::command]
pub async fn copy_proxy_text(
    state: State<'_, AppState>,
    profile_id: String,
    kind: String,
) -> Result<String, String> {
    let profile = state
        .profiles
        .find(&profile_id)
        .await
        .map_err(to_string)?
        .ok_or_else(|| "Profile not found".to_string())?;
    let snippets: ProxySnippets = snippets_for(&profile);
    match kind.as_str() {
        "anthropicUrl" => Ok(snippets.anthropic_url),
        "openaiUrl" => Ok(snippets.openai_url),
        "claudeUnix" => Ok(snippets.claude_code_env_unix),
        "claudeWindows" => Ok(snippets.claude_code_env_windows),
        _ => Err("Unknown copy target".into()),
    }
}

fn to_string(error: impl std::fmt::Display) -> String {
    error.to_string()
}
