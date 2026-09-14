import { useQuery, useMutation, useQueryClient } from "./query";
import { api } from "./client";
import {
  BootstrapDto,
  GameInstallationSummaryDto,
  GameInspectionDto,
  ProfileSummaryDto,
  ProfileOverviewDto,
  ModListItemDto,
  ModDetailsDto,
  OperationPreviewDto,
  OperationDto,
  SmapiStatusDto,
  LaunchSessionDto,
  DiagnosticsDto,
} from "./generated";

// Query keys
export const queryKeys = {
  bootstrap: () => ["bootstrap"] as const,
  overview: () => ["profile-overview"] as const,
  mods: (profileId?: string) => ["mods", profileId ?? "active"] as const,
  modDetails: (id: string) => ["mod-details", id] as const,
  profiles: () => ["profiles"] as const,
  games: () => ["games"] as const,
  smapi: (gameId?: string) => ["smapi", gameId ?? "active"] as const,
  operations: () => ["operations"] as const,
  operation: (id: string) => ["operation", id] as const,
  diagnostics: (gameId?: string) => ["diagnostics", gameId ?? "active"] as const,
  session: () => ["active-session"] as const,
};

export function useBootstrap() {
  return useQuery<BootstrapDto>({
    queryKey: queryKeys.bootstrap(),
    queryFn: () => api.bootstrap(),
    staleTime: 5000,
  });
}

export function useActiveProfileOverview() {
  return useQuery<ProfileOverviewDto>({
    queryKey: queryKeys.overview(),
    queryFn: () => api.getActiveProfileOverview(),
    staleTime: 3000,
  });
}

export function useProfileMods(profileId?: string) {
  return useQuery<ModListItemDto[]>({
    queryKey: queryKeys.mods(profileId),
    queryFn: () => api.listProfileMods(profileId),
    staleTime: 3000,
  });
}

export function useModDetails(profileComponentId: string) {
  return useQuery<ModDetailsDto>({
    queryKey: queryKeys.modDetails(profileComponentId),
    queryFn: () => api.getModDetails(profileComponentId),
    enabled: Boolean(profileComponentId),
  });
}

export function useGameInstallations() {
  return useQuery<GameInstallationSummaryDto[]>({
    queryKey: queryKeys.games(),
    queryFn: () => api.listGameInstallations(),
    staleTime: 10000,
  });
}

export function useDiscoverGames() {
  return useQuery<GameInspectionDto[]>({
    queryKey: ["discover-games"],
    queryFn: () => api.discoverGameInstallations(),
    staleTime: 10000,
  });
}

export function useProfiles() {
  return useQuery<ProfileSummaryDto[]>({
    queryKey: queryKeys.profiles(),
    queryFn: () => api.listProfiles(),
    staleTime: 5000,
  });
}

export function useSmapiStatus(gameId?: string) {
  return useQuery<SmapiStatusDto>({
    queryKey: queryKeys.smapi(gameId),
    queryFn: () => api.getSmapiStatus(gameId),
    staleTime: 5000,
  });
}

export function useRecentOperations(limit = 30) {
  return useQuery<OperationDto[]>({
    queryKey: queryKeys.operations(),
    queryFn: () => api.listRecentOperations(limit),
    staleTime: 2000,
  });
}

export function useOperationDetails(operationId: string) {
  return useQuery<OperationDto>({
    queryKey: queryKeys.operation(operationId),
    queryFn: () => api.getOperationDetails(operationId),
    enabled: Boolean(operationId),
    refetchInterval: 1000,
  });
}

export function useDiagnosticsReport(gameId?: string) {
  return useQuery<DiagnosticsDto>({
    queryKey: queryKeys.diagnostics(gameId),
    queryFn: () => api.getDiagnosticsReport(gameId),
    staleTime: 5000,
  });
}

export function useActiveLaunchSession() {
  return useQuery<LaunchSessionDto | null>({
    queryKey: queryKeys.session(),
    queryFn: () => api.getActiveLaunchSession(),
    refetchInterval: 2000,
  });
}

// Mutations

export function useActivateProfile() {
  const qc = useQueryClient();
  return useMutation<void, string>({
    mutationFn: (profileId) => api.activateProfile(profileId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: queryKeys.bootstrap() });
      qc.invalidateQueries({ queryKey: ["smapi"] });
      qc.invalidateQueries({ queryKey: queryKeys.operations() });
      qc.invalidateQueries({ queryKey: queryKeys.overview() });
      qc.invalidateQueries({ queryKey: queryKeys.profiles() });
      qc.invalidateQueries({ queryKey: ["mods"] });
    },
  });
}

export function useCreateProfile() {
  const qc = useQueryClient();
  return useMutation<ProfileSummaryDto, { name: string; gameInstallationId: string }>({
    mutationFn: ({ name, gameInstallationId }) =>
      api.createProfile(name, gameInstallationId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: queryKeys.profiles() });
    },
  });
}

export function useArchiveProfile() {
  const qc = useQueryClient();
  return useMutation<void, string>({
    mutationFn: (profileId) => api.archiveProfile(profileId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: queryKeys.profiles() });
      qc.invalidateQueries({ queryKey: queryKeys.overview() });
    },
  });
}

export function useArchivedProfiles() {
  return useQuery<ProfileSummaryDto[]>({
    queryKey: [...queryKeys.profiles(), "archived"],
    queryFn: () => api.listArchivedProfiles(),
  });
}

export function useRestoreProfile() {
  const qc = useQueryClient();
  return useMutation<void, string>({
    mutationFn: (profileId) => api.restoreProfile(profileId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: queryKeys.profiles() });
      qc.invalidateQueries({ queryKey: queryKeys.overview() });
    },
  });
}

export function useInspectPackage() {
  return useMutation<OperationPreviewDto, { archivePath: string; profileId?: string }>({
    mutationFn: ({ archivePath, profileId }) =>
      api.inspectPackageForInstall(archivePath, profileId),
  });
}

export function useExecuteOperation() {
  const qc = useQueryClient();
  return useMutation<OperationDto, string>({
    mutationFn: (operationId) => api.executeOperation(operationId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["mods"] });
      qc.invalidateQueries({ queryKey: queryKeys.overview() });
      qc.invalidateQueries({ queryKey: queryKeys.operations() });
    },
  });
}

export function useInstallSmapi() {
  const qc = useQueryClient();
  return useMutation<SmapiStatusDto, string | undefined>({
    mutationFn: (gameId) => api.installPinnedSmapi(gameId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["smapi"] });
      qc.invalidateQueries({ queryKey: queryKeys.overview() });
      qc.invalidateQueries({ queryKey: queryKeys.games() });
    },
  });
}

export function useLaunchGame() {
  const qc = useQueryClient();
  return useMutation<LaunchSessionDto, string | undefined>({
    mutationFn: (mode) => api.launchActiveProfile(mode ?? "Modded"),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: queryKeys.session() });
      qc.invalidateQueries({ queryKey: queryKeys.overview() });
    },
  });
}

export function useTerminateSession() {
  const qc = useQueryClient();
  return useMutation<void, string | undefined>({
    mutationFn: (sessionId) => api.terminateActiveLaunchSession(sessionId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: queryKeys.session() });
      qc.invalidateQueries({ queryKey: queryKeys.overview() });
    },
  });
}
