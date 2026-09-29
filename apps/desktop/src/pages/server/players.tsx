import { useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, Ban, Crown, DoorOpen, MoreHorizontal, ShieldCheck, ShieldOff, UserPlus, Users } from "lucide-react";
import { useState, type FormEvent, type ReactNode } from "react";
import { toast } from "sonner";
import type { KnownPlayerDto } from "@/bindings/KnownPlayerDto";
import type { PlayerActionDto } from "@/bindings/PlayerActionDto";
import type { ServerPlayersDto } from "@/bindings/ServerPlayersDto";
import { PageBody } from "@/app/app-shell";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import { Badge, Banner, Card, CardHeader, EmptyState, Field, Input, SkeletonRows, StatusDot, Switch, Tooltip } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatDuration, formatRelative } from "@/lib/format";
import { qk, usePlayers } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";
import { useServerId } from "./use-server-id";

type Tab = "players" | "whitelist" | "operators" | "bans";

/** Run an action and report the server's reply (or MCPanel's result). */
function useAction(serverId: string) {
  const qc = useQueryClient();
  const [busy, setBusy] = useState(false);
  const run = async (action: PlayerActionDto): Promise<boolean> => {
    setBusy(true);
    try {
      const o = await api.players.action(serverId, action);
      const text = o.messages.join("\n");
      if (o.via === "console") toast.message(text || "Command sent to the server", { description: text ? "Server reply" : undefined });
      else toast.success(text || "Saved");
      await qc.invalidateQueries({ queryKey: qk.players(serverId) });
      return true;
    } catch (e) {
      toast.error(errorMessage(e));
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { run, busy };
}

function NameForm({
  label,
  placeholder,
  onSubmit,
  disabled,
}: {
  label: string;
  placeholder: string;
  onSubmit: (name: string) => Promise<boolean>;
  disabled: boolean;
}) {
  const [name, setName] = useState("");
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (name.trim() && (await onSubmit(name.trim()))) setName("");
  };
  return (
    <form onSubmit={submit} className="flex gap-2 border-b border-border p-3">
      <Input
        value={name}
        onChange={(e) => setName(e.target.value)}
        placeholder={placeholder}
        maxLength={32}
        disabled={disabled}
        className="max-w-xs"
      />
      <Button type="submit" size="sm" variant="primary" disabled={disabled || !name.trim()}>
        <UserPlus /> {label}
      </Button>
    </form>
  );
}

function ReasonDialog({
  title,
  description,
  confirm,
  withIp,
  onClose,
  onSubmit,
}: {
  title: string;
  description: string;
  confirm: string;
  withIp?: boolean;
  onClose: () => void;
  onSubmit: (target: string, reason: string | null) => Promise<boolean>;
}) {
  const [target, setTarget] = useState("");
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    setBusy(true);
    const ok = await onSubmit(target.trim(), reason.trim() || null);
    setBusy(false);
    if (ok) onClose();
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        title={title}
        description={description}
        footer={
          <>
            <DialogClose asChild>
              <Button variant="ghost">Cancel</Button>
            </DialogClose>
            <Button variant="danger" disabled={busy || (withIp !== undefined && !target.trim())} onClick={submit}>
              {confirm}
            </Button>
          </>
        }
      >
        <div className="space-y-3">
          {withIp !== undefined && (
            <Field label={withIp ? "IP address" : "Player name"}>
              <Input value={target} onChange={(e) => setTarget(e.target.value)} autoFocus maxLength={withIp ? 45 : 32} />
            </Field>
          )}
          <Field label="Reason (optional)" hint="Shown to the player.">
            <Input value={reason} onChange={(e) => setReason(e.target.value)} maxLength={256} autoFocus={withIp === undefined} />
          </Field>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function PlayerRow({
  p,
  data,
  run,
  onKick,
  onBan,
}: {
  p: KnownPlayerDto;
  data: ServerPlayersDto;
  run: (a: PlayerActionDto) => Promise<boolean>;
  onKick: () => void;
  onBan: () => void;
}) {
  const canEdit = !data.readOnlyReason;
  return (
    <tr className="border-b border-border last:border-0 hover:bg-surface-2">
      <td className="px-4 py-2">
        <div className="flex items-center gap-2">
          <StatusDot tone={p.online ? "success" : "neutral"} />
          <Tooltip content={p.uuid ?? "UUID unknown"}>
            <span className="text-fg">{p.name}</span>
          </Tooltip>
          {p.opLevel != null && (
            <Tooltip content={`Operator (level ${p.opLevel})`}>
              <Badge tone="info">
                <Crown className="size-3" /> Op
              </Badge>
            </Tooltip>
          )}
          {p.whitelisted && <Badge tone="success">Whitelisted</Badge>}
          {p.banned && <Badge tone="danger">Banned</Badge>}
        </div>
      </td>
      <td className="px-4 py-2 text-muted">{p.online ? "Online now" : p.lastSeen ? formatRelative(p.lastSeen) : "—"}</td>
      <td className="px-4 py-2 text-muted">{p.totalPlayMs ? formatDuration(p.totalPlayMs) : "—"}</td>
      <td className="px-2 py-1 text-right">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon-sm" aria-label={`Actions for ${p.name}`} disabled={!canEdit}>
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent>
            {p.opLevel == null ? (
              <DropdownMenuItem onSelect={() => void run({ action: "op", name: p.name })}>
                <Crown /> Make operator
              </DropdownMenuItem>
            ) : (
              <DropdownMenuItem onSelect={() => void run({ action: "deop", name: p.name })}>
                <Crown /> Remove operator
              </DropdownMenuItem>
            )}
            {p.whitelisted ? (
              <DropdownMenuItem onSelect={() => void run({ action: "whitelist_remove", name: p.name })}>
                <ShieldOff /> Remove from whitelist
              </DropdownMenuItem>
            ) : (
              <DropdownMenuItem onSelect={() => void run({ action: "whitelist_add", name: p.name })}>
                <ShieldCheck /> Add to whitelist
              </DropdownMenuItem>
            )}
            <DropdownMenuSeparator />
            <DropdownMenuItem disabled={!p.online || !data.live} onSelect={onKick}>
              <DoorOpen /> Kick…
            </DropdownMenuItem>
            {p.banned ? (
              <DropdownMenuItem onSelect={() => void run({ action: "pardon", name: p.name })}>
                <Ban /> Unban
              </DropdownMenuItem>
            ) : (
              <DropdownMenuItem destructive onSelect={onBan}>
                <Ban /> Ban…
              </DropdownMenuItem>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </td>
    </tr>
  );
}

function SimpleList({
  rows,
  empty,
  onRemove,
  removeLabel,
  disabled,
}: {
  rows: { key: string; main: ReactNode; sub?: ReactNode }[];
  empty: string;
  onRemove: (key: string) => void;
  removeLabel: string;
  disabled: boolean;
}) {
  if (rows.length === 0) return <p className="p-4 text-xs text-muted">{empty}</p>;
  return (
    <ul>
      {rows.map((r) => (
        <li key={r.key} className="flex items-center justify-between gap-3 border-b border-border px-4 py-2 last:border-0 hover:bg-surface-2">
          <div className="min-w-0">
            <p className="truncate text-[13px] text-fg">{r.main}</p>
            {r.sub && <p className="truncate text-xs text-muted">{r.sub}</p>}
          </div>
          <Button size="sm" variant="ghost" disabled={disabled} onClick={() => onRemove(r.key)}>
            {removeLabel}
          </Button>
        </li>
      ))}
    </ul>
  );
}

export function ServerPlayers() {
  const id = useServerId();
  const { data, isLoading, error } = usePlayers(id);
  const { run, busy } = useAction(id);
  const [tab, setTab] = useState<Tab>("players");
  const [dialog, setDialog] = useState<{ kind: "kick" | "ban"; name: string } | { kind: "ban_ip" } | { kind: "ban_name" } | null>(null);

  if (isLoading)
    return (
      <PageBody className="max-w-5xl">
        <Card>
          <SkeletonRows rows={4} />
        </Card>
      </PageBody>
    );
  if (!data)
    return (
      <EmptyState tone="danger" icon={<AlertTriangle />} title="Players are unavailable" description={error ? errorMessage(error) : undefined} />
    );
  const ro = !!data.readOnlyReason;
  const tabs: [Tab, string, number][] = [
    ["players", "Players", data.known.length],
    ["whitelist", "Whitelist", data.whitelist.length],
    ["operators", "Operators", data.operators.length],
    ["bans", "Bans", data.bans.length + data.ipBans.length],
  ];

  return (
    <PageBody className="max-w-5xl space-y-4">
      <Card>
        <CardHeader
          title={
            data.onlineKnown
              ? `${data.online.length}${data.maxPlayers != null ? ` / ${data.maxPlayers}` : ""} online`
              : data.readOnlyReason
                ? "Online players unknown"
                : "The server is not running"
          }
          description={
            data.readOnlyReason ??
            (data.live
              ? "Changes are sent to the running server as console commands."
              : "The server is stopped: MCPanel edits its player lists directly.")
          }
          actions={
            <label className="flex items-center gap-2 text-xs text-muted">
              Whitelist {data.whitelistEnabled ? "on" : "off"}
              <Switch
                checked={data.whitelistEnabled}
                disabled={ro || busy}
                onCheckedChange={(enabled) => void run({ action: "set_whitelist", enabled })}
                aria-label="Whitelist"
              />
            </label>
          }
        />
        {data.online.length > 0 && (
          <div className="flex flex-wrap gap-1.5 px-4 py-3">
            {data.online.map((n) => (
              <Badge key={n} tone="success">
                {n}
              </Badge>
            ))}
          </div>
        )}
      </Card>

      {data.whitelistEnabled && data.whitelist.length === 0 && (
        <Banner tone="warning" title="The whitelist is on and empty — nobody can join">
          Add players to the whitelist or turn it off. Minecraft 26.x turns the whitelist on for new servers.
        </Banner>
      )}
      {!data.onlineMode && (
        <Banner tone="warning" title="Offline mode">
          Player names are not verified, so anyone can join with any name — including an operator's. Use the whitelist, or enable online mode in
          Properties.
        </Banner>
      )}

      <Card>
        <div className="flex gap-1 border-b border-border px-3 pt-2">
          {tabs.map(([t, label, n]) => (
            <button
              key={t}
              type="button"
              onClick={() => setTab(t)}
              className={cn(
                "-mb-px border-b-2 px-3 py-2 text-[13px] transition-colors duration-150",
                tab === t ? "border-accent font-medium text-fg" : "border-transparent text-muted hover:text-fg",
              )}
            >
              {label} <span className="text-faint">{n}</span>
            </button>
          ))}
        </div>

        <div key={tab} className="animate-fade-in">
          {tab === "players" &&
            (data.known.length === 0 ? (
              <EmptyState icon={<Users />} title="No players yet" description="Players who join, and players on the server's lists, appear here." />
            ) : (
              <table className="w-full text-[13px]">
                <thead className="border-b border-border text-left text-xs text-muted">
                  <tr>
                    <th className="px-4 py-2 font-medium">Player</th>
                    <th className="px-4 py-2 font-medium">Last seen</th>
                    <th className="px-4 py-2 font-medium">Play time</th>
                    <th className="w-10">
                      <span className="sr-only">Actions</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {data.known.map((p) => (
                    <PlayerRow
                      key={p.name}
                      p={p}
                      data={data}
                      run={run}
                      onKick={() => setDialog({ kind: "kick", name: p.name })}
                      onBan={() => setDialog({ kind: "ban", name: p.name })}
                    />
                  ))}
                </tbody>
              </table>
            ))}

          {tab === "whitelist" && (
            <>
              <NameForm label="Add" placeholder="Player name" disabled={ro || busy} onSubmit={(name) => run({ action: "whitelist_add", name })} />
              <SimpleList
                rows={data.whitelist.map((w) => ({ key: w.name, main: w.name, sub: w.uuid }))}
                empty="Nobody is on the whitelist."
                removeLabel="Remove"
                disabled={ro || busy}
                onRemove={(name) => void run({ action: "whitelist_remove", name })}
              />
            </>
          )}

          {tab === "operators" && (
            <>
              <NameForm label="Make operator" placeholder="Player name" disabled={ro || busy} onSubmit={(name) => run({ action: "op", name })} />
              <SimpleList
                rows={data.operators.map((o) => ({
                  key: o.name,
                  main: o.name,
                  sub: `Level ${o.level}${o.bypassesPlayerLimit ? " · bypasses player limit" : ""}`,
                }))}
                empty="There are no operators."
                removeLabel="Remove"
                disabled={ro || busy}
                onRemove={(name) => void run({ action: "deop", name })}
              />
            </>
          )}

          {tab === "bans" && (
            <>
              <div className="flex gap-2 border-b border-border p-3">
                <Button size="sm" variant="danger-outline" disabled={ro} onClick={() => setDialog({ kind: "ban_name" })}>
                  <Ban /> Ban player…
                </Button>
                <Button size="sm" variant="danger-outline" disabled={ro} onClick={() => setDialog({ kind: "ban_ip" })}>
                  <Ban /> Ban IP address…
                </Button>
              </div>
              <SimpleList
                rows={[...data.bans, ...data.ipBans].map((b) => ({
                  key: b.target,
                  main: b.target,
                  sub: [b.reason, b.source && `by ${b.source}`, b.expires ? `until ${b.expires}` : "permanent"].filter(Boolean).join(" · "),
                }))}
                empty="Nobody is banned."
                removeLabel="Unban"
                disabled={ro || busy}
                onRemove={(target) => {
                  const isIp = data.ipBans.some((b) => b.target === target);
                  void run(isIp ? { action: "pardon_ip", ip: target } : { action: "pardon", name: target });
                }}
              />
            </>
          )}
        </div>
      </Card>

      {dialog?.kind === "kick" && (
        <ReasonDialog
          title={`Kick ${dialog.name}?`}
          description="The player is disconnected and can join again."
          confirm="Kick"
          onClose={() => setDialog(null)}
          onSubmit={(_, reason) => run({ action: "kick", name: dialog.name, reason })}
        />
      )}
      {dialog?.kind === "ban" && (
        <ReasonDialog
          title={`Ban ${dialog.name}?`}
          description="The player is disconnected and cannot join until unbanned."
          confirm="Ban"
          onClose={() => setDialog(null)}
          onSubmit={(_, reason) => run({ action: "ban", name: dialog.name, reason })}
        />
      )}
      {dialog?.kind === "ban_name" && (
        <ReasonDialog
          title="Ban a player"
          description="The player cannot join until unbanned."
          confirm="Ban"
          withIp={false}
          onClose={() => setDialog(null)}
          onSubmit={(name, reason) => run({ action: "ban", name, reason })}
        />
      )}
      {dialog?.kind === "ban_ip" && (
        <ReasonDialog
          title="Ban an IP address"
          description="Every player connecting from this address is refused."
          confirm="Ban"
          withIp
          onClose={() => setDialog(null)}
          onSubmit={(ip, reason) => run({ action: "ban_ip", ip, reason })}
        />
      )}
    </PageBody>
  );
}
