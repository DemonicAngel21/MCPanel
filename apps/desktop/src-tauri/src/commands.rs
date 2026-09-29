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
    s.db.close().await;
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

// ──────────────────────────── templates ───────────────────────────

#[tauri::command]
pub fn templates_list(s: State<'_, AppState>) -> R<Vec<TemplateDto>> {
    s.api.templates_list(&s.principal())
}

#[tauri::command]
pub async fn templates_resolve(
    s: State<'_, AppState>,
    id: String,
    software_id: String,
    game_version: String,
) -> R<ResolvedTemplateDto> {
    s.api
        .templates_resolve(&s.principal(), &id, &software_id, &game_version)
        .await
}

#[tauri::command]
pub async fn templates_apply(
    s: State<'_, AppState>,
    server_id: String,
    id: String,
    plugins: Vec<String>,
) -> R<Vec<String>> {
    s.api
        .templates_apply(&s.principal(), &server_id, &id, plugins)
        .await
}

// ───────────────────────────── crashes ────────────────────────────

#[tauri::command]
pub async fn restart_policy(s: State<'_, AppState>, server_id: String) -> R<RestartPolicyDto> {
    s.api.restart_policy(&s.principal(), &server_id).await
}

#[tauri::command]
pub async fn restart_policy_update(
    s: State<'_, AppState>,
    server_id: String,
    policy: RestartPolicyDto,
) -> R<RestartPolicyDto> {
    s.api
        .restart_policy_update(&s.principal(), &server_id, policy)
        .await
}

#[tauri::command]
pub async fn crash_history(
    s: State<'_, AppState>,
    server_id: String,
    limit: u32,
) -> R<Vec<CrashEventDto>> {
    s.api.crash_history(&s.principal(), &server_id, limit).await
}

// ──────────────────────────────── disk ────────────────────────────

#[tauri::command]
pub async fn server_disk_usage(s: State<'_, AppState>, server_id: String) -> R<DiskUsageDto> {
    s.api.server_disk_usage(&s.principal(), &server_id).await
}

// ───────────────────────────── diagnostics ────────────────────────

#[tauri::command]
pub async fn diagnostics_export(
    s: State<'_, AppState>,
    server_id: String,
    grant: String,
) -> R<u32> {
    s.api
        .diagnostics_export(&s.principal(), &server_id, &grant)
        .await
}

// ─────────────────────────────── cloud ────────────────────────────

#[tauri::command]
pub async fn cloud_list(s: State<'_, AppState>) -> R<Vec<CloudStatusDto>> {
    s.api.cloud_list(&s.principal()).await
}

/// Sign-in hosts the browser may be sent to (the providers' authorization endpoints).
const CLOUD_AUTH_HOSTS: &[&str] = &[
    "accounts.google.com",
    "login.microsoftonline.com",
    "www.dropbox.com",
];

/// Start a sign-in and open the provider's page in the system browser. Returns the flow id.
#[tauri::command]
pub async fn cloud_connect(app: AppHandle, s: State<'_, AppState>, provider: String) -> R<String> {
    let (flow, url) = s.api.cloud_begin_connect(&s.principal(), &provider).await?;
    let ok = tauri::Url::parse(&url).is_ok_and(|u| {
        u.scheme() == "https" && u.host_str().is_some_and(|h| CLOUD_AUTH_HOSTS.contains(&h))
    });
    if !ok {
        s.api.cloud_cancel(&s.principal(), &flow)?;
        return Err(ApiError::invalid("Unexpected sign-in address"));
    }
    use tauri_plugin_opener::OpenerExt;
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        s.api.cloud_cancel(&s.principal(), &flow)?;
        return Err(ApiError::new("IO", format!("Cannot open the browser: {e}")));
    }
    Ok(flow)
}

#[tauri::command]
pub fn cloud_flow(s: State<'_, AppState>, flow_id: String) -> R<CloudFlowDto> {
    s.api.cloud_flow(&s.principal(), &flow_id)
}

#[tauri::command]
pub fn cloud_cancel(s: State<'_, AppState>, flow_id: String) -> R<()> {
    s.api.cloud_cancel(&s.principal(), &flow_id)
}

#[tauri::command]
pub async fn cloud_check(s: State<'_, AppState>, provider: String) -> R<CloudStatusDto> {
    s.api.cloud_check(&s.principal(), &provider).await
}

#[tauri::command]
pub async fn cloud_disconnect(s: State<'_, AppState>, provider: String) -> R<String> {
    s.api.cloud_disconnect(&s.principal(), &provider).await
}

// ───────────────────────────── encryption ─────────────────────────

#[tauri::command]
pub async fn encryption_status(s: State<'_, AppState>) -> R<EncryptionStatusDto> {
    s.api.encryption_status(&s.principal()).await
}

#[tauri::command]
pub async fn encryption_setup(
    s: State<'_, AppState>,
    passphrase: String,
    kit_grant: String,
) -> R<EncryptionStatusDto> {
    s.api
        .encryption_setup(&s.principal(), passphrase, &kit_grant)
        .await
}

