import { useQueryClient } from "@tanstack/react-query";
import {
  Bot,
  CheckCircle2,
  Cpu,
  ExternalLink,
  Eye,
  EyeOff,
  Flame,
  Globe,
  Key,
  Layers,
  Loader2,
  Search,
  Server,
  Sparkles,
  Trash2,
  Zap,
} from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/overlays";
import { Badge, Card, Field, Input, Label, Switch } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { qk, useAccount, useAiConfig } from "@/lib/queries";
import { cn, errorMessage } from "@/lib/utils";
import type { AiConfigDto } from "@/bindings/AiConfigDto";

interface ProviderDefinition {
  id: string;
  name: string;
  description: string;
  icon: typeof Sparkles;
  defaultModel: string;
  isLocal: boolean;
  defaultBaseUrl: string;
  keyPlaceholder: string;
  keyUrl?: string;
  keyUrlLabel?: string;
  models: { value: string; label: string }[];
}

const PROVIDER_DEFINITIONS: ProviderDefinition[] = [
  {
    id: "gemini",
    name: "Google Gemini",
    description: "Fast, powerful multimodal models with generous free tier directly from Google AI Studio.",
    icon: Sparkles,
    defaultModel: "gemini-3.8-flash",
    isLocal: false,
    defaultBaseUrl: "",
    keyPlaceholder: "Paste your Gemini API key (AIza...)",
    keyUrl: "https://aistudio.google.com/app/apikey",
    keyUrlLabel: "Get Gemini key",
    models: [
      { value: "gemini-3.8-flash", label: "Gemini 3.8 Flash (Fast & Smart)" },
      { value: "gemini-2.5-flash", label: "Gemini 2.5 Flash" },
      { value: "gemini-2.5-pro", label: "Gemini 2.5 Pro" },
    ],
  },
  {
    id: "openai",
    name: "OpenAI",
    description: "Industry-standard GPT models including GPT-4o and lightweight GPT-4o-mini.",
    icon: Bot,
    defaultModel: "gpt-4o-mini",
    isLocal: false,
    defaultBaseUrl: "",
    keyPlaceholder: "sk-...",
    keyUrl: "https://platform.openai.com/api-keys",
    keyUrlLabel: "Get OpenAI key",
    models: [
      { value: "gpt-4o-mini", label: "GPT-4o Mini (Fast & Cheap)" },
      { value: "gpt-4o", label: "GPT-4o (High Intelligence)" },
      { value: "o3-mini", label: "o3-mini (Reasoning)" },
    ],
  },
  {
    id: "anthropic",
    name: "Anthropic Claude",
    description: "Thoughtful, high-steerability Claude models renowned for coding and instruction following.",
    icon: Layers,
    defaultModel: "claude-3-7-sonnet-20250219",
    isLocal: false,
    defaultBaseUrl: "",
    keyPlaceholder: "sk-ant-...",
    keyUrl: "https://console.anthropic.com/settings/keys",
    keyUrlLabel: "Get Claude key",
    models: [
      { value: "claude-3-7-sonnet-20250219", label: "Claude 3.7 Sonnet (Latest)" },
      { value: "claude-3-5-sonnet-20241022", label: "Claude 3.5 Sonnet" },
      { value: "claude-3-5-haiku-20241022", label: "Claude 3.5 Haiku (Fast)" },
    ],
  },
  {
    id: "deepseek",
    name: "DeepSeek",
    description: "High-capability, cost-effective reasoning and coding models hosted by DeepSeek.",
    icon: Cpu,
    defaultModel: "deepseek-chat",
    isLocal: false,
    defaultBaseUrl: "https://api.deepseek.com",
    keyPlaceholder: "sk-...",
    keyUrl: "https://platform.deepseek.com/api_keys",
    keyUrlLabel: "Get DeepSeek key",
    models: [
      { value: "deepseek-chat", label: "DeepSeek Chat (V3)" },
      { value: "deepseek-reasoner", label: "DeepSeek Reasoner (R1)" },
    ],
  },
  {
    id: "groq",
    name: "Groq (Ultra-Fast LPU)",
    description: "Ultra-low-latency inference for open-weight models powered by Groq LPU hardware.",
    icon: Zap,
    defaultModel: "llama-3.3-70b-versatile",
    isLocal: false,
    defaultBaseUrl: "https://api.groq.com/openai/v1",
    keyPlaceholder: "gsk_...",
    keyUrl: "https://console.groq.com/keys",
    keyUrlLabel: "Get Groq key",
    models: [
      { value: "llama-3.3-70b-versatile", label: "Llama 3.3 70B Versatile" },
      { value: "llama-3.1-8b-instant", label: "Llama 3.1 8B Instant (Ultra-fast)" },
      { value: "mixtral-8x7b-32768", label: "Mixtral 8x7B" },
    ],
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    description: "Unified router giving access to hundreds of AI models from multiple providers with a single key.",
    icon: Globe,
    defaultModel: "openai/gpt-4o-mini",
    isLocal: false,
    defaultBaseUrl: "https://openrouter.ai/api/v1",
    keyPlaceholder: "sk-or-...",
    keyUrl: "https://openrouter.ai/keys",
    keyUrlLabel: "Get OpenRouter key",
    models: [
      { value: "openai/gpt-4o-mini", label: "OpenAI: GPT-4o-mini" },
      { value: "anthropic/claude-3.5-sonnet", label: "Anthropic: Claude 3.5 Sonnet" },
      { value: "deepseek/deepseek-r1", label: "DeepSeek: R1" },
      { value: "meta-llama/llama-3.3-70b-instruct", label: "Meta: Llama 3.3 70B" },
    ],
  },
  {
    id: "mistral",
    name: "Mistral AI",
    description: "European frontier models designed for high performance and efficiency.",
    icon: Flame,
    defaultModel: "mistral-small-latest",
    isLocal: false,
    defaultBaseUrl: "https://api.mistral.ai/v1",
    keyPlaceholder: "Paste your Mistral API key",
    keyUrl: "https://console.mistral.ai/api-keys",
    keyUrlLabel: "Get Mistral key",
    models: [
      { value: "mistral-small-latest", label: "Mistral Small (Fast)" },
      { value: "mistral-large-latest", label: "Mistral Large" },
      { value: "codestral-latest", label: "Codestral (Coding)" },
    ],
  },
  {
    id: "ollama",
    name: "Ollama (Local / Offline)",
    description: "Run open models (Llama, Mistral, Qwen) 100% locally on your own GPU/CPU with no internet required.",
    icon: Server,
    defaultModel: "llama3.2",
    isLocal: true,
    defaultBaseUrl: "http://localhost:11434/v1",
    keyPlaceholder: "Optional for Ollama (leave blank for local)",
    keyUrl: "https://ollama.com",
    keyUrlLabel: "Download Ollama",
    models: [
      { value: "llama3.2", label: "Llama 3.2 (3B - Fast & Lightweight)" },
      { value: "llama3.3", label: "Llama 3.3 (70B)" },
      { value: "qwen2.5-coder", label: "Qwen 2.5 Coder (Coding)" },
      { value: "mistral", label: "Mistral 7B" },
      { value: "phi3", label: "Phi-3 Mini" },
    ],
  },
  {
    id: "lmstudio",
    name: "LM Studio (Local / Offline)",
    description: "Connect to LM Studio local server running on your computer. Fully private and offline.",
    icon: Server,
    defaultModel: "local-model",
    isLocal: true,
    defaultBaseUrl: "http://localhost:1234/v1",
    keyPlaceholder: "Optional for LM Studio (leave blank)",
    keyUrl: "https://lmstudio.ai",
    keyUrlLabel: "Download LM Studio",
    models: [{ value: "local-model", label: "Currently Loaded Model in LM Studio" }],
  },
  {
    id: "custom",
    name: "Custom / OpenAI-Compatible",
    description: "Connect to any custom proxy, vLLM, LocalAI, or custom OpenAI-compatible endpoint.",
    icon: Cpu,
    defaultModel: "gpt-4o-mini",
    isLocal: false,
    defaultBaseUrl: "",
    keyPlaceholder: "API key (optional if endpoint doesn't require auth)",
    models: [{ value: "gpt-4o-mini", label: "Default: gpt-4o-mini" }],
  },
];

