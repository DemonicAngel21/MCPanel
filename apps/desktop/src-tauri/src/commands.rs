//! Tauri IPC adapter: thin wrappers that forward to the Application API. No business
//! logic lives here.

use crate::state::AppState;
use mcpanel_api::dto::*;
use mcpanel_api::{ApiError, GrantDto, GrantKind};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio_util::sync::CancellationToken;

type R<T> = Result<T, ApiError>;

// ───────────────────────────── system ─────────────────────────────

#[tauri::command]
pub fn app_info(s: State<'_, AppState>) -> R<AppInfoDto> {
    s.api.app_info(&s.principal())
}

#[tauri::command]
pub fn system_metrics(s: State<'_, AppState>) -> R<SystemMetricsDto> {
    s.api.system_metrics(&s.principal())
}

#[tauri::command]
pub async fn settings_get(s: State<'_, AppState>) -> R<SettingsDto> {
    s.api.settings_get(&s.principal()).await
}

#[tauri::command]
pub async fn settings_update(s: State<'_, AppState>, patch: SettingsPatchDto) -> R<SettingsDto> {
    s.api.settings_update(&s.principal(), patch).await
}

/// Open the MCPanel logs folder in Explorer (the path is MCPanel-owned, not user input).
#[tauri::command]
pub fn open_logs_folder(s: State<'_, AppState>) -> R<()> {
    let dir = s.api.core().paths.logs_dir();
    std::process::Command::new("explorer.exe")
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| ApiError::new("IO", format!("Cannot open folder: {e}")))
}

/// Quit MCPanel. `mode`: "stop" = stop servers gracefully first; "leave" = leave them
/// running (they will be detected as detached on next launch).
#[tauri::command]
pub async fn app_quit(app: AppHandle, s: State<'_, AppState>, mode: String) -> R<()> {
    match mode.as_str() {
        "stop" => s.api.servers_stop_all(&s.principal()).await?,
        "leave" => {}
        _ => return Err(ApiError::invalid("Unknown quit mode")),
    }
    s.quitting.store(true, std::sync::atomic::Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

// ───────────────────────────── dialogs ────────────────────────────

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::new("INTERNAL", e.to_string()))
}

#[tauri::command]
pub async fn dialog_pick_folder(
    app: AppHandle,
    s: State<'_, AppState>,
    title: String,
) -> R<Option<GrantDto>> {
    let a = app.clone();
    let picked =
        blocking(move || a.dialog().file().set_title(title).blocking_pick_folder()).await?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| s.api.grants.issue(GrantKind::Directory, p)))
}

#[tauri::command]
pub async fn dialog_pick_java(app: AppHandle, s: State<'_, AppState>) -> R<Option<GrantDto>> {
    let a = app.clone();
    let picked = blocking(move || {
        a.dialog()
            .file()
            .set_title("Select java.exe")
            .add_filter("Java runtime", &["exe"])
            .blocking_pick_file()
    })
    .await?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| s.api.grants.issue(GrantKind::Source, p)))
}

#[tauri::command]
pub async fn dialog_pick_import(
    app: AppHandle,
    s: State<'_, AppState>,
    folder: bool,
) -> R<Vec<GrantDto>> {
    let a = app.clone();
    let paths = blocking(move || {
        let d = a.dialog().file().set_title("Upload to server");
        if folder {
            d.blocking_pick_folder().into_iter().collect::<Vec<_>>()
        } else {
            d.blocking_pick_files().unwrap_or_default()
        }
    })
    .await?;
    Ok(paths
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .map(|p| s.api.grants.issue(GrantKind::Source, p))
        .collect())
}

#[tauri::command]
pub async fn dialog_save_file(
    app: AppHandle,
    s: State<'_, AppState>,
    default_name: String,
) -> R<Option<GrantDto>> {
    let a = app.clone();
    let picked = blocking(move || {
        a.dialog()
            .file()
            .set_file_name(default_name)
            .blocking_save_file()
    })
    .await?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| s.api.grants.issue(GrantKind::SaveTarget, p)))
}

// ────────────────────────────── java ──────────────────────────────

#[tauri::command]
pub async fn java_list(s: State<'_, AppState>) -> R<Vec<JavaRuntimeDto>> {
    s.api.java_list(&s.principal()).await
}

