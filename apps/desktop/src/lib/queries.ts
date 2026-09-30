import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { api } from "./api";

export const qk = {
  appInfo: ["app-info"] as const,
  settings: ["settings"] as const,
  systemMetrics: ["system-metrics"] as const,
  java: ["java"] as const,
  software: ["software"] as const,
  versions: (softwareId: string, snapshots: boolean) => ["versions", softwareId, snapshots] as const,
  builds: (softwareId: string, version: string) => ["builds", softwareId, version] as const,
  preview: (softwareId: string, version: string, build: string | null) => ["preview", softwareId, version, build] as const,
  propertySchema: (version: string) => ["property-schema", version] as const,
  servers: ["servers"] as const,
  server: (id: string) => ["servers", id] as const,
  serverMetrics: (id: string) => ["servers", id, "metrics"] as const,
  serverProperties: (id: string) => ["servers", id, "properties"] as const,
  files: (id: string, path: string) => ["servers", id, "files", path] as const,
  audit: (serverId: string | null) => ["audit", serverId] as const,
  jobs: ["jobs"] as const,
  templates: ["templates"] as const,
  restartPolicy: (serverId: string) => ["servers", serverId, "restart-policy"] as const,
  crashes: (serverId: string) => ["servers", serverId, "crashes"] as const,
  content: (serverId: string) => ["servers", serverId, "content"] as const,
  bedrock: (serverId: string) => ["servers", serverId, "bedrock"] as const,
  tunnel: ["tunnel"] as const,
  playit: ["playit"] as const,
  account: ["account"] as const,
  playitTunnels: ["playit", "tunnels"] as const,
  encryption: ["encryption"] as const,
  cloud: ["cloud"] as const,
  diskUsage: (serverId: string) => ["servers", serverId, "disk"] as const,
  notifications: ["notifications"] as const,
  unreadNotifications: ["notifications", "unread"] as const,
  notificationPrefs: ["notifications", "prefs"] as const,
  players: (serverId: string) => ["servers", serverId, "players"] as const,
  backups: (serverId: string | null) => ["backups", serverId] as const,
  backupPolicy: (serverId: string) => ["backups", "policy", serverId] as const,
  backupLocation: ["backups", "location"] as const,
};

export const useAppInfo = () => useQuery({ queryKey: qk.appInfo, queryFn: api.app.info, staleTime: Infinity });
export const useSettings = () => useQuery({ queryKey: qk.settings, queryFn: api.settings.get });
export const useServers = () => useQuery({ queryKey: qk.servers, queryFn: api.servers.list });
export const useServer = (id: string) => useQuery({ queryKey: qk.server(id), queryFn: () => api.servers.get(id) });
export const useJava = () => useQuery({ queryKey: qk.java, queryFn: api.java.list });
export const useSoftware = () => useQuery({ queryKey: qk.software, queryFn: api.software.list, staleTime: Infinity });

export const useSystemMetrics = () => useQuery({ queryKey: qk.systemMetrics, queryFn: api.system.metrics, refetchInterval: 2000 });

export const useServerMetrics = (id: string, enabled: boolean) =>
  useQuery({ queryKey: qk.serverMetrics(id), queryFn: () => api.servers.metrics(id), refetchInterval: enabled ? 2000 : false });

export const useAudit = (serverId: string | null, limit = 50) =>
  useQuery({ queryKey: qk.audit(serverId), queryFn: () => api.audit.query(serverId, null, limit) });

const AUDIT_PAGE = 100;

/** Paged audit log (newest first), optionally scoped to one server. */
export function useAuditPages(serverId: string | null) {
  return useInfiniteQuery({
    queryKey: ["audit", serverId, "pages"],
    queryFn: ({ pageParam }) => api.audit.query(serverId, pageParam, AUDIT_PAGE),
    initialPageParam: null as number | null,
    getNextPageParam: (last) => (last.length < AUDIT_PAGE ? undefined : (last[last.length - 1]?.occurredAt ?? undefined)),
  });
}

export const useBackups = (serverId: string | null) => useQuery({ queryKey: qk.backups(serverId), queryFn: () => api.backups.list(serverId) });
export const useBackupPolicy = (serverId: string) => useQuery({ queryKey: qk.backupPolicy(serverId), queryFn: () => api.backups.policy(serverId) });
export const useBackupLocation = () => useQuery({ queryKey: qk.backupLocation, queryFn: api.backups.location });
export const usePlayers = (serverId: string) => useQuery({ queryKey: qk.players(serverId), queryFn: () => api.players.get(serverId) });
export const useContent = (serverId: string) => useQuery({ queryKey: qk.content(serverId), queryFn: () => api.content.list(serverId) });
export const useRestartPolicy = (serverId: string) => useQuery({ queryKey: qk.restartPolicy(serverId), queryFn: () => api.crashes.policy(serverId) });
export const useNotifications = (enabled: boolean) => useQuery({ queryKey: qk.notifications, queryFn: () => api.notifications.list(100), enabled });
export const useUnreadNotifications = () => useQuery({ queryKey: qk.unreadNotifications, queryFn: api.notifications.unread });
export const useNotificationPrefs = () => useQuery({ queryKey: qk.notificationPrefs, queryFn: api.notifications.prefs });
export const useDiskUsage = (serverId: string) =>
  useQuery({ queryKey: qk.diskUsage(serverId), queryFn: () => api.disk.usage(serverId), staleTime: 60_000 });
export const useCloud = () => useQuery({ queryKey: qk.cloud, queryFn: api.cloud.list });
export const useEncryption = () => useQuery({ queryKey: qk.encryption, queryFn: api.encryption.status });
export const useTunnel = () =>
  useQuery({
    queryKey: qk.tunnel,
    queryFn: api.tunnels.status,
    staleTime: 15_000,
    // Follow a pending account link or a starting/stopping agent closely.
    refetchInterval: (q) => (q.state.data?.linkState === "waiting" || /^(starting|stopping)$/.test(q.state.data?.phase ?? "") ? 2_000 : false),
  });
export const useTunnelAddress = (serverId: string) =>
  useQuery({ queryKey: [...qk.tunnel, "address", serverId], queryFn: () => api.tunnels.serverAddress(serverId) });
export const useBedrock = (serverId: string) => useQuery({ queryKey: qk.bedrock(serverId), queryFn: () => api.bedrock.status(serverId) });
export const useCrashes = (serverId: string) => useQuery({ queryKey: qk.crashes(serverId), queryFn: () => api.crashes.history(serverId, 10) });
export const useTemplates = () => useQuery({ queryKey: qk.templates, queryFn: api.templates.list, staleTime: Infinity });

export const usePlayitAgent = () =>
  useQuery({
    queryKey: qk.playit,
    queryFn: api.playit.status,
    staleTime: 10_000,
    // Follow a pending link closely.
    refetchInterval: (q) => (q.state.data?.linkState === "waiting" ? 2_000 : 15_000),
  });
export const usePlayitTunnels = (enabled: boolean) =>
  useQuery({
    queryKey: qk.playitTunnels,
    queryFn: api.playit.tunnels,
    enabled,
    staleTime: 10_000,
    refetchInterval: enabled ? 20_000 : false,
    retry: 1,
  });
export const useAccount = () =>
  useQuery({
    queryKey: qk.account,
    queryFn: api.account.status,
    staleTime: 30_000,
    // Follow a Google sign-in in the browser closely.
    refetchInterval: (q) => (q.state.data?.googleState === "waiting" ? 1_500 : false),
  });
