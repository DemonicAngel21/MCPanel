import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  Coffee,
  Download,
  ExternalLink,
  FastForward,
  FilePlus2,
  Globe,
  Link2,
  Monitor,
  Moon,
  Palette,
  Rocket,
  Server,
  Sparkles,
  Sun,
  UserRound,
} from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import type { SettingsPatchDto } from "@/bindings/SettingsPatchDto";
import { AccentPicker } from "@/components/accent-picker";
import { AccountForms, AccountSummary } from "@/components/account-panel";
import { AiConfigCard } from "@/components/ai-config-card";
import { Button } from "@/components/ui/button";
import { Badge, Card, Spinner } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { qk, useAccount, useJava, usePlayitAgent, useSettings } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";

const openPlayitDownload = () => api.app.openExternal("https://playit.gg/download").catch((e) => toast.error(errorMessage(e)));

const STEPS = [
  { id: "welcome", label: "Welcome", icon: <Rocket /> },
  { id: "account", label: "Account", icon: <UserRound /> },
  { id: "appearance", label: "Appearance", icon: <Palette /> },
  { id: "java", label: "Java", icon: <Coffee /> },
  { id: "ai", label: "AI Assistant", icon: <Sparkles /> },
  { id: "internet", label: "Internet access", icon: <Globe /> },
  { id: "done", label: "Ready", icon: <Check /> },
] as const;

