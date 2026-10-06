import { useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, Bot, Check, ChevronDown, ChevronRight, Copy, Loader2, RotateCcw, Send, Sparkles, User, Wrench, XCircle } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { PageBody, PageHeader } from "@/app/app-shell";
import { AiConfigCard } from "@/components/ai-config-card";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/overlays";
import { Badge } from "@/components/ui/primitives";
import type { AiChatMessageDto } from "@/bindings/AiChatMessageDto";
import type { AiPendingConfirmationDto } from "@/bindings/AiPendingConfirmationDto";
import type { AiToolExecutionDto } from "@/bindings/AiToolExecutionDto";
import { api } from "@/lib/api";
import { qk, useAiConfig, useServers } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";

function getTimestamp(): number {
  return Date.now();
}

const STARTER_PROMPTS = [
  "List all servers and their current status",
  "Check server.properties on my server",
  "Search for the LuckPerms plugin on Modrinth",
  "Create a backup of my server",
  "Show the last 50 lines of console logs",
  "Start the server",
];

function FormattedContent({ content }: { content: string }) {
  const [copiedIndex, setCopiedIndex] = useState<number | null>(null);

  // Split into code blocks and normal text blocks
  const parts = content.split(/(```[\s\S]*?```)/g);

  const copyCode = (text: string, index: number) => {
    navigator.clipboard.writeText(text).catch(() => {});
    setCopiedIndex(index);
    setTimeout(() => setCopiedIndex(null), 2000);
  };

  return (
    <div className="space-y-2 text-[13px] leading-relaxed break-words">
      {parts.map((part, i) => {
        if (part.startsWith("```") && part.endsWith("```")) {
          const lines = part.slice(3, -3);
          const firstLineBreak = lines.indexOf("\n");
          let lang = "";
          let code = lines;
          if (firstLineBreak !== -1) {
            lang = lines.slice(0, firstLineBreak).trim();
            code = lines.slice(firstLineBreak + 1);
          }
          return (
            <div key={i} className="my-2 overflow-hidden rounded-md border border-border bg-surface-2">
              <div className="flex items-center justify-between border-b border-border bg-surface-3/50 px-3 py-1 text-[11px] text-muted">
                <span className="font-mono">{lang || "code"}</span>
                <button type="button" onClick={() => copyCode(code, i)} className="flex items-center gap-1 transition-colors hover:text-fg">
                  {copiedIndex === i ? <Check className="size-3 text-accent" /> : <Copy className="size-3" />}
                  <span>{copiedIndex === i ? "Copied" : "Copy"}</span>
                </button>
              </div>
              <pre className="overflow-x-auto p-3 font-mono text-xs text-fg">
                <code>{code}</code>
              </pre>
            </div>
          );
        }

        // Render plain text with paragraphs and inline code
        const paragraphs = part.split("\n\n");
        return (
          <div key={i} className="space-y-1.5">
            {paragraphs.map((p, pIdx) => {
              if (!p.trim()) return null;
              const lines = p.split("\n");
              const isBulletList = lines.some((l) => l.trim().startsWith("* ") || l.trim().startsWith("- "));
              if (isBulletList) {
                return (
                  <ul key={pIdx} className="list-disc space-y-1 pl-5">
                    {lines
                      .filter((l) => l.trim().startsWith("* ") || l.trim().startsWith("- "))
                      .map((l, itIdx) => (
                        <li key={itIdx}>
                          <InlineFormatted text={l.trim().slice(2)} />
                        </li>
                      ))}
                  </ul>
                );
              }
              return (
                <p key={pIdx}>
                  <InlineFormatted text={p} />
                </p>
              );
            })}
          </div>
        );
      })}
    </div>
  );
}

