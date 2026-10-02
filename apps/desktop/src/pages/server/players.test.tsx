import { cleanup, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ServerPlayers } from "./players";
import type { ServerPlayersDto } from "@/bindings/ServerPlayersDto";

const mockUsePlayers = vi.hoisted(() => vi.fn());
vi.mock("@/lib/queries", () => ({
  qk: { players: (id: string) => ["players", id] },
  usePlayers: mockUsePlayers,
}));

vi.mock("./use-server-id", () => ({
  useServerId: () => "test-server-1",
}));

vi.mock("@/lib/api", () => ({
  api: {
    players: {
      action: vi.fn(),
    },
  },
}));

function baseData(overrides: Partial<ServerPlayersDto> = {}): ServerPlayersDto {
  return {
    live: true,
    readOnlyReason: null,
    onlineKnown: true,
    online: [],
    maxPlayers: 20,
    onlineMode: true,
    whitelistEnabled: false,
    enforceWhitelist: false,
    known: [],
    whitelist: [],
    operators: [],
    bans: [],
    ipBans: [],
    ...overrides,
  };
}

function renderComponent() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ServerPlayers />
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
  mockUsePlayers.mockReset();
});

describe("ServerPlayers Whitelist & Online Mode logic", () => {
  it("shows offline-mode active info banner when onlineMode=false and whitelistEnabled=true, without asking to enable whitelist", () => {
    mockUsePlayers.mockReturnValue({
      data: baseData({
        onlineMode: false,
        whitelistEnabled: true,
        whitelist: [
          {
            name: "testplayer",
            uuid: "00000000-0000-0000-0000-000000000001",
          },
        ],
      }),
      isLoading: false,
    });

    renderComponent();

    // Must show whitelist active info banner
    expect(screen.getByText("Offline mode (whitelist active)")).toBeInTheDocument();
    expect(screen.getByText("Online verification is disabled, but only whitelisted player names can connect to the server.")).toBeInTheDocument();

    // Must NOT ask to enable the whitelist
    expect(screen.queryByText(/Enable the whitelist, turn on online mode/i)).not.toBeInTheDocument();
  });

  it("shows offline-mode warning banner when onlineMode=false and whitelistEnabled=false", () => {
    mockUsePlayers.mockReturnValue({
      data: baseData({
        onlineMode: false,
        whitelistEnabled: false,
      }),
      isLoading: false,
    });

    renderComponent();

    expect(screen.getByText("Offline mode (whitelist disabled)")).toBeInTheDocument();
    expect(screen.getByText(/Names aren’t verified; anyone can join with any username/i)).toBeInTheDocument();
    expect(screen.queryByText("Offline mode (whitelist active)")).not.toBeInTheDocument();
  });

  it("does not show offline-mode banners when onlineMode=true", () => {
    mockUsePlayers.mockReturnValue({
      data: baseData({
        onlineMode: true,
        whitelistEnabled: true,
        whitelist: [
          {
            name: "player1",
            uuid: "00000000-0000-0000-0000-000000000001",
          },
        ],
      }),
      isLoading: false,
    });

    renderComponent();

    expect(screen.queryByText("Offline mode (whitelist disabled)")).not.toBeInTheDocument();
    expect(screen.queryByText("Offline mode (whitelist active)")).not.toBeInTheDocument();
    expect(screen.queryByText(/The whitelist is on and empty/i)).not.toBeInTheDocument();
  });

  it("shows empty whitelist warning when whitelistEnabled=true and whitelist is empty", () => {
    mockUsePlayers.mockReturnValue({
      data: baseData({
        onlineMode: true,
        whitelistEnabled: true,
        whitelist: [],
      }),
      isLoading: false,
    });

    renderComponent();

    expect(screen.getByText("The whitelist is on and empty — nobody can join")).toBeInTheDocument();
  });
});