#[tauri::command]
pub async fn java_detect(s: State<'_, AppState>) -> R<Vec<JavaRuntimeDto>> {
    s.api.java_detect(&s.principal()).await
}

#[tauri::command]
pub async fn java_add(s: State<'_, AppState>, grant: String) -> R<JavaRuntimeDto> {
    s.api.java_add(&s.principal(), &grant).await
}

#[tauri::command]
pub async fn java_revalidate(s: State<'_, AppState>, id: String) -> R<JavaRuntimeDto> {
    s.api.java_revalidate(&s.principal(), &id).await
}

#[tauri::command]
pub async fn java_remove(s: State<'_, AppState>, id: String) -> R<()> {
    s.api.java_remove(&s.principal(), &id).await
}

// ──────────────────────────── software ────────────────────────────

#[tauri::command]
pub fn software_list(s: State<'_, AppState>) -> R<Vec<SoftwareDto>> {
    s.api.software_list(&s.principal())
}

#[tauri::command]
pub async fn software_versions(
    s: State<'_, AppState>,
    software_id: String,
    include_snapshots: bool,
) -> R<Vec<GameVersionDto>> {
    s.api
        .software_versions(&s.principal(), &software_id, include_snapshots)
        .await
}

#[tauri::command]
pub async fn software_builds(
    s: State<'_, AppState>,
    software_id: String,
    game_version: String,
) -> R<Vec<SoftwareBuildDto>> {
    s.api
        .software_builds(&s.principal(), &software_id, &game_version)
        .await
}

#[tauri::command]
pub async fn software_preview(
    s: State<'_, AppState>,
    software_id: String,
    game_version: String,
    build: Option<String>,
) -> R<InstallPreviewDto> {
    s.api
        .software_preview(&s.principal(), &software_id, &game_version, build)
        .await
}

// ───────────────────────────── servers ────────────────────────────

#[tauri::command]
pub async fn servers_list(s: State<'_, AppState>) -> R<Vec<ServerDto>> {
    s.api.servers_list(&s.principal()).await
}

#[tauri::command]
pub async fn servers_get(s: State<'_, AppState>, id: String) -> R<ServerDto> {
    s.api.servers_get(&s.principal(), &id).await
}

#[tauri::command]
pub async fn servers_check_location(
    s: State<'_, AppState>,
    name: String,
    parent_grant: Option<String>,
) -> R<LocationCheckDto> {
    s.api
        .servers_check_location(&s.principal(), &name, parent_grant.as_deref())
        .await
}

#[tauri::command]
pub async fn servers_create(s: State<'_, AppState>, request: CreateServerDto) -> R<String> {
    s.api.servers_create(&s.principal(), request).await
}

#[tauri::command]
pub async fn servers_detect_import(s: State<'_, AppState>, grant: String) -> R<ImportDetectionDto> {
    s.api.servers_detect_import(&s.principal(), &grant).await
}

#[tauri::command]
pub async fn servers_import(s: State<'_, AppState>, request: ImportServerDto) -> R<ServerDto> {
    s.api.servers_import(&s.principal(), request).await
}

#[tauri::command]
pub async fn servers_update(
    s: State<'_, AppState>,
    id: String,
    request: UpdateServerDto,
) -> R<ServerDto> {
    s.api.servers_update(&s.principal(), &id, request).await
}

#[tauri::command]
pub async fn servers_delete(s: State<'_, AppState>, id: String, delete_files: bool) -> R<()> {
    s.api
        .servers_delete(&s.principal(), &id, delete_files)
        .await
}

#[tauri::command]
pub async fn servers_accept_eula(s: State<'_, AppState>, id: String) -> R<()> {
    s.api.servers_accept_eula(&s.principal(), &id).await
}

#[tauri::command]
pub async fn servers_start(s: State<'_, AppState>, id: String) -> R<()> {
    s.api.servers_start(&s.principal(), &id).await
}

#[tauri::command]
pub async fn servers_stop(s: State<'_, AppState>, id: String, force: bool) -> R<()> {
    s.api.servers_stop(&s.principal(), &id, force).await
}

#[tauri::command]
pub async fn servers_restart(s: State<'_, AppState>, id: String) -> R<()> {
    s.api.servers_restart(&s.principal(), &id).await
}

#[tauri::command]
pub async fn servers_command(s: State<'_, AppState>, id: String, command: String) -> R<()> {
    s.api.servers_command(&s.principal(), &id, &command).await
}