function InlineFormatted({ text }: { text: string }) {
  const parts = text.split(/(`[^`]+`|\*\*[^*]+\*\*)/g);
  return (
    <>
      {parts.map((part, i) => {
        if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
          return (
            <code key={i} className="rounded bg-surface-3 px-1 py-0.5 font-mono text-[12px] text-accent">
              {part.slice(1, -1)}
            </code>
          );
        }
        if (part.startsWith("**") && part.endsWith("**") && part.length > 4) {
          return (
            <strong key={i} className="font-semibold text-fg">
              {part.slice(2, -2)}
            </strong>
          );
        }
        return part;
      })}
    </>
  );
}

function ToolExecutionsView({ executions }: { executions: AiToolExecutionDto[] }) {
  const [open, setOpen] = useState(false);
  if (!executions || executions.length === 0) return null;

  return (
    <div className="mt-2 rounded-md border border-border bg-surface-2/60 text-xs">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="flex w-full items-center justify-between px-3 py-1.5 text-left text-muted transition-colors hover:text-fg"
      >
        <span className="flex items-center gap-1.5 font-medium">
          <Wrench className="size-3.5 text-accent" />
          <span>
            {executions.length} tool {executions.length === 1 ? "action" : "actions"} executed autonomously
          </span>
        </span>
        {open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
      </button>

      {open && (
        <div className="space-y-2 divide-y divide-border border-t border-border px-3 py-2">
          {executions.map((e, idx) => (
            <div key={idx} className="pt-1.5 first:pt-0">
              <div className="flex items-center justify-between gap-2">
                <span className="font-mono font-medium text-fg">{e.description}</span>
                {e.error ? <Badge tone="danger">Failed</Badge> : <Badge tone="success">Success</Badge>}
              </div>
              {e.result && (
                <p className="mt-1 max-h-24 overflow-y-auto font-mono text-[11px] whitespace-pre-wrap text-muted">
                  {e.result.length > 240 ? `${e.result.slice(0, 240)}…` : e.result}
                </p>
              )}
              {e.error && <p className="mt-1 text-[11px] text-danger">{e.error}</p>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function PendingConfirmationCard({
  confirmation,
  onApprove,
  onDecline,
  busy,
}: {
  confirmation: AiPendingConfirmationDto;
  onApprove: () => void;
  onDecline: () => void;
  busy: boolean;
}) {
  return (
    <div className="mt-3 rounded-lg border-2 border-warning/60 bg-warning/5 p-4 text-[13px] shadow-sm">
      <div className="flex items-start gap-3">
        <AlertTriangle className="mt-0.5 size-5 shrink-0 text-warning" />
        <div className="min-w-0 flex-1 space-y-2">
          <div>
            <div className="flex items-center gap-2">
              <span className="font-semibold text-fg">{confirmation.title}</span>
              <Badge tone="warning">Action Requires Confirmation</Badge>
            </div>
            <p className="mt-1 text-xs text-muted">{confirmation.description}</p>
          </div>

          <div className="overflow-x-auto rounded border border-border bg-surface-2 p-2.5 font-mono text-[11px] text-fg">
            <p className="mb-1 font-sans text-[10px] font-medium text-faint uppercase">Parameters</p>
            <pre className="whitespace-pre-wrap">{JSON.stringify(confirmation.params, null, 2)}</pre>
          </div>

          <div className="flex items-center gap-2 pt-1">
            <Button variant="primary" size="sm" onClick={onApprove} disabled={busy} className="bg-accent text-accent-fg hover:bg-accent/90">
              {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Check className="size-3.5" />}
              Approve & Execute
            </Button>
            <Button variant="outline" size="sm" onClick={onDecline} disabled={busy} className="text-danger hover:bg-danger/10 hover:text-danger">
              <XCircle className="size-3.5" /> Decline
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

export function AiPage() {
  const qc = useQueryClient();
  const { data: config } = useAiConfig();
  const { data: servers } = useServers();

  const [selectedServerId, setSelectedServerId] = useState<string>("all");
  const [messages, setMessages] = useState<AiChatMessageDto[]>(() => {
    try {
      const saved = sessionStorage.getItem("mcpanel_ai_messages");
      if (saved) return JSON.parse(saved);
    } catch {
      // ignore
    }
    return [];
  });
  const [input, setInput] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);

  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    try {
      sessionStorage.setItem("mcpanel_ai_messages", JSON.stringify(messages));
    } catch {
      // ignore
    }
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages, submitting]);

  const handleSend = async (textToSend?: string) => {
    const text = (textToSend ?? input).trim();
    if (!text || submitting) return;

    if (!config?.configured) {
      toast.error("Please configure an AI API key below first.");
      return;
    }

    const userMessage: AiChatMessageDto = {
      id: crypto.randomUUID(),
      role: "user",
      content: text,
      timestamp: getTimestamp(),
      pendingConfirmation: null,
      toolExecutions: null,
    };

    const nextMessages = [...messages, userMessage];
    setMessages(nextMessages);
    setInput("");
    setSubmitting(true);

    try {
      const serverId = selectedServerId === "all" ? null : selectedServerId;
      const res = await api.ai.chat({
        messages: nextMessages,
        serverId,
      });

      setMessages([...nextMessages, res.message]);
      // Invalidate relevant queries in case actions ran
      void qc.invalidateQueries({ queryKey: qk.servers });
      if (serverId) {
        void qc.invalidateQueries({ queryKey: qk.server(serverId) });
        void qc.invalidateQueries({ queryKey: qk.audit(serverId) });
      }
      void qc.invalidateQueries({ queryKey: qk.audit(null) });
    } catch (e) {
      toast.error(errorMessage(e));
      // Add error message as system/assistant notification
      setMessages([
        ...nextMessages,
        {
          id: crypto.randomUUID(),
          role: "assistant",
          content: `⚠️ Error executing request: ${errorMessage(e)}`,
          timestamp: getTimestamp(),
          pendingConfirmation: null,
          toolExecutions: null,
        },
      ]);
    } finally {
      setSubmitting(false);
    }
  };

  const handleConfirm = async (confirmationId: string, approved: boolean) => {
    setConfirmingId(confirmationId);
    try {
      const serverId = selectedServerId === "all" ? null : selectedServerId;
      const res = await api.ai.confirmAction({
        confirmationId,
        approved,
        messages,
        serverId,
      });

      // Update the messages list: clear the pending confirmation from previous message and append the new response
      const updated = messages.map((m) => {
        if (m.pendingConfirmation?.confirmationId === confirmationId) {
          return { ...m, pendingConfirmation: null };
        }
        return m;
      });

      setMessages([...updated, res.message]);
      void qc.invalidateQueries({ queryKey: qk.servers });
      void qc.invalidateQueries({ queryKey: qk.audit(null) });
      if (serverId) {
        void qc.invalidateQueries({ queryKey: qk.server(serverId) });
        void qc.invalidateQueries({ queryKey: qk.audit(serverId) });
      }
      toast.success(approved ? "Action approved and executed" : "Action cancelled");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setConfirmingId(null);
    }
  };

  const handleClearHistory = () => {
    setMessages([]);
    try {
      sessionStorage.removeItem("mcpanel_ai_messages");
    } catch {
      // ignore
    }
    toast.success("Conversation cleared");
  };

  const serverOptions = [
    { value: "all", label: "All Servers (Global Context)" },
    ...(servers ?? []).map((s) => ({
      value: s.id,
      label: `${s.name} (${s.software.softwareName} ${s.software.gameVersion})`,
    })),
  ];

  return (
    <>
      <PageHeader
        title={
          <div className="flex items-center gap-2">
            <Sparkles className="size-5 text-accent" />
            <span>AI Assistant</span>
          </div>
        }
        description="Ask MCPanel's AI assistant to manage servers, update properties, install plugins, create backups, or inspect logs."
        actions={
          <div className="flex items-center gap-2">
            <Select
              aria-label="Target server context"
              className="w-56"
              value={selectedServerId}
              onValueChange={setSelectedServerId}
              options={serverOptions}
            />
            {messages.length > 0 && (
              <Button variant="ghost" size="sm" onClick={handleClearHistory} title="Clear conversation">
                <RotateCcw className="size-3.5" /> Clear
              </Button>
            )}
          </div>
        }
      />

      <PageBody className="mx-auto flex h-[calc(100vh-140px)] w-full max-w-5xl flex-col gap-4 p-4">
        {!config?.configured && (
          <div className="mb-2">
            <AiConfigCard />
          </div>
        )}

        {/* Chat Messages Log */}
        <div ref={scrollRef} className="flex-1 space-y-4 overflow-y-auto rounded-xl border border-border bg-surface p-4 shadow-sm">
          {messages.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center space-y-4 p-8 text-center">
              <div className="flex size-14 items-center justify-center rounded-2xl border border-accent/20 bg-accent-soft text-accent">
                <Sparkles className="size-7" />
              </div>
              <div className="max-w-md space-y-1">
                <h3 className="text-base font-semibold text-fg">How can I help with your servers?</h3>
                <p className="text-xs text-muted">
                  I can autonomously edit files, update configs, search plugins, and monitor servers. Major actions like turning on/off servers and
                  deleting files will ask for your confirmation.
                </p>
              </div>

              <div className="grid w-full max-w-xl gap-2 pt-2 text-left sm:grid-cols-2">
                {STARTER_PROMPTS.map((prompt) => (
                  <button
                    key={prompt}
                    type="button"
                    onClick={() => void handleSend(prompt)}
                    className="flex items-center gap-2 rounded-lg border border-border bg-surface-2 p-2.5 text-left text-xs text-muted transition-all hover:border-accent/50 hover:bg-surface-3 hover:text-fg"
                  >
                    <Sparkles className="size-3.5 shrink-0 text-accent" />
                    <span className="truncate">{prompt}</span>
                  </button>
                ))}
              </div>
            </div>
          ) : (
            messages.map((m) => {
              const isUser = m.role === "user";
              return (
                <div key={m.id} className={cn("flex gap-3", isUser ? "justify-end" : "justify-start")}>
                  {!isUser && (
                    <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg border border-accent/20 bg-accent-soft text-accent">
                      <Bot className="size-4" />
                    </div>
                  )}

                  <div
                    className={cn(
                      "max-w-[85%] rounded-xl px-4 py-3 shadow-xs",
                      isUser ? "ml-12 bg-accent text-accent-fg" : "mr-12 border border-border bg-surface-2 text-fg",
                    )}
                  >
                    <FormattedContent content={m.content} />

                    {m.toolExecutions && m.toolExecutions.length > 0 && <ToolExecutionsView executions={m.toolExecutions} />}

                    {m.pendingConfirmation && (
                      <PendingConfirmationCard
                        confirmation={m.pendingConfirmation}
                        busy={confirmingId === m.pendingConfirmation.confirmationId}
                        onApprove={() => void handleConfirm(m.pendingConfirmation!.confirmationId, true)}
                        onDecline={() => void handleConfirm(m.pendingConfirmation!.confirmationId, false)}
                      />
                    )}
                  </div>

                  {isUser && (
                    <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent">
                      <User className="size-4" />
                    </div>
                  )}
                </div>
              );
            })
          )}

          {submitting && (
            <div className="flex animate-pulse items-center justify-start gap-3 text-xs text-muted">
              <div className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-accent/20 bg-accent-soft text-accent">
                <Bot className="size-4" />
              </div>
              <div className="flex items-center gap-2 rounded-xl border border-border bg-surface-2 px-4 py-3">
                <Loader2 className="size-3.5 animate-spin text-accent" />
                <span>Thinking and managing server tasks…</span>
              </div>
            </div>
          )}
        </div>

        {/* Input Bar */}
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void handleSend();
          }}
          className="flex items-center gap-2"
        >
          <div className="relative flex-1">
            <input
              type="text"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder={
                config?.configured
                  ? selectedServerId !== "all"
                    ? `Ask anything about this server… (e.g. 'update motd', 'check logs', 'install essentials')`
                    : "Ask anything about your servers or tell the AI what to do…"
                  : "Configure AI Assistant above to start"
              }
              disabled={submitting || !config?.configured}
              className="h-11 w-full rounded-xl border border-border-strong bg-surface px-4 pr-12 text-[13px] text-fg shadow-xs placeholder:text-faint focus:border-accent/60 focus:ring-2 focus:ring-ring/30 focus:outline-none disabled:opacity-50"
            />
          </div>

          <Button
            type="submit"
            variant="primary"
            size="md"
            disabled={!input.trim() || submitting || !config?.configured}
            className="h-11 rounded-xl px-5 shadow-xs"
          >
            {submitting ? <Loader2 className="size-4 animate-spin" /> : <Send className="size-4" />}
            <span>Send</span>
          </Button>
        </form>
      </PageBody>
    </>
  );
}
