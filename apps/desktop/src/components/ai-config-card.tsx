import { useQueryClient } from "@tanstack/react-query";
import { Bot, CheckCircle2, Key, Loader2, ShieldCheck, Sparkles, Trash2 } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/overlays";
import { Badge, Card, CardHeader, Field, Input } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { qk, useAiConfig } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";

interface ProviderOption {
  value: string;
  label: string;
  defaultModel: string;
  isLocal: boolean;
  defaultBaseUrl: string;
  keyPlaceholder: string;
  keyHint: string;
}

const DEFAULT_PROVIDER: ProviderOption = {
  value: "gemini",
  label: "Google Gemini",
  defaultModel: "gemini-3.8-flash",
  isLocal: false,
  defaultBaseUrl: "",
  keyPlaceholder: "Paste your Gemini API key",
  keyHint: "Get a Google Gemini API key at https://aistudio.google.com/app/apikey",
};

const PROVIDERS: ProviderOption[] = [
  DEFAULT_PROVIDER,
  {
    value: "openai",
    label: "OpenAI",
    defaultModel: "gpt-4o-mini",
    isLocal: false,
    defaultBaseUrl: "",
    keyPlaceholder: "sk-...",
    keyHint: "Get an OpenAI API key at https://platform.openai.com/api-keys",
  },
  {
    value: "anthropic",
    label: "Anthropic Claude",
    defaultModel: "claude-3-7-sonnet-20250219",
    isLocal: false,
    defaultBaseUrl: "",
    keyPlaceholder: "sk-ant-...",
    keyHint: "Get an Anthropic Claude API key at https://console.anthropic.com/settings/keys",
  },
  {
    value: "deepseek",
    label: "DeepSeek",
    defaultModel: "deepseek-chat",
    isLocal: false,
    defaultBaseUrl: "https://api.deepseek.com",
    keyPlaceholder: "sk-...",
    keyHint: "Get a DeepSeek API key at https://platform.deepseek.com/api_keys",
  },
  {
    value: "groq",
    label: "Groq (Fast)",
    defaultModel: "llama-3.3-70b-versatile",
    isLocal: false,
    defaultBaseUrl: "https://api.groq.com/openai/v1",
    keyPlaceholder: "gsk_...",
    keyHint: "Get a Groq API key at https://console.groq.com/keys",
  },
  {
    value: "openrouter",
    label: "OpenRouter",
    defaultModel: "openai/gpt-4o-mini",
    isLocal: false,
    defaultBaseUrl: "https://openrouter.ai/api/v1",
    keyPlaceholder: "sk-or-...",
    keyHint: "Get an OpenRouter API key at https://openrouter.ai/keys",
  },
  {
    value: "mistral",
    label: "Mistral AI",
    defaultModel: "mistral-small-latest",
    isLocal: false,
    defaultBaseUrl: "https://api.mistral.ai/v1",
    keyPlaceholder: "Paste your Mistral API key",
    keyHint: "Get a Mistral API key at https://console.mistral.ai/api-keys",
  },
  {
    value: "ollama",
    label: "Ollama (Local / Offline)",
    defaultModel: "llama3.2",
    isLocal: true,
    defaultBaseUrl: "http://localhost:11434/v1",
    keyPlaceholder: "Not required for local Ollama (leave blank)",
    keyHint: "Runs locally on your machine via Ollama. No API key required!",
  },
  {
    value: "lmstudio",
    label: "LM Studio (Local / Offline)",
    defaultModel: "local-model",
    isLocal: true,
    defaultBaseUrl: "http://localhost:1234/v1",
    keyPlaceholder: "Not required for LM Studio (leave blank)",
    keyHint: "Runs locally on your machine via LM Studio local server. No API key required!",
  },
  {
    value: "custom",
    label: "Custom / OpenAI-Compatible",
    defaultModel: "gpt-4o-mini",
    isLocal: false,
    defaultBaseUrl: "",
    keyPlaceholder: "API key (leave blank if local / none needed)",
    keyHint: "Connect to any custom OpenAI-compatible endpoint, proxy, or local server.",
  },
];