function IndividualProviderCard({
  def,
  config,
  onActivate,
}: {
  def: ProviderDefinition;
  config: AiConfigDto | undefined;
  onActivate: (providerId: string, model: string, baseUrl: string) => Promise<void>;
}) {
  const qc = useQueryClient();
  const isActive = config?.provider === def.id;
  const isConfigured = Boolean(
    config?.useSharedKey
      ? config.configured && !def.isLocal
      : config?.configuredProviders?.includes(def.id),
  );

  const initialModel = isActive && config?.model ? config.model : def.defaultModel;
  const initialBaseUrl = isActive && config?.baseUrl !== undefined ? config.baseUrl || "" : def.defaultBaseUrl;

  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState(initialModel);
  const [customModel, setCustomModel] = useState("");
  const [isCustomModel, setIsCustomModel] = useState(
    !def.models.some((m) => m.value === initialModel),
  );
  const [baseUrl, setBaseUrl] = useState(initialBaseUrl);
  const [showKey, setShowKey] = useState(false);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);

  const effectiveModel = isCustomModel ? customModel.trim() || def.defaultModel : model;

  const handleSave = async (makeActive = false) => {
    setSaving(true);
    try {
      await api.ai.saveConfig({
        provider: makeActive || isActive ? def.id : null,
        model: makeActive || isActive ? effectiveModel : null,
        baseUrl: baseUrl.trim() ? baseUrl.trim() : null,
        apiKey: apiKey.trim() ? apiKey.trim() : null,
        useSharedKey: config?.useSharedKey ?? false,
        syncApiKeys: config?.syncApiKeys ?? true,
      });
      await qc.invalidateQueries({ queryKey: qk.aiConfig });
      setApiKey("");
      toast.success(`${def.name} settings saved successfully!`);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  const handleTest = async () => {
    setTesting(true);
    try {
      await api.ai.testProvider(
        def.id,
        effectiveModel,
        baseUrl.trim() || null,
        apiKey.trim() || null,
      );
      toast.success(`${def.name} connection test passed! Model responded.`);
    } catch (e) {
      toast.error(`Connection failed: ${errorMessage(e)}`);
    } finally {
      setTesting(false);
    }
  };

  const handleClearKey = async () => {
    if (!confirm(`Are you sure you want to remove the API key for ${def.name}?`)) {
      return;
    }
    setSaving(true);
    try {
      await api.ai.saveConfig({
        provider: def.id,
        apiKey: "",
      });
      await qc.invalidateQueries({ queryKey: qk.aiConfig });
      setApiKey("");
      toast.success(`API key for ${def.name} removed.`);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  const modelOptions = [
    ...def.models,
    { value: "custom", label: "Custom model identifier…" },
  ];

  const Icon = def.icon;

  return (
    <Card className={cn("transition-all", isActive && "border-accent ring-1 ring-accent/30")}>
      <div className="space-y-4 p-5">
        {/* Header */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex items-center gap-3">
            <div
              className={cn(
                "flex size-10 shrink-0 items-center justify-center rounded-xl border border-border",
                isActive ? "bg-accent-soft text-accent border-accent/30" : "bg-surface-2 text-muted",
              )}
            >
              <Icon className="size-5" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h3 className="font-semibold text-fg text-sm">{def.name}</h3>
                {isActive && (
                  <Badge tone="success" className="text-[10px] py-0 px-1.5">
                    Active Assistant
                  </Badge>
                )}
                {isConfigured && !def.isLocal && (
                  <Badge tone="success" className="text-[10px] py-0 px-1.5 gap-1">
                    <CheckCircle2 className="size-2.5" /> Configured
                  </Badge>
                )}
                {def.isLocal && (
                  <Badge tone="neutral" className="text-[10px] py-0 px-1.5">
                    Local / Offline
                  </Badge>
                )}
              </div>
              <p className="text-xs text-muted mt-0.5 line-clamp-2">{def.description}</p>
            </div>
          </div>

          <div className="flex items-center gap-2 shrink-0">
            {!isActive && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => onActivate(def.id, effectiveModel, baseUrl)}
                className="text-xs h-8"
              >
                Set Active
              </Button>
            )}
          </div>
        </div>

        {/* Dedicated API Key Input Box */}
        <Field
          label={
            <div className="flex items-center justify-between text-xs">
              <span className="font-medium text-fg">
                {def.isLocal ? "API Key (Optional for local server)" : `${def.name} API Key`}
              </span>
              {def.keyUrl && (
                <a
                  href={def.keyUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="flex items-center gap-1 text-[11px] text-accent hover:underline"
                >
                  <span>{def.keyUrlLabel || "Get API key"}</span>
                  <ExternalLink className="size-2.5" />
                </a>
              )}
            </div>
          }
          hint={
            isConfigured && !def.isLocal
              ? "Key safely saved in OS credential store. Enter a new key to overwrite it."
              : def.isLocal
                ? "Local model running on your machine. Leave blank unless using an authenticated endpoint."
                : undefined
          }
        >
          <div className="relative">
            <Input
              type={showKey ? "text" : "password"}
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={
                isConfigured && !def.isLocal
                  ? "•••••••••••••••••••••••••••••••• (Key saved; enter new key to replace)"
                  : def.keyPlaceholder
              }
              className="pr-10 font-mono text-xs"
            />
            <button
              type="button"
              onClick={() => setShowKey(!showKey)}
              className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted hover:text-fg"
              aria-label={showKey ? "Hide key" : "Show key"}
            >
              {showKey ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
            </button>
          </div>
        </Field>

        {/* Model Selection & Custom Model */}
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Model" className="text-xs">
            <Select
              aria-label="Model"
              value={isCustomModel ? "custom" : model}
              onValueChange={(val) => {
                if (val === "custom") {
                  setIsCustomModel(true);
                } else {
                  setIsCustomModel(false);
                  setModel(val);
                }
              }}
              options={modelOptions}
            />
          </Field>

          {isCustomModel ? (
            <Field label="Custom Model ID" className="text-xs">
              <Input
                value={customModel}
                onChange={(e) => setCustomModel(e.target.value)}
                placeholder={`e.g. ${def.defaultModel}`}
                className="text-xs font-mono"
              />
            </Field>
          ) : (
            <Field label="Endpoint Base URL" className="text-xs" hint="Leave blank for standard default">
              <Input
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder={def.defaultBaseUrl || "Default provider endpoint"}
                className="text-xs font-mono"
              />
            </Field>
          )}
        </div>

        {isCustomModel && (
          <Field label="Endpoint Base URL" className="text-xs" hint="Leave blank for standard default">
            <Input
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder={def.defaultBaseUrl || "Default provider endpoint"}
              className="text-xs font-mono"
            />
          </Field>
        )}

        {/* Action Row */}
        <div className="flex items-center justify-between pt-1 border-t border-border">
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={handleTest}
              disabled={testing || saving || (!isConfigured && !apiKey.trim() && !def.isLocal)}
              className="text-xs h-8 gap-1.5"
            >
              {testing ? <Loader2 className="size-3.5 animate-spin" /> : <Zap className="size-3.5" />}
              <span>Test Connection</span>
            </Button>

            <Button
              variant="primary"
              size="sm"
              onClick={() => handleSave(false)}
              disabled={saving || testing}
              className="text-xs h-8 gap-1.5"
            >
              {saving ? <Loader2 className="size-3.5 animate-spin" /> : <Key className="size-3.5" />}
              <span>Save Settings</span>
            </Button>
          </div>

          {isConfigured && !def.isLocal && (
            <Button
              variant="ghost"
              size="sm"
              onClick={handleClearKey}
              className="text-xs h-8 text-danger hover:text-danger hover:bg-danger-soft gap-1"
            >
              <Trash2 className="size-3.5" />
              <span>Remove Key</span>
            </Button>
          )}
        </div>
      </div>
    </Card>
  );
}

export function AiProvidersView() {
  const qc = useQueryClient();
  const { data: config } = useAiConfig();
  const { data: account } = useAccount();

  const [filter, setFilter] = useState("");
  const [useSharedKey, setUseSharedKey] = useState(config?.useSharedKey ?? false);
  const [syncApiKeys, setSyncApiKeys] = useState(config?.syncApiKeys ?? true);

  const activeProvider = config?.provider || "gemini";

  const handleSharedKeyToggle = async (checked: boolean) => {
    setUseSharedKey(checked);
    try {
      await api.ai.saveConfig({ useSharedKey: checked });
      await qc.invalidateQueries({ queryKey: qk.aiConfig });
      toast.success(
        checked
          ? "Switched to shared API key across all providers"
          : "Switched to separate API keys per provider",
      );
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const handleSyncToggle = async (checked: boolean) => {
    setSyncApiKeys(checked);
    try {
      await api.ai.saveConfig({ syncApiKeys: checked });
      await qc.invalidateQueries({ queryKey: qk.aiConfig });
      toast.success(
        checked
          ? "API keys now sync with your MCPanel profile"
          : "API key syncing disabled (stored locally only)",
      );
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const handleActivate = async (providerId: string, model: string, baseUrl: string) => {
    try {
      await api.ai.saveConfig({
        provider: providerId,
        model,
        baseUrl: baseUrl.trim() ? baseUrl.trim() : null,
      });
      await qc.invalidateQueries({ queryKey: qk.aiConfig });
      toast.success(`Active AI provider switched to ${providerId}`);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const filteredProviders = PROVIDER_DEFINITIONS.filter(
    (p) =>
      p.name.toLowerCase().includes(filter.toLowerCase()) ||
      p.description.toLowerCase().includes(filter.toLowerCase()) ||
      p.id.toLowerCase().includes(filter.toLowerCase()),
  );

  const configuredCount = config?.configuredProviders?.length ?? 0;

  return (
    <div className="space-y-6 pb-8">
      {/* Top Banner and Preferences */}
      <div className="rounded-xl border border-border bg-surface p-5 space-y-4 shadow-xs">
        <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
          <div>
            <h2 className="text-base font-semibold text-fg flex items-center gap-2">
              <Key className="size-4 text-accent" />
              <span>All AI Providers & API Keys</span>
            </h2>
            <p className="text-xs text-muted mt-1">
              Configure each AI provider with its own distinct API key, model selection, and endpoint. You can switch the active assistant anytime.
            </p>
          </div>

          <div className="flex items-center gap-2">
            <Badge tone="neutral" className="gap-1.5 py-1 px-2.5 text-xs">
              <Bot className="size-3.5 text-accent" />
              <span>
                Active: <strong className="text-fg uppercase">{activeProvider}</strong>
              </span>
            </Badge>
            <Badge tone="success" className="gap-1.5 py-1 px-2.5 text-xs">
              <CheckCircle2 className="size-3.5" />
              <span>{configuredCount} configured</span>
            </Badge>
          </div>
        </div>

        {/* Global Toggles */}
        <div className="grid gap-3 pt-2 sm:grid-cols-2 border-t border-border">
          <div className="flex items-start justify-between gap-3 rounded-lg border border-border bg-surface-2 p-3">
            <div className="space-y-0.5">
              <Label htmlFor="shared-key-global" className="cursor-pointer text-xs font-medium text-fg">
                Use same shared key for all providers
              </Label>
              <p className="text-[11px] text-faint">
                When enabled, one single key applies across all providers instead of per-provider keys.
              </p>
            </div>
            <Switch
              id="shared-key-global"
              checked={useSharedKey}
              onCheckedChange={handleSharedKeyToggle}
            />
          </div>

          <div className="flex items-start justify-between gap-3 rounded-lg border border-border bg-surface-2 p-3">
            <div className="space-y-0.5">
              <Label htmlFor="sync-key-global" className="cursor-pointer text-xs font-medium text-fg">
                Sync keys with account profile
              </Label>
              <p className="text-[11px] text-faint">
                {account?.signedIn
                  ? `Linked to ${account.profile?.displayName || account.profile?.email || "account"}`
                  : "Turn off to disable account syncing and store keys strictly local to this device."}
              </p>
            </div>
            <Switch
              id="sync-key-global"
              checked={syncApiKeys}
              onCheckedChange={handleSyncToggle}
            />
          </div>
        </div>
      </div>

      {/* Filter search bar */}
      <div className="flex items-center justify-between gap-3">
        <div className="relative flex-1 max-w-sm">
          <Search className="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-faint" />
          <Input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Search provider (Gemini, Claude, DeepSeek, Ollama...)"
            className="pl-9 text-xs"
          />
        </div>
        <span className="text-xs text-muted">
          Showing {filteredProviders.length} of {PROVIDER_DEFINITIONS.length} providers
        </span>
      </div>

      {/* Provider Cards Grid */}
      <div className="grid gap-4 md:grid-cols-2">
        {filteredProviders.map((def) => (
          <IndividualProviderCard
            key={def.id}
            def={def}
            config={config}
            onActivate={handleActivate}
          />
        ))}
      </div>
    </div>
  );
}
