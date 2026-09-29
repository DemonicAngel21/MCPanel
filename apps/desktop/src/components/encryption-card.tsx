import { useQueryClient } from "@tanstack/react-query";
import { KeyRound, Lock, Upload } from "lucide-react";
import { useState, type FormEvent } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent } from "@/components/ui/overlays";
import { Badge, Banner, Card, CardHeader, Field, Input, Spinner, Switch } from "@/components/ui/primitives";
import { api } from "@/lib/api";
import { formatRelative } from "@/lib/format";
import { qk, useEncryption } from "@/lib/queries";
import { errorMessage } from "@/lib/utils";

function SetupDialog({ open, onOpenChange, min }: { open: boolean; onOpenChange: (o: boolean) => void; min: number }) {
  const qc = useQueryClient();
  const [pass, setPass] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const tooShort = [...pass].length < min;
  const mismatch = confirm.length > 0 && pass !== confirm;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (tooShort || pass !== confirm) return;
    setBusy(true);
    try {
      const grant = await api.dialog.saveFile("MCPanel Recovery Kit.txt");
      if (!grant) return;
      qc.setQueryData(qk.encryption, await api.encryption.setup(pass, grant.token));
      toast.success("Backup encryption is on. Keep the Recovery Kit and passphrase safe.");
      setPass("");
      setConfirm("");
      onOpenChange(false);
    } catch (err) {
      toast.error(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent title="Set up backup encryption" description="New backups are encrypted with a key that stays on this computer.">
        <form onSubmit={submit} className="space-y-4">
          <p className="text-xs text-muted">
            MCPanel creates the key and saves a <b>Recovery Kit</b>: the key protected by your passphrase. You need the kit and the passphrase to open
            encrypted backups on another computer or after reinstalling Windows. Without them the backups cannot be recovered — not even by MCPanel.
          </p>
          <Field label="Passphrase" hint={`At least ${min} characters. Write it down somewhere safe.`}>
            <Input type="password" autoComplete="new-password" value={pass} onChange={(e) => setPass(e.target.value)} autoFocus />
          </Field>
          <Field label="Repeat the passphrase" hint={mismatch ? "The passphrases do not match." : undefined}>
            <Input type="password" autoComplete="new-password" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
          </Field>
          <div className="flex justify-end gap-2">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={busy || tooShort || pass !== confirm}>
              {busy ? <Spinner className="text-accent-fg" /> : <KeyRound />} Save Recovery Kit and turn on
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function ImportDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const qc = useQueryClient();
  const [pass, setPass] = useState("");
  const [busy, setBusy] = useState(false);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      const grant = await api.dialog.pickFile("Choose your MCPanel Recovery Kit");
      if (!grant) return;
      qc.setQueryData(qk.encryption, await api.encryption.import(grant.token, pass));
      toast.success("Encryption key restored");
      setPass("");
      onOpenChange(false);
    } catch (err) {
      toast.error(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent title="Import Recovery Kit" description="Restores the key for your encrypted backups on this computer.">
        <form onSubmit={submit} className="space-y-4">
          <Field label="Passphrase of the Recovery Kit">
            <Input type="password" autoComplete="current-password" value={pass} onChange={(e) => setPass(e.target.value)} autoFocus />
          </Field>
          <div className="flex justify-end gap-2">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" disabled={busy || pass.length === 0}>
              {busy ? <Spinner className="text-accent-fg" /> : <Upload />} Choose file and import
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Settings card for the Backup Master Key and the "encrypt new backups" switch. */
export function EncryptionCard() {
  const qc = useQueryClient();
  const { data: s } = useEncryption();
  const [setup, setSetup] = useState(false);
  const [importing, setImporting] = useState(false);
  if (!s) return null;
  const toggle = async (on: boolean) => {
    try {
      qc.setQueryData(qk.encryption, await api.encryption.setEnabled(on));
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };
  return (
    <Card>
      <CardHeader
        title="Backup encryption"
        description="Encrypts backup files (age, X25519) so a copied or uploaded backup cannot be read without your key."
        actions={
          s.configured && s.keyAvailable ? (
            <label className="flex items-center gap-2 text-xs text-muted">
              Encrypt new backups
              <Switch checked={s.encryptBackups} onCheckedChange={(v) => void toggle(v)} aria-label="Encrypt new backups" />
            </label>
          ) : undefined
        }
      />
      <div className="space-y-3 p-4 text-xs text-muted">
        {!s.configured && <p>Backups are stored as ordinary ZIP files. Turn on encryption to protect them if you copy them elsewhere.</p>}
        {s.configured && (
          <div className="flex flex-wrap items-center gap-2">
            <Badge tone={s.keyAvailable ? "success" : "warning"}>
              <Lock className="size-3" /> {s.keyAvailable ? "Key on this computer" : "Key missing"}
            </Badge>
            <span className="selectable font-mono text-[11px] break-all text-faint">{s.recipient}</span>
            {s.createdAt != null && <span className="text-faint">· created {formatRelative(s.createdAt)}</span>}
          </div>
        )}
        {s.configured && !s.keyAvailable && (
          <Banner tone="warning" icon={<KeyRound />} title="The encryption key is not on this computer">
            Encrypted backups cannot be verified or restored, and new backups are not encrypted. Import your Recovery Kit to fix this.
          </Banner>
        )}
        <div className="flex flex-wrap gap-2">
          {!s.configured && (
            <Button size="sm" variant="primary" onClick={() => setSetup(true)}>
              <KeyRound /> Set up encryption
            </Button>
          )}
          <Button size="sm" variant="ghost" onClick={() => setImporting(true)}>
            <Upload /> Import Recovery Kit
          </Button>
        </div>
      </div>
      <SetupDialog open={setup} onOpenChange={setSetup} min={s.minPassphraseChars} />
      <ImportDialog open={importing} onOpenChange={setImporting} />
    </Card>
  );
}