#[tauri::command]
pub async fn encryption_import(
    s: State<'_, AppState>,
    kit_grant: String,
    passphrase: String,
) -> R<EncryptionStatusDto> {
    s.api
        .encryption_import(&s.principal(), &kit_grant, passphrase)
        .await
}

#[tauri::command]
pub async fn encryption_set_enabled(
    s: State<'_, AppState>,
    enabled: bool,
) -> R<EncryptionStatusDto> {
    s.api.encryption_set_enabled(&s.principal(), enabled).await
}

/// Pick one existing file (e.g. a Recovery Kit).
#[tauri::command]
pub async fn dialog_pick_file(
    app: AppHandle,
    s: State<'_, AppState>,
    title: String,
) -> R<Option<GrantDto>> {
    let a = app.clone();
    let picked = blocking(move || a.dialog().file().set_title(title).blocking_pick_file()).await?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| s.api.grants.issue(GrantKind::Source, p)))
}

// ──────────────────────────── notifications ───────────────────────

#[tauri::command]
pub async fn notifications_list(s: State<'_, AppState>, limit: u32) -> R<Vec<NotificationDto>> {
    s.api.notifications_list(&s.principal(), limit).await
}

#[tauri::command]
pub async fn notifications_unread(s: State<'_, AppState>) -> R<u32> {
    s.api.notifications_unread(&s.principal()).await
}

#[tauri::command]
pub async fn notifications_mark_read(s: State<'_, AppState>, ids: Option<Vec<String>>) -> R<()> {
    s.api.notifications_mark_read(&s.principal(), ids).await
}

#[tauri::command]
pub async fn notifications_clear(s: State<'_, AppState>) -> R<()> {
    s.api.notifications_clear(&s.principal()).await
}

#[tauri::command]
pub async fn notification_prefs(s: State<'_, AppState>) -> R<NotificationPrefsDto> {
    s.api.notification_prefs(&s.principal()).await
}

#[tauri::command]
pub async fn notification_prefs_update(
    s: State<'_, AppState>,
    prefs: NotificationPrefsDto,
) -> R<NotificationPrefsDto> {
    s.api.notification_prefs_update(&s.principal(), prefs).await
}

// ───────────────────────────── tunnels ────────────────────────────

#[tauri::command]
pub async fn tunnel_status(s: State<'_, AppState>) -> R<TunnelStatusDto> {
    s.api.tunnel_status(&s.principal()).await
}

// ───────────────────────────── bedrock ────────────────────────────

#[tauri::command]
pub async fn bedrock_status(s: State<'_, AppState>, server_id: String) -> R<BedrockStatusDto> {
    s.api.bedrock_status(&s.principal(), &server_id).await
}

#[tauri::command]
pub async fn bedrock_enable(
    s: State<'_, AppState>,
    server_id: String,
    request: BedrockEnableDto,
) -> R<String> {
    s.api
        .bedrock_enable(&s.principal(), &server_id, request)
        .await
}

#[tauri::command]
pub async fn bedrock_configure(
    s: State<'_, AppState>,
    server_id: String,
    settings: BedrockSettingsDto,
) -> R<BedrockStatusDto> {
    s.api
        .bedrock_configure(&s.principal(), &server_id, settings)
        .await
}

#[tauri::command]
pub async fn bedrock_ping(s: State<'_, AppState>, server_id: String) -> R<BedrockPongDto> {
    s.api.bedrock_ping(&s.principal(), &server_id).await
}

// ───────────────────────────── content ────────────────────────────

#[tauri::command]
pub async fn content_list(s: State<'_, AppState>, server_id: String) -> R<ContentListDto> {
    s.api.content_list(&s.principal(), &server_id).await
}

#[tauri::command]
pub async fn content_search(
    s: State<'_, AppState>,
    server_id: String,
    query: SearchRequestDto,
) -> R<SearchPageDto> {
    s.api
        .content_search(&s.principal(), &server_id, query)
        .await
}

#[tauri::command]
pub async fn content_versions(
    s: State<'_, AppState>,
    server_id: String,
    provider: String,
    project_id: String,
) -> R<Vec<ContentVersionDto>> {
    s.api
        .content_versions(&s.principal(), &server_id, &provider, &project_id)
        .await
}

#[tauri::command]
pub async fn content_plan(
    s: State<'_, AppState>,
    server_id: String,
    request: InstallRequestDto,
) -> R<InstallPlanDto> {
    s.api
        .content_plan(&s.principal(), &server_id, request)
        .await
}

#[tauri::command]
pub async fn content_install(
    s: State<'_, AppState>,
    server_id: String,
    request: InstallRequestDto,
) -> R<String> {
    s.api
        .content_install(&s.principal(), &server_id, request)
        .await
}

#[tauri::command]
pub async fn content_remove(
    s: State<'_, AppState>,
    server_id: String,
    file_name: String,
) -> R<bool> {
    s.api
        .content_remove(&s.principal(), &server_id, &file_name)
        .await
}

