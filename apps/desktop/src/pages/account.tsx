import { useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import {
  Archive,
  Cloud,
  Copy,
  Check,
  Globe,
  HardDrive,
  KeyRound,
  Network,
  RefreshCw,
  Server,
  Settings,
  ShieldCheck,
  Sparkles,
  UserCheck,
} from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { PageBody, PageHeader } from "@/app/app-shell";
import { AccountPanel } from "@/components/account-panel";
import { Button } from "@/components/ui/button";
import { Badge, Card, CardHeader } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { qk, useAccount, useAiConfig, useBackups, useMultihostStatus, usePlayitAgent, useServers } from "@/lib/queries";

export function AccountPage() {
  const qc = useQueryClient();
  const { data: account } = useAccount();
  const { data: servers } = useServers();
  const { data: backups } = useBackups(null);
  const { data: multihost } = useMultihostStatus();
  const { data: playit } = usePlayitAgent();
  const { data: aiConfig } = useAiConfig();

  const [copiedUid, setCopiedUid] = useState(false);
  const [syncing, setSyncing] = useState(false);

  const runningServers = (servers ?? []).filter((s) => s.state === "running").length;
  const totalServers = servers?.length ?? 0;
  const totalBackups = backups?.length ?? 0;
  const totalHosts = multihost?.hosts?.length ?? 1;

  const copyUid = (uid: string) => {
    navigator.clipboard.writeText(uid).then(() => {
      setCopiedUid(true);
      toast.success("Account UID copied to clipboard");
      setTimeout(() => setCopiedUid(false), 2000);
    });
  };

  const handleSyncSettings = async () => {
    setSyncing(true);
    try {
      await api.account.syncSettings();
      await qc.invalidateQueries({ queryKey: qk.settings });
      toast.success("Settings synchronized with cloud account");
    } catch (e) {
      toast.error(`Sync failed: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSyncing(false);
    }
  };

  return (
    <>
      <PageHeader
        title="Account & Identity"
        description="Manage your profile, cloud synchronization, linked API keys, and workspace connectivity."
        actions={
          account?.signedIn && (
            <Button variant="outline" size="sm" onClick={() => void handleSyncSettings()} disabled={syncing}>
              <RefreshCw className={`size-3.5 ${syncing ? "animate-spin" : ""}`} /> Sync Now
            </Button>
          )
        }
      />
      <PageBody className="space-y-6 max-w-5xl">
        <div className="grid gap-6 md:grid-cols-3">
          {/* Main Account Authentication & Profile (2 columns) */}
          <div className="space-y-6 md:col-span-2">
            <Card>
              <CardHeader
                title="Profile & Sign-in"
                description="Your MCPanel account coordinates settings across devices and links authorized services."
                actions={
                  account?.signedIn ? (
                    <Badge tone="success" className="gap-1">
                      <UserCheck className="size-3" /> Signed In
                    </Badge>
                  ) : account?.isGuest ? (
                    <Badge tone="neutral">Guest Mode</Badge>
                  ) : (
                    <Badge tone="warning">Not Signed In</Badge>
                  )
                }
              />
              <div className="p-5">
                <AccountPanel />
              </div>
            </Card>

            {/* Account Details & Cloud Synchronization */}
            {account?.signedIn && account.profile && (
              <Card>
                <CardHeader
                  title="Cloud Sync & Data Link"
                  description="Status of your preferences and API credentials linked to this account."
                />
                <div className="divide-y divide-border px-5 text-[13px]">
                  <div className="flex items-center justify-between py-3">
                    <div>
                      <p className="font-medium text-fg">Account Identifier (UID)</p>
                      <p className="text-xs text-muted">Unique ID used for cloud settings and permission scoping.</p>
                    </div>
                    <div className="flex items-center gap-2">
                      <code className="rounded bg-surface-2 px-2 py-1 font-mono text-xs text-fg select-all">
                        {account.profile.uid}
                      </code>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => copyUid(account.profile?.uid || "")}
                        aria-label="Copy account UID"
                      >
                        {copiedUid ? <Check className="size-3.5 text-accent" /> : <Copy className="size-3.5" />}
                      </Button>
                    </div>
                  </div>

                  <div className="flex items-center justify-between py-3">
                    <div>
                      <p className="font-medium text-fg">AI Assistant API Key Sync</p>
                      <p className="text-xs text-muted">
                        {aiConfig?.syncApiKeys
                          ? "API keys are linked to this account in Windows Credential Manager."
                          : "API key syncing is disabled; keys are strictly stored on this local machine."}
                      </p>
                    </div>
                    <Badge tone={aiConfig?.syncApiKeys ? "success" : "neutral"}>
                      {aiConfig?.syncApiKeys ? "Account Linked" : "Local Only"}
                    </Badge>
                  </div>

                  <div className="flex items-center justify-between py-3">
                    <div>
                      <p className="font-medium text-fg">Preferences Sync</p>
                      <p className="text-xs text-muted">Theme, accent color, and buffer limits synchronized via cloud storage.</p>
                    </div>
                    <Badge tone="success" className="gap-1">
                      <Cloud className="size-3" /> Active
                    </Badge>
                  </div>
                </div>
              </Card>
            )}

            {/* Security & Credentials Architecture */}
            <Card>
              <CardHeader
                title="Security & Local Encryption"
                description="How MCPanel protects your credentials, tokens, and server operations."
              />
              <div className="space-y-3 p-5 text-xs text-muted">
                <div className="flex items-start gap-3">
                  <ShieldCheck className="size-5 shrink-0 text-accent" />
                  <div className="space-y-1">
                    <p className="font-medium text-fg">Hardware-Backed Credential Storage</p>
                    <p>
                      AI API keys, cloud storage OAuth tokens, and server secrets are never written to plain-text files or Git. They are stored directly in the Windows Credential Manager.
                    </p>
                  </div>
                </div>
                <div className="flex items-start gap-3 pt-1">
                  <KeyRound className="size-5 shrink-0 text-accent" />
                  <div className="space-y-1">
                    <p className="font-medium text-fg">Per-Account Scoping & Isolation</p>
                    <p>
                      Credentials stored for one account cannot be accessed if you log into a different account or switch to guest mode.
                    </p>
                  </div>
                </div>
              </div>
            </Card>
          </div>

          {/* Side Column: Workspace Stats & Quick Navigation */}
          <div className="space-y-6">
            <Card>
              <CardHeader title="Workspace Overview" description="Local instances managed on this machine." />
              <div className="divide-y divide-border px-5 text-xs">
                <div className="flex items-center justify-between py-2.5">
                  <span className="flex items-center gap-2 text-muted">
                    <Server className="size-4 text-accent" /> Servers
                  </span>
                  <span className="font-medium text-fg">
                    {totalServers} ({runningServers} online)
                  </span>
                </div>
                <div className="flex items-center justify-between py-2.5">
                  <span className="flex items-center gap-2 text-muted">
                    <Archive className="size-4 text-accent" /> Backups
                  </span>
                  <span className="font-medium text-fg">{totalBackups}</span>
                </div>
                <div className="flex items-center justify-between py-2.5">
                  <span className="flex items-center gap-2 text-muted">
                    <Network className="size-4 text-accent" /> Remote Hosts
                  </span>
                  <span className="font-medium text-fg">{totalHosts}</span>
                </div>
                <div className="flex items-center justify-between py-2.5">
                  <span className="flex items-center gap-2 text-muted">
                    <Globe className="size-4 text-accent" /> Playit.gg
                  </span>
                  <Badge tone={playit?.linked ? "success" : "neutral"} className="text-[10px]">
                    {playit?.linked ? "Linked" : playit?.installed ? "Ready" : "Not installed"}
                  </Badge>
                </div>
                <div className="flex items-center justify-between py-2.5">
                  <span className="flex items-center gap-2 text-muted">
                    <Sparkles className="size-4 text-accent" /> AI Assistant
                  </span>
                  <Badge tone={aiConfig?.configured ? "success" : "neutral"} className="text-[10px]">
                    {aiConfig?.configured ? "Configured" : "Inactive"}
                  </Badge>
                </div>
              </div>
            </Card>

            <Card>
              <CardHeader title="Quick Actions" />
              <div className="space-y-2 p-5 text-xs">
                <Button asChild variant="outline" size="sm" className="w-full justify-start gap-2">
                  <Link to="/settings">
                    <Settings className="size-3.5" /> Open Application Settings
                  </Link>
                </Button>
                <Button asChild variant="outline" size="sm" className="w-full justify-start gap-2">
                  <Link to="/ai">
                    <Sparkles className="size-3.5" /> Open AI Assistant
                  </Link>
                </Button>
                <Button asChild variant="outline" size="sm" className="w-full justify-start gap-2">
                  <Link to="/backups">
                    <HardDrive className="size-3.5" /> Manage Backups
                  </Link>
                </Button>
              </div>
            </Card>
          </div>
        </div>
      </PageBody>
    </>
  );
}
