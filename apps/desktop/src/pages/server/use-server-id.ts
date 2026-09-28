import { useParams } from "@tanstack/react-router";

export function useServerId(): string {
  return useParams({ strict: false }).serverId ?? "";
}