#[tauri::command]
pub async fn content_set_enabled(
    s: State<'_, AppState>,
    server_id: String,
    file_name: String,
    enabled: bool,
) -> R<bool> {
    s.api
        .content_set_enabled(&s.principal(), &server_id, &file_name, enabled)
        .await
}

#[tauri::command]
pub async fn content_discard_pending(
    s: State<'_, AppState>,
    server_id: String,
    id: String,
) -> R<()> {
    s.api
        .content_discard_pending(&s.principal(), &server_id, &id)
        .await
}

#[tauri::command]
pub async fn content_check_updates(
    s: State<'_, AppState>,
    server_id: String,
) -> R<Vec<UpdateInfoDto>> {
    s.api
        .content_check_updates(&s.principal(), &server_id)
        .await
}

// ───────────────────────────── players ────────────────────────────

#[tauri::command]
pub async fn players_get(s: State<'_, AppState>, server_id: String) -> R<ServerPlayersDto> {
    s.api.players_get(&s.principal(), &server_id).await
}

#[tauri::command]
pub async fn players_action(
    s: State<'_, AppState>,
    server_id: String,
    action: PlayerActionDto,
) -> R<PlayerActionOutcomeDto> {
    s.api
        .players_action(&s.principal(), &server_id, action)
        .await
}

// ───────────────────────────── backups ────────────────────────────

#[tauri::command]
pub async fn backups_list(s: State<'_, AppState>, server_id: Option<String>) -> R<Vec<BackupDto>> {
    s.api
        .backups_list(&s.principal(), server_id.as_deref())
        .await
}

#[tauri::command]
pub async fn backups_create(
    s: State<'_, AppState>,
    server_id: String,
    note: Option<String>,
) -> R<String> {
    s.api.backups_create(&s.principal(), &server_id, note).await
}

#[tauri::command]
pub async fn backups_delete(s: State<'_, AppState>, id: String) -> R<()> {
    s.api.backups_delete(&s.principal(), &id).await
}

#[tauri::command]
pub async fn backups_verify(s: State<'_, AppState>, id: String) -> R<String> {
    s.api.backups_verify(&s.principal(), &id).await
}

#[tauri::command]
pub async fn backups_restore_preview(s: State<'_, AppState>, id: String) -> R<RestorePreviewDto> {
    s.api.backups_restore_preview(&s.principal(), &id).await
}

#[tauri::command]
pub async fn backups_restore(s: State<'_, AppState>, id: String) -> R<String> {
    s.api.backups_restore(&s.principal(), &id).await
}

#[tauri::command]
pub async fn backups_policy(s: State<'_, AppState>, server_id: String) -> R<BackupPolicyDto> {
    s.api.backups_policy(&s.principal(), &server_id).await
}

#[tauri::command]
pub async fn backups_policy_update(
    s: State<'_, AppState>,
    server_id: String,
    policy: BackupPolicyUpdateDto,
) -> R<BackupPolicyDto> {
    s.api
        .backups_policy_update(&s.principal(), &server_id, policy)
        .await
}

#[tauri::command]
pub async fn backups_location(s: State<'_, AppState>) -> R<BackupLocationDto> {
    s.api.backups_location(&s.principal()).await
}

#[tauri::command]
pub async fn backups_set_location(
    s: State<'_, AppState>,
    grant: Option<String>,
) -> R<BackupLocationDto> {
    s.api
        .backups_set_location(&s.principal(), grant.as_deref())
        .await
}

/// Show a backup archive in Explorer. The path comes from the backup record.
#[tauri::command]
pub async fn backups_reveal(s: State<'_, AppState>, id: String) -> R<()> {
    let path = s.api.backups_path(&s.principal(), &id).await?;
    let mut arg = std::ffi::OsString::from("/select,");
    arg.push(&path);
    std::process::Command::new("explorer.exe")
        .arg(arg)
        .spawn()
        .map(|_| ())
        .map_err(|e| ApiError::new("IO", format!("Cannot open folder: {e}")))
}

/// Open the backups folder in Explorer.
#[tauri::command]
pub async fn backups_open_folder(s: State<'_, AppState>) -> R<()> {
    let loc = s.api.backups_location(&s.principal()).await?;
    std::fs::create_dir_all(&loc.directory)
        .map_err(|e| ApiError::new("IO", format!("Cannot create folder: {e}")))?;
    std::process::Command::new("explorer.exe")
        .arg(&loc.directory)
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
    // Project pages of the content providers and the tunnel provider's site (HTTPS,
    // exact host, no credentials).
    const ALLOWED_HOSTS: &[&str] = &[
        "modrinth.com",
        "hangar.papermc.io",
        "geysermc.org",
        "www.spigotmc.org",
        "myaccount.google.com",
        "account.live.com",
        "www.dropbox.com",
        "playit.gg",
    ];
    let host_ok = tauri::Url::parse(&url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none()
            && u.host_str().is_some_and(|h| ALLOWED_HOSTS.contains(&h))
    });
    if !ALLOWED.contains(&url.as_str()) && !host_ok {
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
