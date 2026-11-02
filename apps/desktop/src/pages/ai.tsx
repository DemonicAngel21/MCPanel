import { useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  ArrowDown,
  Bot,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  Key,
  Loader2,
  RotateCcw,
  Send,
  Server,
  Sparkles,
  StopCircle,
  User,
  Wrench,
  XCircle,
} from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { toast } from "sonner";
import { PageHeader } from "@/app/app-shell";
import { AiProvidersView } from "@/components/ai-providers-view";
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

function formatTime(ts: number): string {
  try {
    return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  } catch {
    return "";
  }
}

const STARTER_PROMPTS = [
  {
    category: "Server Operations",
    prompts: ["List all servers and their current status", "Start the server and monitor startup logs", "Create a full backup of this server"],
  },
  {
    category: "Configuration & Tuning",
    prompts: [
      "Inspect server.properties for port & max players",
      "Check server JVM flags and memory allocations",
      "Search for the LuckPerms plugin on Modrinth",
    ],
  },
  {
    category: "Troubleshooting",
    prompts: ["Show the last 50 lines of console logs", "Analyze the latest crash report or server errors", "Check disk space usage across servers"],
  },
];

function InlineFormatted({ text, isUser }: { text: string; isUser?: boolean }) {
  const parts = text.split(/(`[^`]+`|\*\*[^*]+\*\*)/g);
  return (
    <>
      {parts.map((part, i) => {
        if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
          return (
            <code
              key={i}
              className={cn("rounded px-1.5 py-0.5 font-mono text-[12px]", isUser ? "bg-black/20 text-white" : "bg-surface-3 text-accent")}
            >
              {part.slice(1, -1)}
            </code>
          );
        }
        if (part.startsWith("**") && part.endsWith("**") && part.length > 4) {
          return (
            <strong key={i} className={cn("font-semibold", isUser ? "text-white" : "text-fg")}>
              {part.slice(2, -2)}
            </strong>
          );
        }
        return part;
      })}
    </>
  );
}

function FormattedContent({ content, isUser }: { content: string; isUser?: boolean }) {
  const [copiedIndex, setCopiedIndex] = useState<number | null>(null);

  const parts = content.split(/(```[\s\S]*?```)/g);

  const copyCode = (text: string, index: number) => {
    navigator.clipboard.writeText(text).catch(() => {});
    setCopiedIndex(index);
    setTimeout(() => setCopiedIndex(null), 2000);
  };

  return (
    <div className={cn("space-y-2.5 text-[13px] leading-relaxed break-words", isUser && "text-white")}>
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
            <div key={i} className="my-2 overflow-hidden rounded-lg border border-border bg-surface-2 text-fg shadow-xs">
              <div className="flex items-center justify-between border-b border-border bg-surface-3/60 px-3 py-1.5 text-[11px] text-muted">
                <span className="font-mono font-medium">{lang || "code"}</span>
                <button
                  type="button"
                  onClick={() => copyCode(code, i)}
                  className="flex items-center gap-1.5 text-muted transition-colors hover:text-fg"
                >
                  {copiedIndex === i ? <Check className="size-3 text-accent" /> : <Copy className="size-3" />}
                  <span>{copiedIndex === i ? "Copied" : "Copy"}</span>
                </button>
              </div>
              <pre className="overflow-x-auto p-3 font-mono text-xs leading-relaxed text-fg">
                <code>{code}</code>
              </pre>
            </div>
          );
        }

        const paragraphs = part.split("\n\n");
        return (
          <div key={i} className="space-y-2">
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
                          <InlineFormatted text={l.trim().slice(2)} isUser={isUser} />
                        </li>
                      ))}
                  </ul>
                );
              }
              return (
                <p key={pIdx} className="leading-relaxed">
                  <InlineFormatted text={p} isUser={isUser} />
                </p>
              );
            })}
          </div>
        );
      })}
    </div>
  );
}

function ToolExecutionsView({ executions }: { executions: AiToolExecutionDto[] }) {
  const [open, setOpen] = useState(false);
  if (!executions || executions.length === 0) return null;

  const failureCount = executions.filter((e) => Boolean(e.error)).length;

  return (
    <div className="mt-2.5 rounded-lg border border-border bg-surface-2/80 text-xs shadow-xs">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="flex w-full items-center justify-between px-3 py-2 text-left text-muted transition-colors hover:text-fg"
      >
        <span className="flex items-center gap-2 font-medium">
          <Wrench className="size-3.5 text-accent" />
          <span>
            {executions.length} autonomous {executions.length === 1 ? "action" : "actions"} executed
          </span>
          {failureCount > 0 ? (
            <Badge tone="danger" className="px-1.5 py-0 text-[10px]">
              {failureCount} failed
            </Badge>
          ) : (
            <Badge tone="success" className="px-1.5 py-0 text-[10px]">
              All succeeded
            </Badge>
          )}
        </span>
        {open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
      </button>

      {open && (
        <div className="space-y-2 divide-y divide-border border-t border-border px-3 py-2.5">
          {executions.map((e, idx) => (
            <div key={idx} className="pt-2 first:pt-0">
              <div className="flex items-center justify-between gap-2">
                <span className="font-mono font-medium text-fg">{e.description}</span>
                {e.error ? <Badge tone="danger">Failed</Badge> : <Badge tone="success">Success</Badge>}
              </div>
              {e.result && (
                <p className="mt-1.5 max-h-32 overflow-y-auto rounded bg-surface-3/50 p-2 font-mono text-[11px] whitespace-pre-wrap text-muted">
                  {e.result.length > 300 ? `${e.result.slice(0, 300)}…` : e.result}
                </p>
              )}
              {e.error && <p className="mt-1 text-[11px] font-medium text-danger">{e.error}</p>}
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
    <div className="mt-3 rounded-xl border-2 border-warning/60 bg-warning/5 p-4 text-[13px] shadow-sm">
      <div className="flex items-start gap-3">
        <AlertTriangle className="mt-0.5 size-5 shrink-0 text-warning" />
        <div className="min-w-0 flex-1 space-y-2.5">
          <div>
            <div className="flex items-center gap-2">
              <span className="font-semibold text-fg">{confirmation.title}</span>
              <Badge tone="warning">Action Requires Confirmation</Badge>
            </div>
            <p className="mt-1 text-xs text-muted">{confirmation.description}</p>
          </div>

          <div className="overflow-x-auto rounded-lg border border-border bg-surface-2 p-2.5 font-mono text-[11px] text-fg">
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
  const [activeTab, setActiveTab] = useState<"chat" | "providers">("chat");
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
  const [copiedMessageId, setCopiedMessageId] = useState<string | null>(null);
  const [isAtBottom, setIsAtBottom] = useState(true);

  const scrollRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Auto-resize textarea as content changes
  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
      textareaRef.current.style.height = `${Math.min(textareaRef.current.scrollHeight, 160)}px`;
    }
  }, [input]);

  // Persist messages in sessionStorage
  useEffect(() => {
    try {
      sessionStorage.setItem("mcpanel_ai_messages", JSON.stringify(messages));
    } catch {
      // ignore
    }
    if (isAtBottom && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages, submitting, isAtBottom]);

  const handleScroll = () => {
    if (!scrollRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollRef.current;
    const atBottom = scrollHeight - scrollTop - clientHeight < 40;
    setIsAtBottom(atBottom);
  };

  const scrollToBottom = () => {
    if (scrollRef.current) {
      scrollRef.current.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
      setIsAtBottom(true);
    }
  };

  const copyMessage = (id: string, text: string) => {
    navigator.clipboard.writeText(text).catch(() => {});
    setCopiedMessageId(id);
    toast.success("Message copied to clipboard");
    setTimeout(() => setCopiedMessageId(null), 2000);
  };

  const handleSend = async (textToSend?: string) => {
    const text = (textToSend ?? input).trim();
    if (!text || submitting) return;

    if (!config?.configured) {
      toast.error("Please configure an AI provider first.");
      setActiveTab("providers");
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
    setIsAtBottom(true);

    try {
      const serverId = selectedServerId === "all" ? null : selectedServerId;
      const res = await api.ai.chat({
        messages: nextMessages,
        serverId,
      });

      setMessages([...nextMessages, res.message]);
      void qc.invalidateQueries({ queryKey: qk.servers });
      if (serverId) {
        void qc.invalidateQueries({ queryKey: qk.server(serverId) });
        void qc.invalidateQueries({ queryKey: qk.audit(serverId) });
      }
      void qc.invalidateQueries({ queryKey: qk.audit(null) });
    } catch (e) {
      toast.error(errorMessage(e));
      setMessages([
        ...nextMessages,
        {
          id: crypto.randomUUID(),
          role: "assistant",
          content: `Request failed: ${errorMessage(e)}`,
          timestamp: getTimestamp(),
          pendingConfirmation: null,
          toolExecutions: null,
        },
      ]);
    } finally {
      setSubmitting(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleSend();
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

  const selectedServer = (servers ?? []).find((s) => s.id === selectedServerId);

  const serverOptions = [
    { value: "all", label: "All Servers (Global Context)" },
    ...(servers ?? []).map((s) => ({
      value: s.id,
      label: `${s.name} (${s.software.softwareName} ${s.software.gameVersion})`,
    })),
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden bg-background">
      <PageHeader
        title={
          <div className="flex flex-wrap items-center gap-4">
            <div className="flex items-center gap-2">
              <Sparkles className="size-5 text-accent" />
              <span>AI Assistant</span>
            </div>
            <div className="flex items-center rounded-lg border border-border bg-surface-2 p-0.5 text-xs">
              <button
                type="button"
                onClick={() => setActiveTab("chat")}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-3 py-1 font-medium transition-colors",
                  activeTab === "chat" ? "bg-surface text-fg shadow-xs" : "text-muted hover:text-fg",
                )}
              >
                <Bot className="size-3.5 text-accent" />
                <span>Assistant Chat</span>
              </button>
              <button
                type="button"
                onClick={() => setActiveTab("providers")}
                className={cn(
                  "flex items-center gap-1.5 rounded-md px-3 py-1 font-medium transition-colors",
                  activeTab === "providers" ? "bg-surface text-fg shadow-xs" : "text-muted hover:text-fg",
                )}
              >
                <Key className="size-3.5 text-accent" />
                <span>API Providers & Keys</span>
                {(config?.configuredProviders?.length ?? 0) > 0 && (
                  <Badge tone="success" className="ml-1 px-1.5 py-0 text-[10px]">
                    {config?.configuredProviders?.length}
                  </Badge>
                )}
              </button>
            </div>
          </div>
        }
        description={
          activeTab === "chat"
            ? "Autonomous server management, log diagnosis, plugin installation, and file editing."
            : "Manage individual API keys, models, and endpoints for all supported AI providers."
        }
        actions={
          activeTab === "chat" ? (
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
          ) : (
            <Button variant="outline" size="sm" onClick={() => setActiveTab("chat")} className="h-8 gap-1.5 text-xs">
              <Bot className="size-3.5 text-accent" />
              <span>Back to Chat</span>
            </Button>
          )
        }
      />

      {activeTab === "providers" ? (
        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-6 sm:px-6">
          <div className="mx-auto max-w-4xl">
            <AiProvidersView />
          </div>
        </div>
      ) : (
        <div className="mx-auto flex min-h-0 w-full max-w-4xl flex-1 flex-col px-4 pt-3 pb-4 sm:px-6">
          {/* Status Sub-bar */}
          <div className="mb-3 flex items-center justify-between rounded-xl border border-border bg-surface-2/60 px-3.5 py-2 text-xs text-muted shadow-xs">
            <div className="flex items-center gap-2">
              <span className="flex items-center gap-1.5 font-medium text-fg">
                <Server className="size-3.5 text-accent" />
                {selectedServer ? selectedServer.name : "All Servers"}
              </span>
              <span className="text-border">|</span>
              <span className="font-mono text-[11px]">
                {config?.configured ? (
                  <>
                    <span className="capitalize">{config.provider}</span>: {config.model}
                  </>
                ) : (
                  "No provider active"
                )}
              </span>
            </div>
            <button
              type="button"
              onClick={() => setActiveTab("providers")}
              className="flex items-center gap-1 text-[11px] font-medium text-accent hover:underline"
            >
              <Key className="size-3" />
              <span>Switch Model or Keys</span>
            </button>
          </div>

          {!config?.configured && (
            <div className="mb-3 flex items-center justify-between rounded-xl border border-warning/40 bg-warning/10 p-3.5 shadow-xs">
              <div className="flex items-center gap-3">
                <div className="flex size-9 items-center justify-center rounded-lg bg-warning-soft text-warning">
                  <AlertTriangle className="size-4" />
                </div>
                <div>
                  <h4 className="text-xs font-semibold text-fg">No AI Provider Configured Yet</h4>
                  <p className="text-[11px] text-muted">Set up Google Gemini, OpenAI, Claude, DeepSeek, or run Ollama locally to start chatting.</p>
                </div>
              </div>
              <Button variant="primary" size="sm" onClick={() => setActiveTab("providers")} className="h-8 gap-1.5 text-xs">
                <Key className="size-3.5" />
                <span>Configure Providers</span>
              </Button>
            </div>
          )}

          {/* Chat Messages Log */}
          <div
            ref={scrollRef}
            onScroll={handleScroll}
            className="relative min-h-0 flex-1 space-y-4 overflow-y-auto rounded-2xl border border-border bg-surface p-4 shadow-xs"
          >
            {messages.length === 0 ? (
              <div className="flex h-full flex-col items-center justify-center space-y-5 p-6 text-center">
                <div className="flex size-14 items-center justify-center rounded-2xl border border-accent/20 bg-accent-soft text-accent">
                  <Sparkles className="size-7" />
                </div>
                <div className="max-w-md space-y-1.5">
                  <h3 className="text-base font-semibold text-fg">How can I help with your servers?</h3>
                  <p className="text-xs text-muted">
                    I can autonomously edit files, update configs, search plugins, and monitor server status. Major actions like restarting servers
                    and deleting files will ask for your confirmation.
                  </p>
                </div>

                <div className="grid w-full max-w-2xl gap-3 pt-2 text-left sm:grid-cols-3">
                  {STARTER_PROMPTS.map((group) => (
                    <div key={group.category} className="space-y-2">
                      <span className="text-[11px] font-semibold text-faint uppercase">{group.category}</span>
                      <div className="space-y-1.5">
                        {group.prompts.map((prompt) => (
                          <button
                            key={prompt}
                            type="button"
                            onClick={() => void handleSend(prompt)}
                            className="flex w-full items-start gap-2 rounded-lg border border-border bg-surface-2 p-2.5 text-left text-xs text-muted transition-all hover:border-accent/40 hover:bg-surface-3 hover:text-fg"
                          >
                            <Sparkles className="mt-0.5 size-3 shrink-0 text-accent" />
                            <span className="line-clamp-2">{prompt}</span>
                          </button>
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            ) : (
              messages.map((m) => {
                const isUser = m.role === "user";
                return (
                  <div key={m.id} className={cn("group flex gap-3", isUser ? "justify-end" : "justify-start")}>
                    {!isUser && (
                      <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-xl border border-accent/20 bg-accent-soft text-accent">
                        <Bot className="size-4" />
                      </div>
                    )}

                    <div className="flex max-w-[85%] flex-col space-y-1">
                      <div
                        className={cn(
                          "relative rounded-2xl px-4 py-3 shadow-xs",
                          isUser ? "bg-accent text-accent-fg" : "border border-border bg-surface-2 text-fg",
                        )}
                      >
                        <FormattedContent content={m.content} isUser={isUser} />

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

                      {/* Message footer & actions */}
                      <div
                        className={cn(
                          "flex items-center gap-2 px-1 text-[10px] text-muted opacity-0 transition-opacity group-hover:opacity-100",
                          isUser ? "justify-end" : "justify-start",
                        )}
                      >
                        <span>{formatTime(m.timestamp)}</span>
                        <span>•</span>
                        <button type="button" onClick={() => copyMessage(m.id, m.content)} className="flex items-center gap-1 hover:text-fg">
                          {copiedMessageId === m.id ? (
                            <>
                              <Check className="size-2.5 text-accent" />
                              <span>Copied</span>
                            </>
                          ) : (
                            <>
                              <Copy className="size-2.5" />
                              <span>Copy</span>
                            </>
                          )}
                        </button>
                      </div>
                    </div>

                    {isUser && (
                      <div className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-xl bg-accent-soft text-accent">
                        <User className="size-4" />
                      </div>
                    )}
                  </div>
                );
              })
            )}

            {submitting && (
              <div className="flex animate-pulse items-center justify-start gap-3 text-xs text-muted">
                <div className="flex size-8 shrink-0 items-center justify-center rounded-xl border border-accent/20 bg-accent-soft text-accent">
                  <Bot className="size-4" />
                </div>
                <div className="flex items-center gap-2 rounded-2xl border border-border bg-surface-2 px-4 py-3">
                  <Loader2 className="size-3.5 animate-spin text-accent" />
                  <span>Thinking and executing server tasks…</span>
                </div>
              </div>
            )}

            {/* Scroll to bottom button */}
            {!isAtBottom && (
              <button
                type="button"
                onClick={scrollToBottom}
                className="absolute right-6 bottom-4 flex size-8 items-center justify-center rounded-full border border-border bg-surface shadow-md transition-all hover:bg-surface-2 hover:text-fg"
                title="Scroll to bottom"
              >
                <ArrowDown className="size-4 text-accent" />
              </button>
            )}
          </div>

          {/* Auto-growing Textarea Input Bar */}
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void handleSend();
            }}
            className="mt-3 flex flex-col gap-2 rounded-2xl border border-border bg-surface p-2.5 shadow-xs focus-within:border-accent/60 focus-within:ring-2 focus-within:ring-ring/20"
          >
            <textarea
              ref={textareaRef}
              rows={1}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={
                config?.configured
                  ? selectedServer
                    ? `Ask about ${selectedServer.name} or type instructions… (Shift+Enter for newline)`
                    : "Ask anything about your servers or tell the AI what to do… (Shift+Enter for newline)"
                  : "Configure an AI Provider above to start chatting…"
              }
              disabled={submitting || !config?.configured}
              className="w-full resize-none bg-transparent px-2 py-1 text-[13px] text-fg placeholder:text-faint focus:outline-none disabled:opacity-50"
            />

            <div className="flex items-center justify-between border-t border-border/50 pt-2 text-xs text-muted">
              <span className="text-[11px] text-faint">
                Press <kbd className="rounded bg-surface-2 px-1 py-0.5 font-mono text-[10px]">Enter</kbd> to send,{" "}
                <kbd className="rounded bg-surface-2 px-1 py-0.5 font-mono text-[10px]">Shift+Enter</kbd> for new line
              </span>

              <div className="flex items-center gap-2">
                {submitting && (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => setSubmitting(false)}
                    className="h-8 gap-1 text-xs text-danger hover:bg-danger/10 hover:text-danger"
                  >
                    <StopCircle className="size-3.5" />
                    <span>Stop</span>
                  </Button>
                )}

                <Button
                  type="submit"
                  variant="primary"
                  size="sm"
                  disabled={!input.trim() || submitting || !config?.configured}
                  className="h-8 gap-1.5 rounded-lg px-4 shadow-xs"
                >
                  {submitting ? <Loader2 className="size-3.5 animate-spin" /> : <Send className="size-3.5" />}
                  <span>Send</span>
                </Button>
              </div>
            </div>
          </form>
        </div>
      )}
    </div>
  );
}
