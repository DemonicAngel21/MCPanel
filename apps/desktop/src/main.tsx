import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Toaster } from "sonner";
import { TooltipProvider } from "@/components/ui/primitives";
import { router } from "./router";
import "./styles.css";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5_000,
      retry: (count, error) => count < 2 && (error as { retryable?: boolean }).retryable === true,
      refetchOnWindowFocus: false,
    },
  },
});

// Block the WebView's default context menu except in editable fields.
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement | null;
  if (!t?.closest("input, textarea, .monaco-editor, .selectable")) e.preventDefault();
});

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <RouterProvider router={router} context={{ queryClient }} />
          <Toaster
            position="bottom-right"
            theme="system"
            toastOptions={{
              classNames: {
                toast: "!bg-surface-2 !border-border-strong !text-fg",
                description: "!text-muted",
              },
            }}
          />
        </TooltipProvider>
      </QueryClientProvider>
    </StrictMode>,
  );
}