const PROVIDER_MODELS: Record<string, { value: string; label: string }[]> = {
  gemini: [
    { value: "gemini-3.8-flash", label: "Gemini 3.8 Flash (Fast & Smart)" },
    { value: "gemini-3.7-flash", label: "Gemini 3.7 Flash" },
    { value: "gemini-3.5-flash", label: "Gemini 3.5 Flash" },
  ],
  openai: [
    { value: "gpt-4o-mini", label: "GPT-4o Mini (Fast & Cost-Effective)" },
    { value: "gpt-4o", label: "GPT-4o (High Intelligence)" },
    { value: "o3-mini", label: "o3-mini (Reasoning)" },
    { value: "o1", label: "o1 (Full Reasoning)" },
  ],
  anthropic: [
    { value: "claude-3-7-sonnet-20250219", label: "Claude 3.7 Sonnet (Hybrid Reasoning)" },
    { value: "claude-3-5-sonnet-20241022", label: "Claude 3.5 Sonnet" },
    { value: "claude-3-5-haiku-20241022", label: "Claude 3.5 Haiku (Fast)" },
  ],
  deepseek: [
    { value: "deepseek-chat", label: "DeepSeek V3 (deepseek-chat)" },
    { value: "deepseek-reasoner", label: "DeepSeek R1 (deepseek-reasoner)" },
  ],
  groq: [
    { value: "llama-3.3-70b-versatile", label: "Llama 3.3 70B (Fast Versatile)" },
    { value: "llama-3.1-8b-instant", label: "Llama 3.1 8B (Ultra Fast)" },
    { value: "mixtral-8x7b-32768", label: "Mixtral 8x7B" },
  ],
  openrouter: [
    { value: "openai/gpt-4o-mini", label: "OpenAI GPT-4o Mini" },
    { value: "anthropic/claude-3.5-sonnet", label: "Anthropic Claude 3.5 Sonnet" },
    { value: "deepseek/deepseek-chat", label: "DeepSeek V3" },
    { value: "meta-llama/llama-3.3-70b-instruct", label: "Meta Llama 3.3 70B Instruct" },
  ],
  mistral: [
    { value: "mistral-small-latest", label: "Mistral Small (Fast)" },
    { value: "mistral-large-latest", label: "Mistral Large" },
    { value: "codestral-latest", label: "Codestral" },
  ],
  ollama: [
    { value: "llama3.2", label: "Llama 3.2 (Default)" },
    { value: "llama3.3", label: "Llama 3.3 (70B)" },
    { value: "qwen2.5-coder", label: "Qwen 2.5 Coder" },
    { value: "mistral", label: "Mistral 7B" },
    { value: "phi3", label: "Phi-3 Mini" },
  ],
  lmstudio: [{ value: "local-model", label: "Currently Loaded Model in LM Studio" }],
};

