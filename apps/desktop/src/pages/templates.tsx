import { Link } from "@tanstack/react-router";
import { Blocks, Gauge, LayoutTemplate, Skull, Trees, Users } from "lucide-react";
import type { ReactNode } from "react";
import { PageBody, PageHeader } from "@/app/app-shell";
import { Button } from "@/components/ui/button";
import { Badge, Card, Skeleton } from "@/components/ui/primitives";
import { useSoftware, useTemplates } from "@/lib/queries";

const ICONS: Record<string, ReactNode> = {
  trees: <Trees />,
  users: <Users />,
  blocks: <Blocks />,
  skull: <Skull />,
  gauge: <Gauge />,
};

function every(minutes: number) {
  return minutes % 60 === 0 ? `every ${minutes / 60} h` : `every ${minutes} min`;
}

export function TemplatesPage() {
  const { data: templates, isLoading } = useTemplates();
  const { data: software } = useSoftware();
  const name = (id: string) => software?.find((s) => s.id === id)?.displayName ?? id;
  return (
    <>
      <PageHeader
        title="Templates"
        description="Start a new server from a preset. The create wizard is filled in from the template, and you can still change everything before creating the server."
      />
      <PageBody>
        {isLoading && (
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
            {Array.from({ length: 5 }, (_, i) => (
              <Skeleton key={i} className="h-40 rounded-lg" />
            ))}
          </div>
        )}
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {templates?.map((t) => (
            <Card key={t.id} className="flex flex-col p-4 transition-colors duration-150 hover:border-border-strong">
              <div className="mb-2 flex items-center gap-2.5">
                <span className="flex size-9 items-center justify-center rounded-md bg-accent-soft text-accent [&_svg]:size-5">
                  {ICONS[t.icon] ?? <LayoutTemplate />}
                </span>
                <h2 className="text-[14px] font-semibold text-fg">{t.name}</h2>
              </div>
              <p className="flex-1 text-xs text-muted">{t.description}</p>
              <div className="mt-3 flex flex-wrap gap-1">
                <Badge>{t.software.map(name).join(" / ")}</Badge>
                {t.memoryMb && (
                  <Badge>
                    {t.memoryMb.min / 1024}–{t.memoryMb.max / 1024} GB
                  </Badge>
                )}
                {t.backupIntervalMinutes && <Badge tone="info">Backups {every(t.backupIntervalMinutes)}</Badge>}
                {t.autoRestart && <Badge tone="success">Auto-restart</Badge>}
                {t.plugins.map((p) => (
                  <Badge key={p.project} tone="warning">
                    + {p.name}
                  </Badge>
                ))}
              </div>
              <Button asChild variant="primary" size="sm" className="mt-4 self-start">
                <Link to="/servers/new" search={{ template: t.id }}>
                  Create server
                </Link>
              </Button>
            </Card>
          ))}
        </div>
      </PageBody>
    </>
  );
}
