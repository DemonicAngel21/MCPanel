import { cleanup, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AccountPanel } from "./account-panel";
import { MAIN_NAV_ITEMS } from "@/app/main-nav-items";

const mockUseAccount = vi.hoisted(() => vi.fn());
vi.mock("@/lib/queries", () => ({ qk: { account: ["account"] }, useAccount: mockUseAccount }));

function renderPanel() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <AccountPanel />
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  mockUseAccount.mockReset();
});

describe("Account entry and states", () => {
  it("keeps Account in the main sidebar regardless of account availability", () => {
    expect(MAIN_NAV_ITEMS.find(({ label }) => label === "Account")).toMatchObject({ to: "/account" });
  });

  it("shows an unavailable explanation when Firebase is not configured", () => {
    mockUseAccount.mockReturnValue({ data: { configured: false, googleAvailable: false, signedIn: false, profile: null } });
    renderPanel();
    expect(screen.getByText(/Accounts are not available in this build/)).toBeInTheDocument();
  });

  it("shows unavailable state instead of stale sign-in controls when Firebase is absent", () => {
    mockUseAccount.mockReturnValue({
      data: {
        configured: false,
        googleAvailable: false,
        signedIn: true,
        profile: {
          uid: "cached-user",
          email: "user@example.test",
          emailVerified: true,
          displayName: "Cached User",
          photoUrl: null,
          provider: "password",
        },
      },
    });
    renderPanel();
    expect(screen.getByText(/Accounts are not available in this build/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Sign out/ })).not.toBeInTheDocument();
  });

  it("shows create and sign-in options when Firebase is configured and signed out", () => {
    mockUseAccount.mockReturnValue({ data: { configured: true, googleAvailable: true, signedIn: false, profile: null } });
    renderPanel();
    expect(screen.getByRole("tab", { name: "Create account" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Sign in" })).toBeInTheDocument();
  });

  it("shows Google, Microsoft, and Guest options when signed out", () => {
    mockUseAccount.mockReturnValue({
      data: {
        configured: true,
        googleAvailable: true,
        microsoftAvailable: true,
        signedIn: false,
        isGuest: false,
        profile: null,
      },
    });
    renderPanel();
    expect(screen.getByRole("button", { name: /Continue with Google/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Continue with Microsoft/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Continue as Guest/ })).toBeInTheDocument();
  });

  it("shows account state, sync button and controls when signed in", () => {
    mockUseAccount.mockReturnValue({
      data: {
        configured: true,
        googleAvailable: true,
        microsoftAvailable: true,
        signedIn: true,
        isGuest: false,
        profile: {
          accountId: "test-user",
          accountType: "email",
          uid: "test-user",
          email: "user@example.test",
          emailVerified: true,
          displayName: "Test User",
          photoUrl: null,
          provider: "password",
        },
      },
    });
    renderPanel();
    expect(screen.getByText("Test User")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Sync Settings/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Sign out/ })).toBeInTheDocument();
  });

  it("shows guest mode state with upgrade and exit buttons", () => {
    mockUseAccount.mockReturnValue({
      data: {
        configured: true,
        googleAvailable: true,
        microsoftAvailable: true,
        signedIn: false,
        isGuest: true,
        profile: {
          accountId: "guest",
          accountType: "guest",
          uid: "guest",
          email: null,
          emailVerified: false,
          displayName: "Guest User",
          photoUrl: null,
          provider: "guest",
        },
      },
    });
    renderPanel();
    expect(screen.getByText("Guest User")).toBeInTheDocument();
    expect(screen.getByText("Guest Mode")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Sign In or Create Account/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Exit Guest Mode/ })).toBeInTheDocument();
  });
});
