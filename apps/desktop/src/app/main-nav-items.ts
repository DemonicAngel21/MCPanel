import {
  Activity,
  Archive,
  Coffee,
  Globe,
  LayoutDashboard,
  LayoutTemplate,
  Network,
  Server,
  Settings,
  Sparkles,
  UserRound,
  type LucideIcon,
} from "lucide-react";

export const MAIN_NAV_ITEMS: { to: string; label: string; icon: LucideIcon; exact?: boolean }[] = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard, exact: true },
  { to: "/servers", label: "Servers", icon: Server },
  { to: "/hosts", label: "Hosts", icon: Network },
  { to: "/backups", label: "Backups", icon: Archive },
  { to: "/java", label: "Java runtimes", icon: Coffee },
  { to: "/templates", label: "Templates", icon: LayoutTemplate },
  { to: "/playit", label: "Playit.gg", icon: Globe },
  { to: "/ai", label: "AI Assistant", icon: Sparkles },
  { to: "/account", label: "Account", icon: UserRound },
  { to: "/activity", label: "Activity", icon: Activity },
  { to: "/settings", label: "Settings", icon: Settings },
];
