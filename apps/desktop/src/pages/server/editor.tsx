import Editor, { loader, type OnMount } from "@monaco-editor/react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useBlocker } from "@tanstack/react-router";
import * as monaco from "monaco-editor";
import editorWorker from "monaco-editor/editor/editor.worker?worker";
import jsonWorker from "monaco-editor/language/json/json.worker?worker";
import { AlertTriangle, ArrowLeft, Lock, Save } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { parseDocument } from "yaml";
import type { TextDocumentDto } from "@/bindings/TextDocumentDto";
import { Button } from "@/components/ui/button";
import { Banner, Spinner, Switch } from "@/components/ui/primitives";
import { api, ApiError } from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { errorMessage } from "@/lib/utils";
import { serverEditorRoute } from "@/router";
import { useUi } from "@/stores/ui";
import { useServerId } from "./use-server-id";

// Bundle Monaco locally (no CDN; CSP-compatible).
self.MonacoEnvironment = {
  getWorker(_id: string, label: string) {
    if (label === "json") return new jsonWorker();
    return new editorWorker();
  },
};
loader.config({ monaco });

function languageFor(path: string): string {
  const p = path.toLowerCase();
  if (p.endsWith(".json") || p.endsWith(".mcmeta") || p.endsWith(".json5")) return "json";
  if (p.endsWith(".yml") || p.endsWith(".yaml")) return "yaml";
  if (p.endsWith(".properties") || p.endsWith(".toml") || p.endsWith(".cfg") || p.endsWith(".ini") || p.endsWith(".conf")) return "ini";
  if (p.endsWith(".java")) return "java";
  if (p.endsWith(".js") || p.endsWith(".mjs")) return "javascript";
  if (p.endsWith(".ts")) return "typescript";
  if (p.endsWith(".md")) return "markdown";
  if (p.endsWith(".xml")) return "xml";
  if (p.endsWith(".sh")) return "shell";
  if (p.endsWith(".bat") || p.endsWith(".cmd")) return "bat";
  if (p.endsWith(".ps1")) return "powershell";
  if (p.endsWith(".html")) return "html";
  if (p.endsWith(".css")) return "css";
  return "plaintext";
}

const EOL_LABEL: Record<string, string> = { lf: "LF", cr_lf: "CRLF", mixed: "Mixed", none: "—" };

