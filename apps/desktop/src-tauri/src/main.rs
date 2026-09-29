//! MCPanel desktop host. Wires adapters into the headless core, exposes the Application
//! API over Tauri IPC, and owns desktop concerns (window, tray, dialogs, logging).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod logging;
mod state;
mod tray;

use mcpanel_api::dto::EventDto;
use mcpanel_api::{Api, GrantKind};
use mcpanel_core::{Core, CoreDeps};
use state::AppState;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tauri::{DragDropEvent, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_notification::NotificationExt;

type Started = (
    Arc<Api>,
    mcpanel_db::Database,
    Vec<tracing_appender::non_blocking::WorkerGuard>,
);

/// Credential Manager namespace: the installed app uses `MCPanel`; a development
/// instance with its own data directory (MCPANEL_DATA_DIR) gets a separate one so it
/// never touches the real Backup Master Key.
fn secret_namespace() -> String {
    match std::env::var_os("MCPANEL_DATA_DIR") {
        None => "MCPanel".into(),
        Some(dir) => {
            // FNV-1a: stable across builds.
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in dir.to_string_lossy().to_lowercase().bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            format!("MCPanel-dev-{h:016x}")
        }
    }
}

async fn build_api() -> Result<Started, String> {
    let paths = mcpanel_platform::default_paths().map_err(|e| e.message)?;
    let guards = logging::init(&paths.logs_dir());
    tracing::info!(target: "mcpanel_desktop", version = env!("CARGO_PKG_VERSION"), "MCPanel starting");
    let (db, report) = mcpanel_db::Database::open(&paths.database_file())
        .await
        .map_err(|e| e.message)?;
    if let Some(b) = &report.backup {
        tracing::info!(target: "mcpanel::db", "pre-migration backup: {}", b.display());
    }
    let http = mcpanel_providers::http_client().map_err(|e| e.message)?;
    let core = Core::start(CoreDeps {
        paths,
        platform: Arc::new(mcpanel_platform::NativePlatform::new()),
        downloader: Arc::new(mcpanel_providers::HttpDownloader::new(http.clone())),
        registry: mcpanel_providers::builtin_registry(&http),
        profiles: Arc::new(mcpanel_providers::MojangProfiles::new(http.clone())),
        secrets: Arc::new(mcpanel_platform::NativeSecretStore::new(&secret_namespace())),
        repos: db.repositories(),
    })
    .await
    .map_err(|e| e.message)?;
    // Discover Java runtimes in the background on every launch.
    {
        let java = Arc::clone(&core.java);
        tokio::spawn(async move {
            if let Err(e) = java.detect().await {
                tracing::warn!(target: "mcpanel::java", "java detection failed: {}", e.message);
            }
        });
    }
    Ok((
        Arc::new(Api::new(core, env!("CARGO_PKG_VERSION"))),
        db,
        guards,
    ))
}

fn forward_events(app: tauri::AppHandle, api: Arc<Api>) {
    let mut rx = api.subscribe_events();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(env) => {
                    let dto = EventDto::from(&env);
                    let refresh_tray = matches!(
                        dto,
                        EventDto::ServerStateChanged { .. }
                            | EventDto::ServerCreated { .. }
                            | EventDto::ServerDeleted { .. }
                            | EventDto::ServerUpdated { .. }
                    );
                    // Desktop notifications follow the user's notification rules.
                    if let EventDto::NotificationCreated {
                        title,
                        body,
                        desktop: true,
                        ..
                    } = &dto
                    {
                        let mut n = app.notification().builder().title(title);
                        if !body.is_empty() {
                            n = n.body(body);
                        }
                        let _ = n.show();
                    }
                    let _ = app.emit("mcpanel://event", dto);
                    if refresh_tray {
                        tray::refresh(&app).await;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let _ = app.emit("mcpanel://resync", ());
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn main() {
    let mut builder = tauri::Builder::default();
    // One window per installation. A development instance with its own data directory
    // may run next to the installed app (it would otherwise just focus that app).
    if std::env::var_os("MCPANEL_DATA_DIR").is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            commands::show_main(app);
        }));
    }
    let app = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            match tauri::async_runtime::block_on(build_api()) {
                Ok((api, db, guards)) => {
                    app.manage(AppState::new(Arc::clone(&api), db));
                    app.manage(guards);
                    forward_events(handle.clone(), api);
                    tray::create(&handle)?;
                    let h = handle.clone();
                    tauri::async_runtime::spawn(async move { tray::refresh(&h).await });
                }
                Err(message) => {
                    handle
                        .dialog()
                        .message(format!("MCPanel could not start:\n\n{message}"))
                        .kind(MessageDialogKind::Error)
                        .title("MCPanel")
                        .blocking_show();
                    std::process::exit(1);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                let app = window.app_handle();
                let state = app.state::<AppState>();
                if state.quitting.load(Ordering::SeqCst) {
                    return;
                }
                // Close = minimise to tray (decision #2).
                api.prevent_close();
                let _ = window.hide();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let p = state.principal();
                    if let Ok(s) = state.api.settings_get(&p).await
                        && !s.tray_notice_shown
                    {
                        let _ = app
                            .notification()
                            .builder()
                            .title("MCPanel is still running")
                            .body("Your servers keep running. Use the tray icon to reopen or quit MCPanel.")
                            .show();
                        let _ = state
                            .api
                            .settings_update(
                                &p,
                                mcpanel_api::dto::SettingsPatchDto {
                                    tray_notice_shown: Some(true),
                                    ..Default::default()
                                },
                            )
                            .await;
                    }
                });
            }
            WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
                // OS drag-and-drop: convert paths into single-use grants for the UI.
                let app = window.app_handle();
                let state = app.state::<AppState>();
                let grants: Vec<_> = paths
                    .iter()
                    .map(|p| state.api.grants.issue(GrantKind::Source, p.clone()))
                    .collect();
                let _ = window.emit("mcpanel://files-dropped", grants);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::system_metrics,
            commands::settings_get,
            commands::settings_update,
            commands::open_logs_folder,
            commands::templates_list,
            commands::templates_resolve,
            commands::templates_apply,
            commands::restart_policy,
            commands::restart_policy_update,
            commands::crash_history,
            commands::tunnel_status,
            commands::tunnel_start_agent,
            commands::tunnel_stop_agent,
            commands::tunnel_link,
            commands::tunnel_cancel_link,
            commands::tunnel_server_address,
            commands::tunnel_set_server_address,
            commands::server_disk_usage,
            commands::diagnostics_export,
            commands::cloud_list,
            commands::cloud_connect,
            commands::cloud_flow,
            commands::cloud_cancel,
            commands::cloud_check,
            commands::cloud_disconnect,
            commands::encryption_status,
            commands::encryption_setup,
            commands::encryption_import,
            commands::encryption_set_enabled,
            commands::dialog_pick_file,
            commands::notifications_list,
            commands::notifications_unread,
            commands::notifications_mark_read,
            commands::notifications_clear,
            commands::notification_prefs,
            commands::notification_prefs_update,
            commands::bedrock_status,
            commands::bedrock_enable,
            commands::bedrock_configure,
            commands::bedrock_ping,
            commands::content_list,
            commands::content_search,
            commands::content_versions,
            commands::content_plan,
            commands::content_install,
            commands::content_remove,
            commands::content_set_enabled,
            commands::content_discard_pending,
            commands::content_check_updates,
            commands::players_get,
            commands::players_action,
            commands::backups_list,
            commands::backups_create,
            commands::backups_delete,
            commands::backups_verify,
            commands::backups_restore_preview,
            commands::backups_restore,
            commands::backups_policy,
            commands::backups_policy_update,
            commands::backups_location,
            commands::backups_set_location,
            commands::backups_reveal,
            commands::backups_open_folder,
            commands::app_quit,
            commands::dialog_pick_folder,
            commands::dialog_pick_java,
            commands::dialog_pick_import,
            commands::dialog_save_file,
            commands::java_list,
            commands::java_detect,
            commands::java_add,
            commands::java_revalidate,
            commands::java_remove,
            commands::software_list,
            commands::software_versions,
            commands::software_builds,
            commands::software_preview,
            commands::software_property_schema,
            commands::console_export,
            commands::open_external,
            commands::servers_list,
            commands::servers_get,
            commands::servers_check_location,
            commands::servers_create,
            commands::servers_detect_import,
            commands::servers_import,
            commands::servers_update,
            commands::servers_delete,
            commands::servers_accept_eula,
            commands::servers_start,
            commands::servers_stop,
            commands::servers_restart,
            commands::servers_command,
            commands::servers_metrics,
            commands::servers_running_count,
            commands::servers_properties,
            commands::servers_properties_update,
            commands::servers_open_folder,
            commands::console_history,
            commands::console_search,
            commands::console_subscribe,
            commands::console_unsubscribe,
            commands::files_list,
            commands::files_read,
            commands::files_write,
            commands::files_mkdir,
            commands::files_create,
            commands::files_rename,
            commands::files_move,
            commands::files_copy,
            commands::files_delete,
            commands::files_zip,
            commands::files_unzip,
            commands::files_import,
            commands::files_export,
            commands::files_search,
            commands::jobs_list,
            commands::jobs_get,
            commands::jobs_cancel,
            commands::audit_query,
        ])
        .build(tauri::generate_context!());

    let app = match app {
        Ok(a) => a,
        Err(e) => {
            eprintln!("failed to build MCPanel: {e}");
            std::process::exit(1);
        }
    };
    app.run(|handle, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event {
            // Exits not initiated by our quit flow (e.g. last window closed) are ignored:
            // MCPanel lives in the tray.
            let state = handle.state::<AppState>();
            if code.is_none() && !state.quitting.load(Ordering::SeqCst) {
                api.prevent_exit();
            }
        }
    });
}
