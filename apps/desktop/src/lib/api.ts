/**
 * Typed client for the MCPanel Application API (Tauri IPC transport).
 * This is the ONLY module allowed to call `invoke` (enforced by ESLint).
 * Types come from Rust DTOs via ts-rs (`src/bindings`).
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import type { ApiError as ApiErrorDto } from "@/bindings/ApiError";
import type { AppInfoDto } from "@/bindings/AppInfoDto";
import type { AuditEntryDto } from "@/bindings/AuditEntryDto";
import type { BackupDto } from "@/bindings/BackupDto";
import type { BackupLocationDto } from "@/bindings/BackupLocationDto";
import type { BackupPolicyDto } from "@/bindings/BackupPolicyDto";
import type { BackupPolicyUpdateDto } from "@/bindings/BackupPolicyUpdateDto";
import type { ConsoleBatchDto } from "@/bindings/ConsoleBatchDto";
import type { ContentListDto } from "@/bindings/ContentListDto";
import type { ContentVersionDto } from "@/bindings/ContentVersionDto";
import type { ConsoleLineDto } from "@/bindings/ConsoleLineDto";
import type { BedrockEnableDto } from "@/bindings/BedrockEnableDto";
import type { BedrockPongDto } from "@/bindings/BedrockPongDto";
import type { BedrockSettingsDto } from "@/bindings/BedrockSettingsDto";
import type { BedrockStatusDto } from "@/bindings/BedrockStatusDto";
import type { CrashEventDto } from "@/bindings/CrashEventDto";
import type { EncryptionStatusDto } from "@/bindings/EncryptionStatusDto";
import type { NotificationDto } from "@/bindings/NotificationDto";
import type { NotificationPrefsDto } from "@/bindings/NotificationPrefsDto";
import type { TunnelStatusDto } from "@/bindings/TunnelStatusDto";
import type { CreateServerDto } from "@/bindings/CreateServerDto";
import type { FileEntryDto } from "@/bindings/FileEntryDto";
import type { FileOpResultDto } from "@/bindings/FileOpResultDto";
import type { GameVersionDto } from "@/bindings/GameVersionDto";
import type { GrantDto } from "@/bindings/GrantDto";
import type { ImportDetectionDto } from "@/bindings/ImportDetectionDto";
import type { ImportServerDto } from "@/bindings/ImportServerDto";
import type { InstallPlanDto } from "@/bindings/InstallPlanDto";
import type { InstallPreviewDto } from "@/bindings/InstallPreviewDto";
import type { InstallRequestDto } from "@/bindings/InstallRequestDto";
import type { JavaRuntimeDto } from "@/bindings/JavaRuntimeDto";
import type { JobDto } from "@/bindings/JobDto";
import type { LocationCheckDto } from "@/bindings/LocationCheckDto";
import type { PropertyChangeDto } from "@/bindings/PropertyChangeDto";
import type { PlayerActionDto } from "@/bindings/PlayerActionDto";
import type { PlayerActionOutcomeDto } from "@/bindings/PlayerActionOutcomeDto";
import type { PropertyDto } from "@/bindings/PropertyDto";
import type { ResolvedTemplateDto } from "@/bindings/ResolvedTemplateDto";
import type { RestartPolicyDto } from "@/bindings/RestartPolicyDto";
import type { RestorePreviewDto } from "@/bindings/RestorePreviewDto";
import type { SearchPageDto } from "@/bindings/SearchPageDto";
import type { SearchRequestDto } from "@/bindings/SearchRequestDto";
import type { ServerDto } from "@/bindings/ServerDto";
import type { ServerMetricsDto } from "@/bindings/ServerMetricsDto";
import type { ServerPlayersDto } from "@/bindings/ServerPlayersDto";
import type { ServerPropertiesDto } from "@/bindings/ServerPropertiesDto";
import type { SettingsDto } from "@/bindings/SettingsDto";
import type { SettingsPatchDto } from "@/bindings/SettingsPatchDto";
import type { SoftwareBuildDto } from "@/bindings/SoftwareBuildDto";
import type { SoftwareDto } from "@/bindings/SoftwareDto";
import type { SystemMetricsDto } from "@/bindings/SystemMetricsDto";
import type { TemplateDto } from "@/bindings/TemplateDto";
import type { TextDocumentDto } from "@/bindings/TextDocumentDto";
import type { UpdateInfoDto } from "@/bindings/UpdateInfoDto";
import type { UpdateServerDto } from "@/bindings/UpdateServerDto";
import type { WriteTextDto } from "@/bindings/WriteTextDto";

export class ApiError extends Error {
  readonly code: string;
  readonly details: unknown;
  readonly retryable: boolean;

  constructor(dto: ApiErrorDto) {
    super(dto.message);
    this.name = "ApiError";
    this.code = dto.code;
    this.details = dto.details;
    this.retryable = dto.retryable;
  }
}

function toApiError(e: unknown): ApiError {
  if (e instanceof ApiError) return e;
  if (e && typeof e === "object" && "code" in e && "message" in e) {
    const o = e as ApiErrorDto;
    return new ApiError({ code: String(o.code), message: String(o.message), details: o.details ?? null, retryable: Boolean(o.retryable) });
  }
  return new ApiError({ code: "INTERNAL", message: typeof e === "string" ? e : "Unexpected error", details: null, retryable: false });
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    throw toApiError(e);
  }
}

export const api = {
  app: {
    info: () => call<AppInfoDto>("app_info"),
    quit: (mode: "stop" | "leave") => call<void>("app_quit", { mode }),
    openLogsFolder: () => call<void>("open_logs_folder"),
    openExternal: (url: string) => call<void>("open_external", { url }),
  },
  system: {
    metrics: () => call<SystemMetricsDto>("system_metrics"),
  },
  settings: {
    get: () => call<SettingsDto>("settings_get"),
    update: (patch: SettingsPatchDto) => call<SettingsDto>("settings_update", { patch }),
  },
  dialog: {
    pickFolder: (title: string) => call<GrantDto | null>("dialog_pick_folder", { title }),
    pickJava: () => call<GrantDto | null>("dialog_pick_java"),
    pickImport: (folder: boolean) => call<GrantDto[]>("dialog_pick_import", { folder }),
    saveFile: (defaultName: string) => call<GrantDto | null>("dialog_save_file", { defaultName }),
    pickFile: (title: string) => call<GrantDto | null>("dialog_pick_file", { title }),
  },
  java: {
    list: () => call<JavaRuntimeDto[]>("java_list"),
    detect: () => call<JavaRuntimeDto[]>("java_detect"),
    add: (grant: string) => call<JavaRuntimeDto>("java_add", { grant }),
    revalidate: (id: string) => call<JavaRuntimeDto>("java_revalidate", { id }),
    remove: (id: string) => call<void>("java_remove", { id }),
  },
  software: {
    list: () => call<SoftwareDto[]>("software_list"),
    versions: (softwareId: string, includeSnapshots: boolean) => call<GameVersionDto[]>("software_versions", { softwareId, includeSnapshots }),
    builds: (softwareId: string, gameVersion: string) => call<SoftwareBuildDto[]>("software_builds", { softwareId, gameVersion }),
    preview: (softwareId: string, gameVersion: string, build: string | null) =>
      call<InstallPreviewDto>("software_preview", { softwareId, gameVersion, build }),
    propertySchema: (gameVersion: string) => call<PropertyDto[]>("software_property_schema", { gameVersion }),
  },
  servers: {
    list: () => call<ServerDto[]>("servers_list"),
    get: (id: string) => call<ServerDto>("servers_get", { id }),
    checkLocation: (name: string, parentGrant: string | null) => call<LocationCheckDto>("servers_check_location", { name, parentGrant }),
    create: (request: CreateServerDto) => call<string>("servers_create", { request }),
    detectImport: (grant: string) => call<ImportDetectionDto>("servers_detect_import", { grant }),
    import: (request: ImportServerDto) => call<ServerDto>("servers_import", { request }),
    update: (id: string, request: UpdateServerDto) => call<ServerDto>("servers_update", { id, request }),
    delete: (id: string, deleteFiles: boolean) => call<void>("servers_delete", { id, deleteFiles }),
    acceptEula: (id: string) => call<void>("servers_accept_eula", { id }),
    start: (id: string) => call<void>("servers_start", { id }),
    stop: (id: string, force = false) => call<void>("servers_stop", { id, force }),
    restart: (id: string) => call<void>("servers_restart", { id }),
    command: (id: string, command: string) => call<void>("servers_command", { id, command }),
    metrics: (id: string) => call<ServerMetricsDto>("servers_metrics", { id }),
    runningCount: () => call<number>("servers_running_count"),
    properties: (id: string) => call<ServerPropertiesDto>("servers_properties", { id }),
    updateProperties: (id: string, changes: PropertyChangeDto[]) => call<ServerPropertiesDto>("servers_properties_update", { id, changes }),
    openFolder: (id: string) => call<void>("servers_open_folder", { id }),
  },
  console: {
    history: (id: string, fromSeq: number | null, limit: number) => call<ConsoleLineDto[]>("console_history", { id, fromSeq, limit }),
    search: (id: string, query: string, limit: number) => call<ConsoleLineDto[]>("console_search", { id, query, limit }),
    export: (id: string, grant: string) => call<number>("console_export", { id, grant }),
    /** Subscribe to live console batches. Returns an unsubscribe function. */
    subscribe: async (id: string, afterSeq: number | null, backlog: number, onBatch: (b: ConsoleBatchDto) => void) => {
      const channel = new Channel<ConsoleBatchDto>();
      channel.onmessage = onBatch;
      const subscription = await call<string>("console_subscribe", { id, afterSeq, backlog, onBatch: channel });
      return () => {
        void call<void>("console_unsubscribe", { subscription }).catch(() => undefined);
      };
    },
  },
  files: {
    list: (id: string, path: string) => call<FileEntryDto[]>("files_list", { id, path }),
    read: (id: string, path: string) => call<TextDocumentDto>("files_read", { id, path }),
    write: (id: string, path: string, document: WriteTextDto) => call<TextDocumentDto>("files_write", { id, path, document }),
    mkdir: (id: string, path: string) => call<FileEntryDto>("files_mkdir", { id, path }),
    create: (id: string, path: string) => call<FileEntryDto>("files_create", { id, path }),
    rename: (id: string, path: string, newName: string) => call<FileEntryDto>("files_rename", { id, path, newName }),
    move: (id: string, paths: string[], destination: string) => call<void>("files_move", { id, paths, destination }),
    copy: (id: string, paths: string[], destination: string) => call<FileOpResultDto>("files_copy", { id, paths, destination }),
    delete: (id: string, paths: string[], permanent: boolean) => call<void>("files_delete", { id, paths, permanent }),
    zip: (id: string, paths: string[], archiveName: string) => call<FileOpResultDto>("files_zip", { id, paths, archiveName }),
    unzip: (id: string, archive: string, destination: string, overwrite: boolean) =>
      call<FileOpResultDto>("files_unzip", { id, archive, destination, overwrite }),
    import: (id: string, grants: string[], destination: string) => call<FileEntryDto[]>("files_import", { id, grants, destination }),
    export: (id: string, path: string, grant: string) => call<number>("files_export", { id, path, grant }),
    search: (id: string, path: string, query: string, limit: number) => call<FileEntryDto[]>("files_search", { id, path, query, limit }),
  },
  templates: {
    list: () => call<TemplateDto[]>("templates_list"),
    resolve: (id: string, softwareId: string, gameVersion: string) => call<ResolvedTemplateDto>("templates_resolve", { id, softwareId, gameVersion }),
    /** Returns the plugin install job ids. */
    apply: (serverId: string, id: string, plugins: string[]) => call<string[]>("templates_apply", { serverId, id, plugins }),
  },
  crashes: {
    policy: (serverId: string) => call<RestartPolicyDto>("restart_policy", { serverId }),
    updatePolicy: (serverId: string, policy: RestartPolicyDto) => call<RestartPolicyDto>("restart_policy_update", { serverId, policy }),
    history: (serverId: string, limit: number) => call<CrashEventDto[]>("crash_history", { serverId, limit }),
  },
  encryption: {
    status: () => call<EncryptionStatusDto>("encryption_status"),
    setup: (passphrase: string, kitGrant: string) => call<EncryptionStatusDto>("encryption_setup", { passphrase, kitGrant }),
    import: (kitGrant: string, passphrase: string) => call<EncryptionStatusDto>("encryption_import", { kitGrant, passphrase }),
    setEnabled: (enabled: boolean) => call<EncryptionStatusDto>("encryption_set_enabled", { enabled }),
  },
  notifications: {
    list: (limit: number) => call<NotificationDto[]>("notifications_list", { limit }),
    unread: () => call<number>("notifications_unread"),
    /** `null` marks everything read. */
    markRead: (ids: string[] | null) => call<void>("notifications_mark_read", { ids }),
    clear: () => call<void>("notifications_clear"),
    prefs: () => call<NotificationPrefsDto>("notification_prefs"),
    updatePrefs: (prefs: NotificationPrefsDto) => call<NotificationPrefsDto>("notification_prefs_update", { prefs }),
  },
  tunnels: {
    status: () => call<TunnelStatusDto>("tunnel_status"),
  },
  bedrock: {
    status: (serverId: string) => call<BedrockStatusDto>("bedrock_status", { serverId }),
    /** Returns the setup job id. */
    enable: (serverId: string, request: BedrockEnableDto) => call<string>("bedrock_enable", { serverId, request }),
    configure: (serverId: string, settings: BedrockSettingsDto) => call<BedrockStatusDto>("bedrock_configure", { serverId, settings }),
    ping: (serverId: string) => call<BedrockPongDto>("bedrock_ping", { serverId }),
  },
  content: {
    list: (serverId: string) => call<ContentListDto>("content_list", { serverId }),
    search: (serverId: string, query: SearchRequestDto) => call<SearchPageDto>("content_search", { serverId, query }),
    versions: (serverId: string, provider: string, projectId: string) =>
      call<ContentVersionDto[]>("content_versions", { serverId, provider, projectId }),
    plan: (serverId: string, request: InstallRequestDto) => call<InstallPlanDto>("content_plan", { serverId, request }),
    /** Returns the job id. */
    install: (serverId: string, request: InstallRequestDto) => call<string>("content_install", { serverId, request }),
    /** Returns true when the change was queued until the server stops. */
    remove: (serverId: string, fileName: string) => call<boolean>("content_remove", { serverId, fileName }),
    setEnabled: (serverId: string, fileName: string, enabled: boolean) => call<boolean>("content_set_enabled", { serverId, fileName, enabled }),
    discardPending: (serverId: string, id: string) => call<void>("content_discard_pending", { serverId, id }),
    checkUpdates: (serverId: string) => call<UpdateInfoDto[]>("content_check_updates", { serverId }),
  },
  players: {
    get: (serverId: string) => call<ServerPlayersDto>("players_get", { serverId }),
    /** Sent to the running server's console, or written to its files when stopped. */
    action: (serverId: string, action: PlayerActionDto) => call<PlayerActionOutcomeDto>("players_action", { serverId, action }),
  },
  backups: {
    list: (serverId: string | null) => call<BackupDto[]>("backups_list", { serverId }),
    /** Returns the job id. */
    create: (serverId: string, note: string | null) => call<string>("backups_create", { serverId, note }),
    delete: (id: string) => call<void>("backups_delete", { id }),
    /** Returns the job id; the job result is `{ ok, filesChecked, bytesChecked, problems }`. */
    verify: (id: string) => call<string>("backups_verify", { id }),
    restorePreview: (id: string) => call<RestorePreviewDto>("backups_restore_preview", { id }),
    /** Returns the job id. */
    restore: (id: string) => call<string>("backups_restore", { id }),
    policy: (serverId: string) => call<BackupPolicyDto>("backups_policy", { serverId }),
    updatePolicy: (serverId: string, policy: BackupPolicyUpdateDto) => call<BackupPolicyDto>("backups_policy_update", { serverId, policy }),
    location: () => call<BackupLocationDto>("backups_location"),
    /** `null` resets to the default folder. */
    setLocation: (grant: string | null) => call<BackupLocationDto>("backups_set_location", { grant }),
    reveal: (id: string) => call<void>("backups_reveal", { id }),
    openFolder: () => call<void>("backups_open_folder"),
  },
  jobs: {
    list: (limit: number) => call<JobDto[]>("jobs_list", { limit }),
    get: (id: string) => call<JobDto>("jobs_get", { id }),
    cancel: (id: string) => call<boolean>("jobs_cancel", { id }),
  },
  audit: {
    query: (serverId: string | null, before: number | null, limit: number) => call<AuditEntryDto[]>("audit_query", { serverId, before, limit }),
  },
};

export type Api = typeof api;