function ServerEditorInner({ serverId, path }: { serverId: string; path: string }) {
  const qc = useQueryClient();
  const docQuery = useQuery({
    queryKey: ["servers", serverId, "document", path],
    queryFn: () => api.files.read(serverId, path),
    staleTime: Infinity,
    gcTime: 0,
  });
  // The document last saved by this editor, tied to the query result it was based on.
  const [saved, setSaved] = useState<{ basis: number; doc: TextDocumentDto } | null>(null);
  const doc: TextDocumentDto | null = (saved && saved.basis === docQuery.dataUpdatedAt ? saved.doc : docQuery.data) ?? null;
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [conflict, setConflict] = useState(false);
  const autosave = useUi((s) => s.editorAutosave);
  const setAutosave = useUi((s) => s.setEditorAutosave);
  const wordWrap = useUi((s) => s.editorWordWrap);
  const setWordWrap = useUi((s) => s.setEditorWordWrap);
  const [problems, setProblems] = useState<string | null>(null);
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const timer = useRef<number | null>(null);

  const [theme, setTheme] = useState(() => document.documentElement.dataset.theme ?? "dark");
  useEffect(() => {
    const observer = new MutationObserver(() => {
      const next = document.documentElement.dataset.theme ?? "dark";
      setTheme(next);
      monaco.editor.setTheme(next === "light" ? "vs" : "vs-dark");
    });
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);

  const reload = () => {
    setSaved(null);
    setDirty(false);
    setConflict(false);
    void docQuery.refetch();
  };

  const readOnly = !!doc?.readOnlyReason;
  const language = languageFor(path);

  const validate = useCallback(
    (text: string) => {
      if (language !== "yaml") return;
      const model = editorRef.current?.getModel();
      if (!model) return;
      const parsed = parseDocument(text, { prettyErrors: false });
      const markers = parsed.errors.map((e) => {
        const pos = e.linePos?.[0];
        return {
          severity: monaco.MarkerSeverity.Error,
          message: e.message.split("\n")[0] ?? "YAML error",
          startLineNumber: pos?.line ?? 1,
          startColumn: pos?.col ?? 1,
          endLineNumber: pos?.line ?? 1,
          endColumn: (pos?.col ?? 1) + 1,
        };
      });
      monaco.editor.setModelMarkers(model, "yaml", markers);
      setProblems(markers.length ? `${markers.length} YAML problem${markers.length > 1 ? "s" : ""}` : null);
    },
    [language],
  );

  const save = useCallback(
    async (force = false) => {
      const editor = editorRef.current;
      if (!editor || !doc || readOnly) return;
      setSaving(true);
      try {
        const savedDoc = await api.files.write(serverId, path, {
          content: editor.getValue(),
          encoding: doc.encoding,
          bom: doc.bom,
          expectedSha256: force ? null : doc.sha256,
        });
        setSaved({ basis: docQuery.dataUpdatedAt, doc: savedDoc });
        setDirty(false);
        setConflict(false);
        void qc.invalidateQueries({ queryKey: ["servers", serverId, "files"] });
        if (path === "server.properties") void qc.invalidateQueries({ queryKey: ["servers", serverId, "properties"] });
      } catch (e) {
        if (e instanceof ApiError && e.code === "FILE_CHANGED_ON_DISK") setConflict(true);
        else toast.error(errorMessage(e));
      } finally {
        setSaving(false);
      }
    },
    [doc, docQuery.dataUpdatedAt, path, qc, readOnly, serverId],
  );

  const saveRef = useRef(save);
  useEffect(() => {
    saveRef.current = save;
  }, [save]);

  const onMount: OnMount = (editor) => {
    editorRef.current = editor;
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => void saveRef.current());
    validate(editor.getValue());
  };

  useEffect(() => {
    if (!autosave || !dirty || conflict) return;
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void saveRef.current(), 1500);
    return () => {
      if (timer.current) window.clearTimeout(timer.current);
    };
  }, [autosave, dirty, conflict]);

  useBlocker({
    shouldBlockFn: () => dirty && !window.confirm("You have unsaved changes. Leave without saving?"),
    enableBeforeUnload: () => dirty,
  });

  const dir = path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-3 border-b border-border bg-surface px-6 py-2">
        <Button asChild variant="ghost" size="sm">
          <Link to="/servers/$serverId/files" params={{ serverId }} search={{ path: dir }}>
            <ArrowLeft /> Files
          </Link>
        </Button>
        <p className="min-w-0 flex-1 truncate font-mono text-xs text-fg">
          {path}
          {dirty && <span className="ml-2 text-warning">● unsaved</span>}
        </p>
        {problems && <span className="text-xs text-danger">{problems}</span>}
        <label className="flex items-center gap-2 text-xs text-muted">
          <Switch checked={wordWrap} onCheckedChange={setWordWrap} /> Word wrap
        </label>
        <label className="flex items-center gap-2 text-xs text-muted">
          <Switch checked={autosave} onCheckedChange={setAutosave} disabled={readOnly} /> Autosave
        </label>
        <Button variant="primary" size="sm" disabled={!dirty || saving || readOnly} onClick={() => save()}>
          {saving ? <Spinner className="text-accent-fg" /> : <Save />} Save
        </Button>
      </div>
      {(conflict || readOnly || docQuery.isError) && (
        <div className="space-y-2 px-6 pt-3">
          {docQuery.isError && (
            <Banner tone="danger" icon={<AlertTriangle />} title="Cannot open this file">
              {errorMessage(docQuery.error)}
            </Banner>
          )}
          {readOnly && (
            <Banner tone="info" icon={<Lock />} title="Read-only">
              {doc?.readOnlyReason}
            </Banner>
          )}
          {conflict && (
            <Banner
              tone="warning"
              icon={<AlertTriangle />}
              title="This file changed on disk"
              actions={
                <>
                  <Button size="sm" variant="ghost" onClick={reload}>
                    Discard my changes and reload
                  </Button>
                  <Button size="sm" variant="danger-outline" onClick={() => save(true)}>
                    Overwrite
                  </Button>
                </>
              }
            >
              Another program (or the server itself) modified it after you opened it.
            </Banner>
          )}
        </div>
      )}
      <div className="min-h-0 flex-1 px-6 py-3">
        <div className="h-full overflow-hidden rounded-lg border border-border">
          {doc ? (
            <Editor
              key={`${path}-${doc.sha256}-${conflict}`}
              defaultValue={doc.content}
              language={language}
              theme={theme === "light" ? "vs" : "vs-dark"}
              onMount={onMount}
              onChange={(v) => {
                setDirty(true);
                validate(v ?? "");
              }}
              options={{
                readOnly,
                minimap: { enabled: doc.size < 512 * 1024 },
                fontFamily: "JetBrains Mono Variable, Consolas, monospace",
                fontSize: 13,
                scrollBeyondLastLine: false,
                renderWhitespace: "selection",
                automaticLayout: true,
                tabSize: 2,
                wordWrap: wordWrap ? "on" : "off",
              }}
            />
          ) : (
            <div className="flex h-full items-center justify-center">
              <Spinner />
            </div>
          )}
        </div>
      </div>
      {doc && (
        <div className="flex items-center gap-4 border-t border-border bg-surface px-6 py-1.5 text-[11px] text-faint">
          <span>
            {doc.encoding}
            {doc.bom ? " with BOM" : ""}
          </span>
          <span>{EOL_LABEL[doc.lineEnding] ?? doc.lineEnding}</span>
          <span>{formatBytes(doc.size)}</span>
          <span>{language}</span>
          <span className="flex-1" />
          <span>Ctrl+S save · Ctrl+F find · Ctrl+H replace</span>
        </div>
      )}
    </div>
  );
}

export default function ServerEditor() {
  const serverId = useServerId();
  const { path } = serverEditorRoute.useSearch();
  return <ServerEditorInner key={`${serverId}:${path}`} serverId={serverId} path={path} />;
}