#[tauri::command]
pub async fn servers_metrics(s: State<'_, AppState>, id: String) -> R<ServerMetricsDto> {
    s.api.servers_metrics(&s.principal(), &id).await
}

#[tauri::command]
pub fn servers_running_count(s: State<'_, AppState>) -> R<u32> {
    s.api.servers_running_count(&s.principal())
}

#[tauri::command]
pub async fn servers_properties(s: State<'_, AppState>, id: String) -> R<ServerPropertiesDto> {
    s.api.servers_properties(&s.principal(), &id).await
}

#[tauri::command]
pub async fn servers_properties_update(
    s: State<'_, AppState>,
    id: String,
    changes: Vec<PropertyChangeDto>,
) -> R<ServerPropertiesDto> {
    s.api
        .servers_properties_update(&s.principal(), &id, changes)
        .await
}

/// Open the server's folder in Explorer. The path comes from the server record.
#[tauri::command]
pub async fn servers_open_folder(s: State<'_, AppState>, id: String) -> R<()> {
    let server = s.api.servers_get(&s.principal(), &id).await?;
    std::process::Command::new("explorer.exe")
        .arg(&server.directory)
        .spawn()
        .map(|_| ())
        .map_err(|e| ApiError::new("IO", format!("Cannot open folder: {e}")))
}

// ───────────────────────────── console ────────────────────────────

#[tauri::command]
pub async fn software_property_schema(
    s: State<'_, AppState>,
    game_version: String,
) -> R<Vec<PropertyDto>> {
    s.api
        .software_property_schema(&s.principal(), &game_version)
        .await
}

#[tauri::command]
pub async fn console_export(s: State<'_, AppState>, id: String, grant: String) -> R<u64> {
    s.api.console_export(&s.principal(), &id, &grant).await
}

/// Open one of a fixed set of external links in the default browser.
#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> R<()> {
    const ALLOWED: &[&str] = &[
        "https://aka.ms/MinecraftEULA",
        "https://www.minecraft.net/eula",
        "https://github.com/mcpanel/mcpanel",
    ];
    if !ALLOWED.contains(&url.as_str()) {
        return Err(ApiError::invalid("This link cannot be opened"));
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| ApiError::new("IO", format!("Cannot open link: {e}")))
}

#[tauri::command]
pub fn console_history(
    s: State<'_, AppState>,
    id: String,
    from_seq: Option<u64>,
    limit: u32,
) -> R<Vec<ConsoleLineDto>> {
    s.api.console_history(&s.principal(), &id, from_seq, limit)
}

#[tauri::command]
pub fn console_search(
    s: State<'_, AppState>,
    id: String,
    query: String,
    limit: u32,
) -> R<Vec<ConsoleLineDto>> {
    s.api.console_search(&s.principal(), &id, &query, limit)
}

/// Stream console batches over a Channel. Returns a subscription id for unsubscribing.
#[tauri::command]
pub fn console_subscribe(
    s: State<'_, AppState>,
    id: String,
    after_seq: Option<u64>,
    backlog: u32,
    on_batch: Channel<ConsoleBatchDto>,
) -> R<String> {
    let mut sub = s
        .api
        .console_subscribe(&s.principal(), &id, after_seq, backlog)?;
    let sub_id = uuid::Uuid::new_v4().simple().to_string();
    let cancel = CancellationToken::new();
    s.subscriptions
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(sub_id.clone(), cancel.clone());
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                batch = sub.next_batch(500, Duration::from_millis(50)) => match batch {
                    Some(b) => {
                        if on_batch.send(b.into()).is_err() {
                            break;
                        }
                    }
                    None => break,
                },
            }
        }
    });
    Ok(sub_id)
}

#[tauri::command]
pub fn console_unsubscribe(s: State<'_, AppState>, subscription: String) -> R<()> {
    if let Some(t) = s
        .subscriptions
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&subscription)
    {
        t.cancel();
    }
    Ok(())
}

// ────────────────────────────── files ─────────────────────────────

#[tauri::command]
pub async fn files_list(s: State<'_, AppState>, id: String, path: String) -> R<Vec<FileEntryDto>> {
    s.api.files_list(&s.principal(), &id, &path).await
}

