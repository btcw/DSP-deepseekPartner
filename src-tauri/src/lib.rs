mod commands;
mod gateway;
mod logs;
mod models;
mod storage;

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use gateway::GatewayRegistry;
use logs::LogStore;
use storage::ProfileStore;
use tauri::{Manager, WindowEvent};
use tokio::sync::Mutex;

pub use models::*;

#[derive(Clone)]
pub struct AppState {
    profiles: ProfileStore,
    logs: LogStore,
    gateways: Arc<Mutex<HashMap<String, GatewayRegistry>>>,
}

impl AppState {
    fn new(app: &tauri::App) -> anyhow::Result<Self> {
        let resolver = app.path();
        let config_dir = resolver.app_config_dir()?;
        let log_dir = resolver.app_log_dir()?;
        Ok(Self {
            profiles: ProfileStore::new(config_dir),
            logs: LogStore::new(log_dir),
            gateways: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let state = AppState::new(app)?;
            if std::env::var("DEEPSEEK_GATEWAY_AUTOSTART").as_deref() == Ok("1") {
                let autostart_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    match autostart_state.profiles.list().await {
                        Ok(profiles) => {
                            for profile in profiles {
                                let profile_id = profile.id.clone();
                                match GatewayRegistry::start(
                                    profile.clone(),
                                    autostart_state.logs.clone(),
                                )
                                .await
                                {
                                    Ok(registry) => {
                                        autostart_state
                                            .gateways
                                            .lock()
                                            .await
                                            .insert(profile_id, registry);
                                    }
                                    Err(error) => {
                                        let _ = autostart_state
                                            .logs
                                            .append(
                                                &profile_id,
                                                "error",
                                                None,
                                                format!("Autostart failed: {error}"),
                                            )
                                            .await;
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            eprintln!("Failed to load profiles for autostart: {error}");
                        }
                    }
                });
            }
            app.manage(state);

            #[cfg(desktop)]
            {
                use tauri::tray::TrayIconBuilder;
                let _tray = TrayIconBuilder::with_id("main")
                    .tooltip("DSP-deepseekPartner")
                    .build(app)?;
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_profiles,
            commands::save_profile,
            commands::delete_profile,
            commands::start_profile,
            commands::stop_profile,
            commands::start_all,
            commands::stop_all,
            commands::profile_statuses,
            commands::read_logs,
            commands::clear_logs,
            commands::copy_proxy_text
        ])
        .run(tauri::generate_context!())
        .expect("failed to run DSP-deepseekPartner");
}

pub fn run_headless() {
    let runtime = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    runtime.block_on(async {
        let (config_dir, log_dir) =
            default_app_dirs().expect("failed to resolve DSP-deepseekPartner app dirs");
        let profiles = ProfileStore::new(config_dir);
        let logs = LogStore::new(log_dir);
        let gateways = Arc::new(Mutex::new(HashMap::<String, GatewayRegistry>::new()));
        let loaded_profiles = profiles
            .list()
            .await
            .expect("failed to load DSP-deepseekPartner profiles");

        for profile in loaded_profiles {
            let profile_id = profile.id.clone();
            match GatewayRegistry::start(profile.clone(), logs.clone()).await {
                Ok(registry) => {
                    gateways.lock().await.insert(profile_id, registry);
                }
                Err(error) => {
                    let _ = logs
                        .append(
                            &profile_id,
                            "error",
                            None,
                            format!("Headless start failed: {error}"),
                        )
                        .await;
                }
            }
        }

        std::future::pending::<()>().await;
        let registries = {
            let mut guard = gateways.lock().await;
            guard
                .drain()
                .map(|(_, registry)| registry)
                .collect::<Vec<_>>()
        };
        for registry in registries {
            registry.stop().await;
        }
    });
}

fn default_app_dirs() -> anyhow::Result<(PathBuf, PathBuf)> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME")?;
        return Ok((
            PathBuf::from(&home)
                .join("Library")
                .join("Application Support")
                .join("com.deepseekpartner.gateway"),
            PathBuf::from(home)
                .join("Library")
                .join("Logs")
                .join("com.deepseekpartner.gateway"),
        ));
    }

    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA")?;
        let localappdata = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| appdata.clone());
        return Ok((
            PathBuf::from(appdata).join("com.deepseekpartner.gateway"),
            PathBuf::from(localappdata)
                .join("com.deepseekpartner.gateway")
                .join("logs"),
        ));
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let home = std::env::var("HOME")?;
        Ok((
            PathBuf::from(&home)
                .join(".config")
                .join("com.deepseekpartner.gateway"),
            PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("com.deepseekpartner.gateway")
                .join("logs"),
        ))
    }
}
