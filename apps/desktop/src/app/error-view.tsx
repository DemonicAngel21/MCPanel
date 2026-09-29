import { useRouter, type ErrorComponentProps } from "@tanstack/react-router";
import { AlertTriangle, Compass } from "lucide-react";
import { Button } from "@/components/ui/button";

/** Shown when a page throws while rendering. The rest of the app keeps working. */
export function ErrorView({ error, reset }: ErrorComponentProps) {
  const router = useRouter();
  return (
    <div className="flex flex-1 items-center justify-center p-8">
      <div className="max-w-md rounded-xl border border-danger/30 bg-surface p-6 text-center">
        <AlertTriangle className="mx-auto mb-3 size-8 text-danger" />
        <p className="text-sm font-semibold text-fg">This page could not be displayed</p>
        <p className="selectable mt-2 font-mono text-xs break-words text-muted">{error instanceof Error ? error.message : String(error)}</p>
        <p className="mt-2 text-xs text-faint">Your servers are not affected. Details are in the MCPanel logs.</p>
        <div className="mt-4 flex justify-center gap-2">
          <Button variant="outline" onClick={() => router.history.back()}>
            Go back
          </Button>
          <Button
            variant="primary"
            onClick={() => {
              reset();
              void router.invalidate();
            }}
          >
            Try again
          </Button>
        </div>
      </div>
    </div>
  );
}

/** Shown for an address that matches no page (e.g. a removed server's tab). */
export function NotFoundView() {
  const router = useRouter();
  return (
    <div className="flex flex-1 items-center justify-center p-8">
      <div className="max-w-sm text-center">
        <Compass className="mx-auto mb-3 size-8 text-faint" />
        <p className="text-sm font-semibold text-fg">This page does not exist</p>
        <p className="mt-1.5 text-xs text-muted">The server or page may have been removed.</p>
        <div className="mt-4 flex justify-center gap-2">
          <Button variant="outline" onClick={() => router.history.back()}>
            Go back
          </Button>
          <Button variant="primary" onClick={() => void router.navigate({ to: "/" })}>
            Dashboard
          </Button>
        </div>
      </div>
    </div>
  );
}