#[tauri::command]
pub async fn files_read(s: State<'_, AppState>, id: String, path: String) -> R<TextDocumentDto> {
    s.api.files_read(&s.principal(), &id, &path).await
}

#[tauri::command]
pub async fn files_write(
    s: State<'_, AppState>,
    id: String,
    path: String,
    document: WriteTextDto,
) -> R<TextDocumentDto> {
    s.api
        .files_write(&s.principal(), &id, &path, document)
        .await
}

#[tauri::command]
pub async fn files_mkdir(s: State<'_, AppState>, id: String, path: String) -> R<FileEntryDto> {
    s.api.files_mkdir(&s.principal(), &id, &path).await
}

#[tauri::command]
pub async fn files_create(s: State<'_, AppState>, id: String, path: String) -> R<FileEntryDto> {
    s.api.files_create(&s.principal(), &id, &path).await
}

#[tauri::command]
pub async fn files_rename(
    s: State<'_, AppState>,
    id: String,
    path: String,
    new_name: String,
) -> R<FileEntryDto> {
    s.api
        .files_rename(&s.principal(), &id, &path, &new_name)
        .await
}

#[tauri::command]
pub async fn files_move(
    s: State<'_, AppState>,
    id: String,
    paths: Vec<String>,
    destination: String,
) -> R<()> {
    s.api
        .files_move(&s.principal(), &id, paths, &destination)
        .await
}

#[tauri::command]
pub async fn files_copy(
    s: State<'_, AppState>,
    id: String,
    paths: Vec<String>,
    destination: String,
) -> R<FileOpResultDto> {
    s.api
        .files_copy(&s.principal(), &id, paths, &destination)
        .await
}

#[tauri::command]
pub async fn files_delete(
    s: State<'_, AppState>,
    id: String,
    paths: Vec<String>,
    permanent: bool,
) -> R<()> {
    s.api
        .files_delete(&s.principal(), &id, paths, permanent)
        .await
}

#[tauri::command]
pub async fn files_zip(
    s: State<'_, AppState>,
    id: String,
    paths: Vec<String>,
    archive_name: String,
) -> R<FileOpResultDto> {
    s.api
        .files_zip(&s.principal(), &id, paths, &archive_name)
        .await
}

#[tauri::command]
pub async fn files_unzip(
    s: State<'_, AppState>,
    id: String,
    archive: String,
    destination: String,
    overwrite: bool,
) -> R<FileOpResultDto> {
    s.api
        .files_unzip(&s.principal(), &id, &archive, &destination, overwrite)
        .await
}

#[tauri::command]
pub async fn files_import(
    s: State<'_, AppState>,
    id: String,
    grants: Vec<String>,
    destination: String,
) -> R<Vec<FileEntryDto>> {
    s.api
        .files_import(&s.principal(), &id, grants, &destination)
        .await
}

#[tauri::command]
pub async fn files_export(
    s: State<'_, AppState>,
    id: String,
    path: String,
    grant: String,
) -> R<u64> {
    s.api.files_export(&s.principal(), &id, &path, &grant).await
}

#[tauri::command]
pub async fn files_search(
    s: State<'_, AppState>,
    id: String,
    path: String,
    query: String,
    limit: u32,
) -> R<Vec<FileEntryDto>> {
    s.api
        .files_search(&s.principal(), &id, &path, &query, limit)
        .await
}

// ───────────────────────────── jobs / audit ───────────────────────

#[tauri::command]
pub async fn jobs_list(s: State<'_, AppState>, limit: u32) -> R<Vec<JobDto>> {
    s.api.jobs_list(&s.principal(), limit).await
}

#[tauri::command]
pub async fn jobs_get(s: State<'_, AppState>, id: String) -> R<JobDto> {
    s.api.jobs_get(&s.principal(), &id).await
}

#[tauri::command]
pub fn jobs_cancel(s: State<'_, AppState>, id: String) -> R<bool> {
    s.api.jobs_cancel(&s.principal(), &id)
}

#[tauri::command]
pub async fn audit_query(
    s: State<'_, AppState>,
    server_id: Option<String>,
    before: Option<i64>,
    limit: u32,
) -> R<Vec<AuditEntryDto>> {
    s.api
        .audit_query(&s.principal(), server_id.as_deref(), before, limit)
        .await
}

/// Show and focus the main window (used by tray and single-instance handler).
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
