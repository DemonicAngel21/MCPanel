import type { QueryClient } from "@tanstack/react-query";
import { createHashHistory, createRootRouteWithContext, createRoute, createRouter } from "@tanstack/react-router";
import { lazy } from "react";
import { AppShell } from "@/app/app-shell";
import { ErrorView } from "@/app/error-view";
import { DashboardPage } from "@/pages/dashboard";
import { ServersPage } from "@/pages/servers";
import { CreateServerPage } from "@/pages/create-server";
import { ImportServerPage } from "@/pages/import-server";
import { JavaPage } from "@/pages/java";
import { ActivityPage } from "@/pages/activity";
import { BackupsPage } from "@/pages/backups";
import { SettingsPage } from "@/pages/settings";
import { ServerLayout } from "@/pages/server/layout";
import { ServerOverview } from "@/pages/server/overview";
import { ServerConsole } from "@/pages/server/console";
import { ServerFiles } from "@/pages/server/files";
import { ServerProperties } from "@/pages/server/properties";
import { ServerSettings } from "@/pages/server/settings";
import { ServerActivity } from "@/pages/server/activity";
import { ServerBackups } from "@/pages/server/backups";
import { ServerPlayers } from "@/pages/server/players";
import { ServerContent } from "@/pages/server/content";

// Monaco is large: load the editor only when a file is opened.
const ServerEditor = lazy(() => import("@/pages/server/editor"));

const rootRoute = createRootRouteWithContext<{ queryClient: QueryClient }>()({ component: AppShell });

const dashboard = createRoute({ getParentRoute: () => rootRoute, path: "/", component: DashboardPage });
const servers = createRoute({ getParentRoute: () => rootRoute, path: "/servers", component: ServersPage });
const createServer = createRoute({ getParentRoute: () => rootRoute, path: "/servers/new", component: CreateServerPage });
const importServer = createRoute({ getParentRoute: () => rootRoute, path: "/servers/import", component: ImportServerPage });
const java = createRoute({ getParentRoute: () => rootRoute, path: "/java", component: JavaPage });
const backups = createRoute({ getParentRoute: () => rootRoute, path: "/backups", component: BackupsPage });
const activity = createRoute({ getParentRoute: () => rootRoute, path: "/activity", component: ActivityPage });
const settings = createRoute({ getParentRoute: () => rootRoute, path: "/settings", component: SettingsPage });

export const serverRoute = createRoute({ getParentRoute: () => rootRoute, path: "/servers/$serverId", component: ServerLayout });
const serverOverview = createRoute({ getParentRoute: () => serverRoute, path: "/", component: ServerOverview });
const serverConsole = createRoute({ getParentRoute: () => serverRoute, path: "/console", component: ServerConsole });
export const serverFilesRoute = createRoute({
  getParentRoute: () => serverRoute,
  path: "/files",
  validateSearch: (s: Record<string, unknown>): { path: string } => ({ path: typeof s.path === "string" ? s.path : "" }),
  component: ServerFiles,
});
export const serverEditorRoute = createRoute({
  getParentRoute: () => serverRoute,
  path: "/edit",
  validateSearch: (s: Record<string, unknown>): { path: string } => ({ path: typeof s.path === "string" ? s.path : "" }),
  component: ServerEditor,
});
const serverProperties = createRoute({ getParentRoute: () => serverRoute, path: "/properties", component: ServerProperties });
const serverSettings = createRoute({ getParentRoute: () => serverRoute, path: "/settings", component: ServerSettings });
const serverPlayers = createRoute({ getParentRoute: () => serverRoute, path: "/players", component: ServerPlayers });
const serverContent = createRoute({ getParentRoute: () => serverRoute, path: "/content", component: ServerContent });
const serverBackups = createRoute({ getParentRoute: () => serverRoute, path: "/backups", component: ServerBackups });
const serverActivity = createRoute({ getParentRoute: () => serverRoute, path: "/activity", component: ServerActivity });

const routeTree = rootRoute.addChildren([
  dashboard,
  servers,
  createServer,
  importServer,
  java,
  backups,
  activity,
  settings,
  serverRoute.addChildren([
    serverOverview,
    serverConsole,
    serverFilesRoute,
    serverEditorRoute,
    serverPlayers,
    serverContent,
    serverProperties,
    serverBackups,
    serverSettings,
    serverActivity,
  ]),
]);

export const router = createRouter({
  routeTree,
  history: createHashHistory(),
  context: { queryClient: undefined as unknown as QueryClient },
  defaultPreload: false,
  defaultErrorComponent: ErrorView,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
