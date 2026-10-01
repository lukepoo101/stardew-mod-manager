import { useQuery, useMutation } from "@tanstack/react-query";
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
  PreflightDto,
  DismissedFindingDto,
  ExperimentDto,
  SavesDto,
  ModAnnotationDto,
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
  diagnostics: (gameId?: string) =>
    ["diagnostics", gameId ?? "active"] as const,
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
    // Operation progress is written by the backend while a command is still
    // running, so polling stays: no command completion can announce it.
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
    // Process observation changes without a command completing.
    refetchInterval: 2000,
  });
}

/** The last launch of the active profile, running or finished. */
export function useLatestLaunchSession() {
  return useQuery<LaunchSessionDto | null>({
    queryKey: ["latest-session"] as const,
    queryFn: () => api.getLatestLaunchSession(),
    refetchInterval: 5000,
  });
}

/** The freeze on a profile, or null when it is not frozen. */
export function useProfileFreeze(profileId?: string) {
  return useQuery({
    queryKey: ["profile-freeze", profileId ?? "none"] as const,
    queryFn: () => (profileId ? api.getProfileFreeze(profileId) : null),
    enabled: Boolean(profileId),
    staleTime: 5000,
  });
}

/** Profiles made to try changes, with the profile each came from. */
export function useExperiments() {
  return useQuery<ExperimentDto[]>({
    queryKey: ["experiments"] as const,
    queryFn: () => api.listExperiments(),
    staleTime: 5000,
  });
}

/** Stardew Valley saves with their usual profile and backups. */
export function useSaves() {
  return useQuery<SavesDto>({
    queryKey: ["saves"] as const,
    queryFn: () => api.listSaves(),
    staleTime: 10000,
  });
}

/** What the profile last ran successfully with, if it ever did. */
export function useKnownGood(profileId?: string) {
  return useQuery({
    queryKey: ["known-good", profileId ?? "none"] as const,
    queryFn: () => (profileId ? api.getKnownGood(profileId) : null),
    enabled: Boolean(profileId),
    staleTime: 5000,
  });
}

/** The shared recipe a profile is kept in step with, if any. */
export function useReferenceRecipe(profileId?: string) {
  return useQuery({
    queryKey: ["reference-recipe", profileId ?? "none"] as const,
    queryFn: () => (profileId ? api.getReferenceRecipe(profileId) : null),
    enabled: Boolean(profileId),
    staleTime: 5000,
  });
}

/** Mods in a profile with a required dependency that is not met. */
export function useModProblems(profileId?: string) {
  return useQuery({
    queryKey: ["mod-problems", profileId ?? "none"] as const,
    queryFn: () => (profileId ? api.listModProblems(profileId) : []),
    enabled: Boolean(profileId),
    staleTime: 3000,
  });
}

/** Saved states of a profile's mods, newest first. */
export function useRestorePoints(profileId?: string) {
  return useQuery({
    queryKey: ["restore-points", profileId ?? "none"] as const,
    queryFn: () => (profileId ? api.listRestorePoints(profileId) : []),
    enabled: Boolean(profileId),
    staleTime: 5000,
  });
}

// Mutations
//
// None of these refresh server state themselves. The backend emits one
// "backend-state-changed" event for every state-changing command, and
// shared/api/events.ts turns it into a cache refresh. Keeping that mapping in one
// place means a new mutation cannot forget a query key, and a failed command
// still refreshes state it may have durably changed before failing.

export function useActivateProfile() {
  return useMutation<void, Error, string>({
    mutationFn: (profileId) => api.activateProfile(profileId),
  });
}

export function useCreateProfile() {
  return useMutation<
    ProfileSummaryDto,
    Error,
    { name: string; gameInstallationId: string }
  >({
    mutationFn: ({ name, gameInstallationId }) =>
      api.createProfile(name, gameInstallationId),
  });
}

export function useUpdateProfileDetails() {
  return useMutation<
    ProfileSummaryDto,
    Error,
    { profileId: string; name: string; description: string | null }
  >({
    mutationFn: ({ profileId, name, description }) =>
      api.updateProfileDetails(profileId, name, description),
  });
}

export function useArchiveProfile() {
  return useMutation<void, Error, string>({
    mutationFn: (profileId) => api.archiveProfile(profileId),
  });
}

export function useArchivedProfiles() {
  return useQuery<ProfileSummaryDto[]>({
    queryKey: [...queryKeys.profiles(), "archived"],
    queryFn: () => api.listArchivedProfiles(),
  });
}

export function useRestoreProfile() {
  return useMutation<void, Error, string>({
    mutationFn: (profileId) => api.restoreProfile(profileId),
  });
}

export function useSetDefaultProfile() {
  return useMutation<void, Error, string | null>({
    mutationFn: (profileId) => api.setDefaultProfile(profileId),
  });
}

export function useInspectPackage() {
  return useMutation<
    OperationPreviewDto,
    Error,
    { archivePath: string; profileId?: string }
  >({
    mutationFn: ({ archivePath, profileId }) =>
      api.inspectPackageForInstall(archivePath, profileId),
  });
}

export function useExecuteOperation() {
  return useMutation<OperationDto, Error, string>({
    mutationFn: (operationId) => api.executeOperation(operationId),
  });
}

export function useInstallSmapi() {
  return useMutation<SmapiStatusDto, Error, string | undefined>({
    mutationFn: (gameId) => api.installPinnedSmapi(gameId),
  });
}

export function useLaunchPreflight(mode: string, enabled: boolean) {
  return useQuery<PreflightDto>({
    queryKey: ["launch-preflight", mode] as const,
    queryFn: () => api.getLaunchPreflight(mode),
    enabled,
    staleTime: 3000,
  });
}

/** Favourites, tags and notes, keyed by UniqueID across every profile. */
export function useModAnnotations() {
  return useQuery<ModAnnotationDto[]>({
    queryKey: ["mod-annotations"] as const,
    queryFn: () => api.listModAnnotations(),
    staleTime: 5000,
  });
}

export function useDismissedFindings() {
  return useQuery<DismissedFindingDto[]>({
    queryKey: ["dismissed-findings"] as const,
    queryFn: () => api.listDismissedFindings(),
    staleTime: 5000,
  });
}

export function useLaunchGame() {
  return useMutation<LaunchSessionDto, Error, string | undefined>({
    mutationFn: (mode) => api.launchActiveProfile(mode ?? "Modded"),
  });
}

export function useTerminateSession() {
  return useMutation<void, Error, string | undefined>({
    mutationFn: (sessionId) => api.terminateActiveLaunchSession(sessionId),
  });
}
