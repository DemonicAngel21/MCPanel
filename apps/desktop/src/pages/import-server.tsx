import { useNavigate } from "@tanstack/react-router";
import { AlertTriangle, FolderOpen, Search } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import type { GrantDto } from "@/bindings/GrantDto";
import type { ImportDetectionDto } from "@/bindings/ImportDetectionDto";
import { PageBody, PageHeader } from "@/app/app-shell";
import { SoftwareMark } from "@/components/software-mark";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/overlays";
import { Banner, Card, Field, Input, Spinner } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { useJava, useSoftware } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";
import { LOCATION_WARNING_TEXT as WARNING_TEXT } from "@/lib/location";

export function ImportServerPage() {
  const navigate = useNavigate();
  const { data: software } = useSoftware();
  const { data: java } = useJava();
  const [grant, setGrant] = useState<GrantDto | null>(null);
  const [detection, setDetection] = useState<ImportDetectionDto | null>(null);
  const [busy, setBusy] = useState(false);
  const [name, setName] = useState("");
  const [softwareId, setSoftwareId] = useState<string | undefined>();
  const [version, setVersion] = useState("");
  const [jar, setJar] = useState<string | undefined>();
  const [javaId, setJavaId] = useState<string | undefined>();
  const [maxMem, setMaxMem] = useState(4096);
  const selectedSoftware = software?.find((entry) => entry.id === softwareId);

  const pick = async () => {
    try {
      const g = await api.dialog.pickFolder("Choose an existing server folder");
      if (!g) return;
      setBusy(true);
      const d = await api.servers.detectImport(g.token);
      setGrant(g);
      setDetection(d);
      setName(g.name);
      setSoftwareId(d.detected?.softwareId);
      setVersion(d.detected?.gameVersion ?? "");
      setJar(d.detected?.jar ?? d.jars[0]);
      setJavaId(java?.filter((j) => j.valid).sort((a, b) => b.major - a.major)[0]?.id);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const submit = async () => {
    if (!grant) return;
    setBusy(true);
    try {
      const s = await api.servers.import({
        name,
        directoryGrant: grant.token,
        softwareId: softwareId ?? null,
        gameVersion: version || null,
        jar: jar ?? null,
        javaRuntimeId: javaId ?? null,
        minMemoryMb: 1024,
        maxMemoryMb: maxMem,
      });
      toast.success(`Imported ${s.name}`);
      void navigate({ to: "/servers/$serverId", params: { serverId: s.id } });
    } catch (e) {
      toast.error(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <>
      <PageHeader title="Import server" description="Add a server folder you already have. MCPanel does not move or modify files when importing." />
      <PageBody>
        <div className="space-y-5">
          <Card className="flex items-center justify-between gap-4 p-5">
            <div className="min-w-0">
              <p className="text-[13px] font-medium text-fg">{grant ? grant.displayPath : "No folder selected"}</p>
              <p className="text-xs text-muted">Select the folder that contains the server jar and server.properties.</p>
            </div>
            <Button variant={grant ? "outline" : "primary"} onClick={pick} disabled={busy}>
              {busy ? <Spinner /> : <FolderOpen />} {grant ? "Choose another" : "Choose folder"}
            </Button>
          </Card>

          {detection && (
            <>
              {detection.warnings.map((w) => (
                <Banner key={w} tone="warning" icon={<AlertTriangle />} title="Check this location">
                  {WARNING_TEXT[w] ?? w}
                </Banner>
              ))}
              <Banner
                tone={detection.detected ? "success" : "warning"}
                icon={<Search />}
                title={detection.detected ? "Server software detected" : "Could not detect the server software"}
              >
                {detection.detected
                  ? `Detected ${software?.find((s) => s.id === detection.detected?.softwareId)?.displayName ?? detection.detected.softwareId}${detection.detected.gameVersion ? ` ${detection.detected.gameVersion}` : ""}. Review the details below.`
                  : "Choose the software, version and jar below."}
                {!detection.hasEula && " The Minecraft EULA has not been accepted in this folder yet; you will be asked before the first start."}
              </Banner>
              <Card className="grid grid-cols-2 gap-4 p-5">
                <Field label="Name" className="col-span-2">
                  <Input value={name} onChange={(e) => setName(e.target.value)} maxLength={64} />
                </Field>
                <Field
                  label={
                    <span className="flex items-center gap-2">
                      <SoftwareMark softwareId={softwareId} name={selectedSoftware?.displayName} size="sm" />
                      Server software
                    </span>
                  }
                >
                  <Select
                    value={softwareId}
                    onValueChange={setSoftwareId}
                    placeholder="Select"
                    options={(software ?? []).map((s) => ({ value: s.id, label: s.displayName }))}
                  />
                </Field>
                <Field label="Minecraft version" hint="For example 1.21.4 or 26.3">
                  <Input value={version} onChange={(e) => setVersion(e.target.value.trim())} />
                </Field>
                <Field label="Server jar">
                  <Select value={jar} onValueChange={setJar} placeholder="Select" options={detection.jars.map((j) => ({ value: j, label: j }))} />
                </Field>
                <Field label="Java runtime">
                  <Select
                    value={javaId}
                    onValueChange={setJavaId}
                    placeholder="Select"
                    options={(java ?? []).filter((j) => j.valid).map((j) => ({ value: j.id, label: `Java ${j.major}`, hint: j.vendor ?? undefined }))}
                  />
                </Field>
                <Field label="Maximum memory (MB)">
                  <Input inputMode="numeric" value={maxMem} onChange={(e) => setMaxMem(Number(e.target.value.replace(/\D/g, "")) || 0)} />
                </Field>
              </Card>
              <div className="flex justify-end">
                <Button variant="primary" disabled={busy || !name.trim() || !softwareId || !version || !jar} onClick={submit}>
                  Import server
                </Button>
              </div>
            </>
          )}
        </div>
      </PageBody>
    </>
  );
}
