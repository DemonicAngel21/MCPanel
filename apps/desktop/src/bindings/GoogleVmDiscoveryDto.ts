// Generated from crates/mcpanel-api/src/dto.rs
export type GoogleVmCandidateDto = { projectId: string, projectName: string, zone: string, instanceName: string, status: string, externalIp: string | null, useAccount: boolean, };
export type GoogleVmDiscoveryDto = { instances: Array<GoogleVmCandidateDto>, projectsScanned: number, projectsUnavailable: number, truncated: boolean, };