function useSaveSettings() {
  const qc = useQueryClient();
  return async (patch: SettingsPatchDto) => {
    try {
      const s = await api.settings.update(patch);
      qc.setQueryData(qk.settings, s);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };
}

function StepTitle({ title, description }: { title: string; description: string }) {
  return (
    <div className="space-y-1.5">
      <h1 className="text-2xl font-semibold text-fg">{title}</h1>
      <p className="max-w-2xl text-sm text-muted">{description}</p>
    </div>
  );
}

function Welcome({ onSkip }: { onSkip?: () => void }) {
  const features = [
    { icon: <Server />, title: "Servers in minutes", text: "Paper, Purpur, Fabric, Forge and vanilla, with the right Java picked for you." },
    { icon: <Globe />, title: "Play with friends anywhere", text: "playit.gg tunnels without touching your router." },
    { icon: <Coffee />, title: "Safe by default", text: "Backups, crash recovery and clear explanations when something goes wrong." },
  ];
  return (
    <div className="space-y-8">
      <div className="flex items-center gap-4">
        <img src="/logo.svg" alt="" className="size-14" />
        <StepTitle title="Welcome to MCPanel" description="A few quick steps set MCPanel up for you. You can change everything later in Settings." />
      </div>
      <div className="grid gap-4 lg:grid-cols-3">
        {features.map((f) => (
          <Card key={f.title} className="setup-feature-card space-y-2 p-5">
            <span className="flex size-9 items-center justify-center rounded-lg bg-accent-soft text-accent [&_svg]:size-5">{f.icon}</span>
            <p className="text-sm font-medium text-fg">{f.title}</p>
            <p className="text-xs text-muted">{f.text}</p>
          </Card>
        ))}
      </div>
      {onSkip && (
        <div className="flex items-center justify-between rounded-lg border border-border bg-surface-2 p-4 text-xs text-muted">
          <span>Already know your way around? You can skip this setup wizard anytime and go straight to the dashboard.</span>
          <Button variant="outline" size="sm" onClick={onSkip}>
            <FastForward className="size-3.5" /> Skip setup now
          </Button>
        </div>
      )}
    </div>
  );
}

function AccountStep({ next }: { next: () => void }) {
  const { data: account } = useAccount();
  return (
    <div className="grid gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,420px)]">
      <StepTitle
        title="Your MCPanel account"
        description="Create an account with your email or Google, or sign in. An account is optional; MCPanel and your servers work without one."
      />
      <Card className="p-5">
        {account?.configured && (account.signedIn || account.isGuest) ? <AccountSummary /> : <AccountForms onSignedIn={next} />}
      </Card>
    </div>
  );
}

function AppearanceStep() {
  const { data: settings } = useSettings();
  const save = useSaveSettings();
  const themes = [
    { id: "system", label: "System", icon: <Monitor /> },
    { id: "dark", label: "Dark", icon: <Moon /> },
    { id: "light", label: "Light", icon: <Sun /> },
  ];
  return (
    <div className="space-y-6">
      <StepTitle
        title="Make it yours"
        description="Pick a theme and an accent color. Green is MCPanel's own; you can also match your Windows accent."
      />
      <div role="radiogroup" aria-label="Theme" className="grid max-w-3xl gap-3 sm:grid-cols-3">
        {themes.map((t) => (
          <button
            key={t.id}
            role="radio"
            aria-checked={settings?.theme === t.id}
            onClick={() => void save({ theme: t.id })}
            className={cn(
              "flex cursor-default items-center gap-3 rounded-lg border p-4 text-left text-sm transition-colors duration-150 [&_svg]:size-5",
              settings?.theme === t.id ? "border-accent bg-accent-soft text-fg" : "border-border text-muted hover:border-border-strong hover:text-fg",
            )}
          >
            {t.icon} {t.label}
          </button>
        ))}
      </div>
      <Card className="max-w-3xl">
        <AccentPicker value={settings?.accent ?? "green"} onChange={(accent) => void save({ accent })} />
      </Card>
    </div>
  );
}

function JavaStep() {
  const qc = useQueryClient();
  const { data: java, isLoading } = useJava();
  const [busy, setBusy] = useState(false);

  const detect = async () => {
    setBusy(true);
    try {
      const list = await api.java.detect();
      qc.setQueryData(qk.java, list);
      toast.success(`${list.length} Java runtime${list.length === 1 ? "" : "s"} found`);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const addManually = async () => {
    try {
      const grant = await api.dialog.pickJava();
      if (!grant) return;
      const rt = await api.java.add(grant.token);
      await qc.invalidateQueries({ queryKey: qk.java });
      toast.success(`Added Java ${rt.major} (${rt.version})`);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const valid = (java ?? []).filter((j) => j.valid);
  return (
    <div className="space-y-6">
      <StepTitle
        title="Java"
        description="Minecraft servers run on Java. MCPanel detects installed runtimes automatically, or you can select your own java.exe directly."
      />
      <Card className="max-w-3xl">
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border px-4 py-3">
          <p className="text-sm text-fg">{isLoading ? "Looking…" : `${valid.length} usable runtime${valid.length === 1 ? "" : "s"}`}</p>
          <div className="flex items-center gap-2">
            <Button size="sm" variant="outline" onClick={() => void addManually()} disabled={busy}>
              <FilePlus2 className="size-3.5" /> Choose Java manually
            </Button>
            <Button size="sm" variant="primary" onClick={() => void detect()} disabled={busy}>
              {busy ? <Spinner className="text-accent-fg" /> : <Coffee className="size-3.5" />} Detect Java
            </Button>
          </div>
        </div>
        {valid.length > 0 ? (
          <ul className="divide-y border-border">
            {valid.map((j) => (
              <li key={j.id} className="flex items-center justify-between gap-3 px-4 py-2.5 text-xs">
                <div className="flex items-center gap-2">
                  <span className="font-medium text-fg">Java {j.major}</span>
                  {j.source === "manual" && (
                    <Badge tone="info" className="text-[10px]">
                      Manual
                    </Badge>
                  )}
                </div>
                <span className="truncate font-mono text-muted">{j.path}</span>
              </li>
            ))}
          </ul>
        ) : (
          <div className="space-y-2 px-4 py-4 text-xs text-muted">
            <p>
              No Java found yet. Current Minecraft versions need Java 21 or newer; install a JDK (for example Eclipse Temurin from adoptium.net), then
              press <strong>Detect Java</strong> — or click <strong>Choose Java manually</strong> to select your java.exe directly.
            </p>
          </div>
        )}
      </Card>
    </div>
  );
}

function InternetStep() {
  const qc = useQueryClient();
  const { data: agent } = usePlayitAgent();
  const [busy, setBusy] = useState(false);
  const [installing, setInstalling] = useState(false);
  const installAgent = async () => {
    setInstalling(true);
    try {
      await api.playit.installAgent();
      await qc.invalidateQueries({ queryKey: qk.playit });
      toast.success("Playit agent installed successfully");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setInstalling(false);
    }
  };
  const link = async () => {
    setBusy(true);
    try {
      await api.playit.link();
      await qc.invalidateQueries({ queryKey: qk.playit });
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="space-y-6">
      <StepTitle
        title="Play with friends over the internet (optional)"
        description="MCPanel can link its own playit.gg agent to your free playit.gg account and create tunnels for your servers, so friends can join without router setup."
      />
      <Card className="max-w-3xl space-y-3 p-5">
        {!agent ? (
          <Spinner />
        ) : !agent.installed ? (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="space-y-1">
              <p className="text-sm font-medium text-fg">Add Playit.gg Agent</p>
              <p className="text-xs text-muted">Install the official Playit agent automatically, or download the installer directly.</p>
            </div>
            <div className="flex items-center gap-2">
              <Button variant="primary" onClick={() => void installAgent()} disabled={installing}>
                {installing ? <Spinner /> : <Download />} Install Playit agent
              </Button>
              <Button variant="outline" size="sm" onClick={() => void openPlayitDownload()}>
                Website <ExternalLink />
              </Button>
            </div>
          </div>
        ) : agent.linked ? (
          <p className="flex items-center gap-2 text-sm text-fg">
            <Badge tone="success">Linked</Badge> MCPanel's playit agent is linked to your account.
          </p>
        ) : agent.linkState === "waiting" ? (
          <p className="flex items-center gap-2 text-xs text-fg">
            <Spinner /> Approve MCPanel's agent on playit.gg in your browser…
          </p>
        ) : (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="text-xs text-muted">playit.gg opens in your browser; sign in or create a free account there and approve the agent.</p>
            <Button variant="primary" onClick={() => void link()} disabled={busy}>
              {busy ? <Spinner className="text-accent-fg" /> : <Link2 />} Link with playit.gg
            </Button>
          </div>
        )}
        {agent?.linkState === "failed" && agent.linkError && <p className="text-xs text-danger">{agent.linkError}</p>}
      </Card>
    </div>
  );
}

function AiStep() {
  return (
    <div className="space-y-6">
      <StepTitle
        title="AI Assistant (optional)"
        description="MCPanel includes an autonomous AI assistant to manage servers, edit configuration files, install plugins, and handle backups. Add an API key now or configure it later in Settings."
      />
      <div className="max-w-3xl">
        <AiConfigCard />
      </div>
    </div>
  );
}

function Done() {
  const { data: account } = useAccount();
  const name = account?.profile?.displayName;
  return (
    <div className="space-y-6">
      <StepTitle
        title={name ? `You're all set, ${name}` : "You're all set"}
        description="Create your first server now, or look around first. Settings holds everything you chose here."
      />
    </div>
  );
}

/** First-time setup: shown until finished or skipped. */
export function OnboardingPage() {
  const navigate = useNavigate();
  const save = useSaveSettings();
  const [step, setStep] = useState(0);
  const id = STEPS[step]?.id;
  const finish = async (to: "/" | "/servers/new") => {
    await save({ onboardingCompleted: true });
    void navigate({ to });
  };
  const next = () => setStep((s) => Math.min(s + 1, STEPS.length - 1));

  return (
    <div className="flex h-full bg-background">
      <aside className="setup-sidebar flex w-64 shrink-0 flex-col border-r border-border bg-surface p-6">
        <p className="mb-6 flex items-center gap-2 text-sm font-semibold text-fg">
          <img src="/logo.svg" alt="" className="size-6" /> MCPanel setup
        </p>
        <ol className="space-y-1">
          {STEPS.map((s, i) => (
            <li key={s.id}>
              <button
                onClick={() => i <= step && setStep(i)}
                aria-current={i === step ? "step" : undefined}
                className={cn(
                  "setup-step flex w-full cursor-default items-center gap-3 rounded-md px-2.5 py-2 text-left text-[13px] transition-colors duration-150 [&_svg]:size-4",
                  i === step ? "bg-accent-soft font-medium text-fg" : i < step ? "text-fg hover:bg-surface-3" : "text-faint",
                )}
              >
                <span
                  className={cn(
                    "flex size-6 items-center justify-center rounded-full",
                    i < step ? "bg-accent text-accent-fg" : i === step ? "text-accent" : "text-faint",
                  )}
                >
                  {i < step ? <Check /> : s.icon}
                </span>
                {s.label}
              </button>
            </li>
          ))}
        </ol>
        <div className="flex-1" />
        <Button
          variant="outline"
          size="sm"
          className="w-full justify-center gap-1.5 text-xs text-muted hover:text-fg"
          onClick={() => void finish("/")}
        >
          <FastForward className="size-3.5" /> Skip setup
        </Button>
      </aside>
      <main className="flex min-w-0 flex-1 flex-col">
        <div className="setup-content min-h-0 flex-1 overflow-y-auto px-10 py-10">
          <div className="mx-auto mb-8 max-w-5xl">
            <div className="mb-2 flex items-center justify-between text-[11px] text-muted">
              <span>{STEPS[step]?.label}</span>
              <span>
                {step + 1} of {STEPS.length}
              </span>
            </div>
            <div
              className="h-1 overflow-hidden rounded-full bg-surface-3"
              role="progressbar"
              aria-label="Setup progress"
              aria-valuemin={1}
              aria-valuemax={STEPS.length}
              aria-valuenow={step + 1}
            >
              <div className="setup-progress h-full rounded-full bg-accent" style={{ width: `${((step + 1) / STEPS.length) * 100}%` }} />
            </div>
          </div>
          <div key={id} className="animate-page-in">
            {id === "welcome" && <Welcome onSkip={() => void finish("/")} />}
            {id === "account" && <AccountStep next={next} />}
            {id === "appearance" && <AppearanceStep />}
            {id === "java" && <JavaStep />}
            {id === "ai" && <AiStep />}
            {id === "internet" && <InternetStep />}
            {id === "done" && <Done />}
          </div>
        </div>
        <footer className="flex items-center justify-between gap-3 border-t border-border bg-surface px-10 py-4">
          <div className="flex items-center gap-2">
            <Button variant="ghost" onClick={() => setStep((s) => Math.max(0, s - 1))} disabled={step === 0}>
              <ArrowLeft /> Back
            </Button>
            {id !== "done" && (
              <Button
                variant="ghost"
                onClick={() => void finish("/")}
                className="text-muted hover:text-fg flex items-center gap-1.5"
              >
                <FastForward className="size-3.5" /> Skip setup
              </Button>
            )}
          </div>
          <div className="flex gap-2">
            {id === "done" ? (
              <>
                <Button variant="outline" onClick={() => void finish("/")}>
                  Go to the dashboard
                </Button>
                <Button variant="primary" onClick={() => void finish("/servers/new")}>
                  <Server /> Create my first server
                </Button>
              </>
            ) : (
              <Button variant="primary" onClick={next}>
                {id === "account" || id === "internet" || id === "ai" ? "Continue" : id === "welcome" ? "Get started" : "Next"} <ArrowRight />
              </Button>
            )}
          </div>
        </footer>
      </main>
    </div>
  );
}