export function AiConfigCard() {
  const qc = useQueryClient();
  const { data: config } = useAiConfig();

  const [provider, setProvider] = useState<string>("gemini");
  const [model, setModel] = useState<string>("gemini-3.8-flash");
  const [apiKey, setApiKey] = useState<string>("");
  const [baseUrl, setBaseUrl] = useState<string>("");
  const [customModel, setCustomModel] = useState<string>("");
  const [isCustomModel, setIsCustomModel] = useState<boolean>(false);

  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);

  // Sync state from query when loaded
  const [synced, setSynced] = useState(false);
  if (config && !synced) {
    const activeProvider = config.provider || "gemini";
    setProvider(activeProvider);
    const activeModel = config.model === "gemini-2.5-flash" || config.model === "models/gemini-2.5-flash" ? "gemini-3.8-flash" : config.model;
    const knownModels = (PROVIDER_MODELS[activeProvider] || []).map((m) => m.value);
    if (knownModels.includes(activeModel)) {
      setModel(activeModel);
      setIsCustomModel(false);
    } else {
      setModel("custom");
      setCustomModel(activeModel || "");
      setIsCustomModel(true);
    }
    setBaseUrl(config.baseUrl || "");
    setSynced(true);
  }

  const currentProvider = PROVIDERS.find((p) => p.value === provider) ?? DEFAULT_PROVIDER;
  const isLocal = currentProvider.isLocal;

  const handleProviderChange = (newProvider: string) => {
    setProvider(newProvider);
    const pConfig = PROVIDERS.find((p) => p.value === newProvider);
    const defaultModel = pConfig?.defaultModel ?? "gemini-3.8-flash";
    setModel(defaultModel);
    setIsCustomModel(false);
    if (pConfig?.defaultBaseUrl) {
      setBaseUrl(pConfig.defaultBaseUrl);
    } else if (newProvider === "gemini" || newProvider === "openai" || newProvider === "anthropic") {
      setBaseUrl("");
    }
  };

  const handleModelChange = (newModel: string) => {
    if (newModel === "custom") {
      setIsCustomModel(true);
      setModel("custom");
    } else {
      setIsCustomModel(false);
      setModel(newModel);
    }
  };

  const effectiveModel = isCustomModel ? customModel.trim() || currentProvider.defaultModel : model;

  const handleSave = async () => {
    setSaving(true);
    try {
      await api.ai.saveConfig({
        provider,
        model: effectiveModel,
        baseUrl: baseUrl.trim() ? baseUrl.trim() : null,
        apiKey: apiKey.trim() ? apiKey.trim() : null,
      });
      await qc.invalidateQueries({ queryKey: qk.aiConfig });
      setApiKey("");
      toast.success("AI Assistant configuration saved");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  const handleTest = async () => {
    setTesting(true);
    try {
      // If user typed a new key or changed provider/model, save first
      const hasKeyChange = apiKey.trim().length > 0;
      const hasSettingChange =
        config && (config.provider !== provider || config.model !== effectiveModel || (config.baseUrl || "") !== baseUrl.trim());

      if (hasKeyChange || hasSettingChange) {
        await api.ai.saveConfig({
          provider,
          model: effectiveModel,
          baseUrl: baseUrl.trim() ? baseUrl.trim() : null,
          apiKey: apiKey.trim() ? apiKey.trim() : null,
        });
        await qc.invalidateQueries({ queryKey: qk.aiConfig });
        setApiKey("");
      }
      await api.ai.testConnection();
      toast.success("Connection verified successfully! AI model responded.");
    } catch (e) {
      toast.error(`Connection failed: ${errorMessage(e)}`);
    } finally {
      setTesting(false);
    }
  };

  const handleClear = async () => {
    if (!confirm("Are you sure you want to remove the AI configuration and disconnect the AI Assistant?")) {
      return;
    }
    setSaving(true);
    try {
      await api.ai.saveConfig({
        provider: null,
        model: null,
        baseUrl: null,
        apiKey: "",
      });
      await qc.invalidateQueries({ queryKey: qk.aiConfig });
      setApiKey("");
      toast.success("AI Assistant disconnected");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  const baseModels = PROVIDER_MODELS[provider] || [];
  const modelOptions = [...baseModels, { value: "custom", label: "Custom model identifier…" }];

  const canTest = config?.configured || isLocal || apiKey.trim().length > 0;
  const isUnchanged =
    !apiKey.trim() && config?.provider === provider && config?.model === effectiveModel && (config?.baseUrl || "") === baseUrl.trim();

  return (
    <Card>
      <CardHeader
        title={
          <div className="flex items-center gap-2">
            <Sparkles className="size-4 text-purple-400" />
            <span>AI Assistant</span>
          </div>
        }
        description="Choose your preferred AI provider or local offline model to let MCPanel's AI assistant inspect servers, edit configurations, install plugins, and manage operations."
        actions={
          config?.configured ? (
            <Badge tone="success" className="gap-1">
              <CheckCircle2 className="size-3" /> Configured
            </Badge>
          ) : (
            <Badge tone="neutral">Not configured</Badge>
          )
        }
      />

      <div className="space-y-4 p-4 text-[13px]">
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="AI Provider">
            <Select
              aria-label="AI Provider"
              value={provider}
              onValueChange={handleProviderChange}
              options={PROVIDERS.map((p) => ({ value: p.value, label: p.label }))}
            />
          </Field>

          <Field label="Model">
            <Select aria-label="Model" value={model} onValueChange={handleModelChange} options={modelOptions} />
          </Field>
        </div>

        {isCustomModel && (
          <Field
            label="Custom Model Identifier"
            hint={`Enter the exact model ID for ${currentProvider.label} (e.g. ${currentProvider.defaultModel})`}
          >
            <Input
              value={customModel}
              onChange={(e) => setCustomModel(e.target.value)}
              placeholder={`e.g. ${currentProvider.defaultModel}`}
              aria-label="Custom Model Identifier"
            />
          </Field>
        )}

        {provider !== "gemini" && (
          <Field
            label="Base URL"
            hint={
              currentProvider.defaultBaseUrl
                ? `Default: ${currentProvider.defaultBaseUrl}. Enter a custom URL if using a custom port or proxy.`
                : "Leave blank for provider default, or enter custom proxy/base URL."
            }
          >
            <Input
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder={currentProvider.defaultBaseUrl || "https://api.openai.com/v1"}
              aria-label="Base URL"
            />
          </Field>
        )}

        <Field
          label={
            <div className="flex items-center justify-between">
              <span>{isLocal ? "API Key (Optional for local models)" : "API Key"}</span>
              {config?.configured && !isLocal && <span className="text-[11px] text-accent">Key stored securely in Windows Credential Manager</span>}
            </div>
          }
          hint={currentProvider.keyHint}
        >
          <div className="relative">
            <Input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={
                config?.configured
                  ? isLocal
                    ? "Not required for local models (leave blank)"
                    : "•••••••••••••••••••••••••••••••• (leave blank to keep existing key)"
                  : currentProvider.keyPlaceholder
              }
              aria-label="AI API Key"
              className="pr-8"
            />
            <Key className="pointer-events-none absolute top-2 right-2.5 size-4 text-faint" />
          </div>
        </Field>

        <div className="space-y-1.5 rounded-md border border-border bg-surface-2 p-3 text-xs text-muted">
          <div className="flex items-center gap-1.5 font-medium text-fg">
            <ShieldCheck className="size-4 text-accent" />
            <span>Safety & Automation Guardrails</span>
          </div>
          <p>
            • <strong className="text-fg">Autonomous tasks:</strong> Editing plugin configs, modifying{" "}
            <code className="text-fg">server.properties</code>, viewing files, searching plugins, and listing backups run efficiently without
            interrupting you.
          </p>
          <p>
            • <strong className="text-fg">Protected tasks:</strong> Turning servers on or off, deleting files or backups, restoring backups, and
            executing server console commands always pause for your explicit approval.
          </p>
          <p>
            • All AI operations are recorded into the <strong className="text-fg">Activity Log</strong> with the <code className="text-fg">AI</code>{" "}
            badge.
          </p>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2 pt-1">
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" onClick={() => void handleTest()} disabled={testing || saving || !canTest}>
              {testing ? <Loader2 className="size-3.5 animate-spin" /> : <Bot className="size-3.5" />}
              Test Connection
            </Button>
            {config?.configured && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => void handleClear()}
                disabled={saving || testing}
                className="text-danger hover:text-danger"
              >
                <Trash2 className="size-3.5" /> Disconnect
              </Button>
            )}
          </div>

          <Button variant="primary" size="sm" onClick={() => void handleSave()} disabled={saving || testing || isUnchanged}>
            {saving ? <Loader2 className="size-3.5 animate-spin" /> : <Sparkles className="size-3.5" />}
            Save Settings
          </Button>
        </div>
      </div>
    </Card>
  );
}
