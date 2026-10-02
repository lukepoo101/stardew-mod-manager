use crate::error::{AppError, AppErrorCategory, Recoverability};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

mod bundle;
pub use bundle::*;
mod storage;
pub use storage::*;
mod annotations;
pub use annotations::*;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "BootstrapDto.ts")]
pub struct BootstrapDto {
    pub onboarding_disposition: String,
    pub active_game_installation_id: Option<String>,
    pub active_profile_id: Option<String>,
    pub recovery_summary: Option<String>,
    /// The interrupted operation behind `recovery_summary`, with what is
    /// known to have finished.
    pub recovery: Option<RecoveryDetailDto>,
    pub app_version: String,
}

/// An operation that was interrupted and needs recovery before the app can
/// be used normally.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "RecoveryDetailDto.ts")]
pub struct RecoveryDetailDto {
    pub operation_id: String,
    pub kind: String,
    pub state: String,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub started_at: String,
    pub steps: Vec<OperationStepDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "GameInstallationSummaryDto.ts")]
pub struct GameInstallationSummaryDto {
    pub id: String,
    pub canonical_root: String,
    pub operating_system: String,
    pub storefront: String,
    pub management_mode: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "GameInspectionDto.ts")]
pub struct GameInspectionDto {
    pub candidate_path: String,
    pub storefront: String,
    /// The platform the manager interpreted this directory as.
    pub operating_system: String,
    pub detected_version: Option<String>,
    pub support_state: String,
    pub is_usable: bool,
    pub has_existing_smapi: bool,
    pub has_existing_mods: bool,
    pub is_writable: bool,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ProfileSummaryDto.ts")]
pub struct ProfileSummaryDto {
    pub id: String,
    pub game_installation_id: String,
    pub name: String,
    pub description: Option<String>,
    #[ts(type = "number")]
    pub revision: u64,
    pub mod_count: usize,
    pub created_at: String,
    pub updated_at: String,
    pub state: String,
    /// Whether this is the game's default profile, used when no profile is
    /// explicitly active.
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "FindingDto.ts")]
pub struct FindingDto {
    pub id: String,
    pub fingerprint: String,
    pub code: String,
    pub severity: String,
    pub category: String,
    pub title: String,
    pub summary: String,
    pub affected_entities: Vec<String>,
    pub evidence: Vec<String>,
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "HealthSummaryDto.ts")]
pub struct HealthSummaryDto {
    pub status: String,
    pub warning_count: usize,
    pub error_count: usize,
    /// Findings that inform without asking for action.
    pub info_count: usize,
    pub findings: Vec<FindingDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "SmapiStatusDto.ts")]
pub struct SmapiStatusDto {
    pub is_installed: bool,
    pub observed_version: Option<String>,
    pub tested_version: String,
    pub is_compatible: bool,
    /// "absent", "installed" or "partial" (some SMAPI files are missing).
    pub state: String,
    /// How the installed version relates to the tested one: "same",
    /// "newer", "older", "unknown" (installed but unreadable) or "absent".
    pub comparison: String,
    /// What the state is based on.
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "LaunchSessionSummaryDto.ts")]
pub struct LaunchSessionSummaryDto {
    pub id: String,
    pub state: String,
    pub launched_at: String,
    pub verification_result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ProfileOverviewDto.ts")]
pub struct ProfileOverviewDto {
    pub profile: ProfileSummaryDto,
    pub game: GameInstallationSummaryDto,
    pub mod_count: usize,
    pub smapi_status: SmapiStatusDto,
    pub health_summary: HealthSummaryDto,
    pub last_session: Option<LaunchSessionSummaryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ModListItemDto.ts")]
pub struct ModListItemDto {
    pub profile_component_id: String,
    pub unique_id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub installed_reason: String,
    pub deployment_id: String,
    pub artifact_hash: String,
    pub installed_at: String,
    /// The mod's folder is not where the manager put it (moved or deleted
    /// outside the manager).
    #[serde(default)]
    pub folder_missing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ModDependencyDto.ts")]
pub struct ModDependencyDto {
    pub unique_id: String,
    pub minimum_version: Option<String>,
    pub is_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ContentPackForDto.ts")]
pub struct ContentPackForDto {
    pub unique_id: String,
    pub minimum_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ModDetailsDto.ts")]
pub struct ModDetailsDto {
    pub profile_component_id: String,
    pub unique_id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: Option<String>,
    pub entry_dll: Option<String>,
    pub minimum_api_version: Option<String>,
    pub minimum_game_version: Option<String>,
    pub update_keys: Vec<String>,
    pub dependencies: Vec<ModDependencyDto>,
    pub content_pack_for: Option<ContentPackForDto>,
    pub raw_manifest: String,
    pub artifact_hash: String,
    pub original_filename: Option<String>,
    pub deployment_root_path: String,
    pub installed_at: String,
    pub enabled: bool,
    /// How the package entered the manager, in plain words.
    #[serde(default)]
    pub source: Option<String>,
    /// When the manager first received the package.
    #[serde(default)]
    pub acquired_at: Option<String>,
    /// Earlier archives this mod was installed from in this profile, newest
    /// first, as "version (file)".
    #[serde(default)]
    pub earlier_versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "OperationDto.ts")]
pub struct OperationDto {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub game_installation_id: Option<String>,
    pub profile_id: Option<String>,
    pub progress_current: Option<u32>,
    pub progress_total: Option<u32>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    /// The operation failed and its live changes were undone by a completed
    /// compensation step, so nothing it started was left behind.
    pub rolled_back: bool,
    /// The larger change it was part of, such as reinstalling a mod.
    pub part_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "OperationStepDto.ts")]
pub struct OperationStepDto {
    pub step_index: u32,
    pub step_kind: String,
    pub state: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "OperationEffectDto.ts")]
pub struct OperationEffectDto {
    pub id: String,
    pub operation_id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub change_kind: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "PackageComponentPreviewDto.ts")]
pub struct PackageComponentPreviewDto {
    pub unique_id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: Option<String>,
    pub relative_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "OperationPreviewDto.ts")]
pub struct OperationPreviewDto {
    pub operation_id: String,
    pub artifact_hash: String,
    pub original_filename: String,
    #[ts(type = "number")]
    pub byte_size: u64,
    pub detected_components: Vec<PackageComponentPreviewDto>,
    pub dependencies_satisfied: bool,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
    pub affected_profile_component_ids: Vec<String>,
    #[ts(type = "number | null")]
    pub expected_profile_revision: Option<u64>,
    /// Installed mods this package would replace, when it holds a newer, older
    /// or the same version of something already in the profile.
    #[serde(default)]
    pub replaces: Vec<ReplacementDto>,
    /// Every file in the archive, and whether it will be installed. Files
    /// that are not installed stay in the stored archive.
    #[serde(default)]
    pub files: Vec<PlanFileDto>,
}

/// One file of an archive under review.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "PlanFileDto.ts")]
pub struct PlanFileDto {
    pub path: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "LaunchSessionDto.ts")]
pub struct LaunchSessionDto {
    pub id: String,
    pub profile_id: String,
    /// `modded`, `vanilla` or `runtime_test`.
    pub launch_mode: String,
    pub state: String,
    pub launched_at: String,
    pub ended_at: Option<String>,
    pub pid: Option<u32>,
    pub verified_mods: Vec<String>,
    pub verification_details: Option<String>,
    /// Non-blocking warnings the user reviewed and launched past.
    pub acknowledged_warnings: Vec<String>,
    /// UniqueIDs of the mods that were enabled when the session started.
    pub expected_mods: Vec<String>,
    /// Versions observed just before the game started; `None` when unknown.
    pub game_version: Option<String>,
    pub smapi_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "DiagnosticsDto.ts")]
pub struct DiagnosticsDto {
    /// Structured reading of `raw_log`; empty when the log could not be read.
    pub log_summary: LogSummaryDto,
    pub session_id: Option<String>,
    pub session_state: Option<String>,
    pub findings: Vec<FindingDto>,
    pub raw_log: String,
    pub log_file_path: String,
    /// The platform this build runs on, so a report always describes the code
    /// that produced it rather than what the UI assumes.
    pub host_operating_system: String,
    /// Where the manager keeps its database and profile storage.
    pub app_data_dir: String,
    /// Where the manager keeps downloaded artifacts and the SMAPI installer.
    pub cache_dir: String,
    /// The locations the Steam discovery searched on this host.
    pub steam_installations_checked: Vec<String>,
    /// Where the SMAPI log would live for each platform the manager supports,
    /// so a user can find it even when the manager is not the one that wrote it.
    pub smapi_log_locations: Vec<PlatformPathDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "SkippedModDto.ts")]
pub struct SkippedModDto {
    pub name: String,
    pub version: Option<String>,
    pub reason: String,
    pub missing_dependencies: Vec<String>,
    /// 1-based line in the SMAPI log.
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ModUpdateNoticeDto.ts")]
pub struct ModUpdateNoticeDto {
    pub name: String,
    pub current_version: String,
    pub available_version: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "LogSourceCountDto.ts")]
pub struct LogSourceCountDto {
    pub source: String,
    pub errors: usize,
    pub warnings: usize,
    pub first_error_line: Option<usize>,
}

/// What the SMAPI log says about the session, read without interpretation.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "LogSummaryDto.ts")]
pub struct LogSummaryDto {
    pub smapi_version: Option<String>,
    pub game_version: Option<String>,
    pub loaded_mod_count: Option<usize>,
    pub skipped_mods: Vec<SkippedModDto>,
    pub update_notices: Vec<ModUpdateNoticeDto>,
    pub sources: Vec<LogSourceCountDto>,
    pub total_lines: usize,
}

impl From<manager_core::smapi::LogSummary> for LogSummaryDto {
    fn from(summary: manager_core::smapi::LogSummary) -> Self {
        Self {
            smapi_version: summary.smapi_version,
            game_version: summary.game_version,
            loaded_mod_count: summary.loaded_mod_count,
            skipped_mods: summary
                .skipped_mods
                .into_iter()
                .map(|m| SkippedModDto {
                    name: m.name,
                    version: m.version,
                    reason: m.reason,
                    missing_dependencies: m.missing_dependencies,
                    line: m.line,
                })
                .collect(),
            update_notices: summary
                .update_notices
                .into_iter()
                .map(|n| ModUpdateNoticeDto {
                    name: n.name,
                    current_version: n.current_version,
                    available_version: n.available_version,
                    line: n.line,
                })
                .collect(),
            sources: summary
                .sources
                .into_iter()
                .map(|c| LogSourceCountDto {
                    source: c.source,
                    errors: c.errors,
                    warnings: c.warnings,
                    first_error_line: c.first_error_line,
                })
                .collect(),
            total_lines: summary.total_lines,
        }
    }
}

/// A platform-specific filesystem location, for diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "PlatformPathDto.ts")]
pub struct PlatformPathDto {
    pub operating_system: String,
    pub context: String,
    pub path: String,
}

/// The error contract that crosses the Tauri IPC boundary.
///
/// Every field is carried over from AppError unchanged, including the operation
/// ID that recovery-specific UX needs. Category and recoverability reuse the
/// application enums so the generated TypeScript contract stays a closed union
/// instead of an unconstrained string.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ApiErrorDto.ts")]
pub struct ApiErrorDto {
    pub code: String,
    pub category: AppErrorCategory,
    pub summary: String,
    pub technical_details: Option<String>,
    pub context: Option<String>,
    pub recoverability: Recoverability,
    pub operation_id: Option<String>,
}

impl From<AppError> for ApiErrorDto {
    fn from(error: AppError) -> Self {
        Self {
            code: error.code,
            category: error.category,
            summary: error.summary,
            technical_details: error.technical_details,
            context: error.context,
            recoverability: error.recoverability,
            operation_id: error.operation_id,
        }
    }
}

/// What deleting an archived profile will remove, shown before it happens.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ProfileDeletePreviewDto.ts")]
pub struct ProfileDeletePreviewDto {
    pub profile_id: String,
    pub name: String,
    pub mod_count: usize,
    /// Size of the profile's own folder (its installed mods and settings).
    #[ts(type = "number")]
    pub folder_bytes: u64,
    /// Packages the profile used. They are kept; other profiles may use them
    /// and storage cleanup can remove the unused ones later.
    pub packages_kept: usize,
    /// Why it cannot be deleted right now, if it cannot.
    pub blocked_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use manager_core::ids::OperationId;

    #[test]
    fn api_error_dto_preserves_every_structured_field() {
        let operation_id = OperationId::new();
        let error = AppError {
            code: "PROFILE_OPERATION_UNRESOLVED".to_string(),
            category: AppErrorCategory::OperationConflict,
            summary: "Profile has an unresolved operation".to_string(),
            technical_details: Some("unresolved operation 018f3a".to_string()),
            context: Some("profile 018f3b".to_string()),
            recoverability: Recoverability::RequiresManualIntervention,
            operation_id: Some(operation_id.to_string()),
        };

        let dto = ApiErrorDto::from(error);

        assert_eq!(dto.code, "PROFILE_OPERATION_UNRESOLVED");
        assert_eq!(dto.category, AppErrorCategory::OperationConflict);
        assert_eq!(dto.summary, "Profile has an unresolved operation");
        assert_eq!(
            dto.technical_details.as_deref(),
            Some("unresolved operation 018f3a")
        );
        assert_eq!(dto.context.as_deref(), Some("profile 018f3b"));
        assert_eq!(
            dto.recoverability,
            Recoverability::RequiresManualIntervention
        );
        assert_eq!(
            dto.operation_id.as_deref(),
            Some(operation_id.to_string().as_str())
        );
    }

    #[test]
    fn api_error_dto_serializes_using_the_snake_case_ipc_convention() {
        let dto = ApiErrorDto::from(AppError::preview_stale(17, 18));

        let serialized = serde_json::to_value(&dto).expect("serialize ApiErrorDto");

        assert_eq!(
            serialized,
            serde_json::json!({
                "code": "PREVIEW_STALE",
                "category": "operation_conflict",
                "summary": "Profile was modified since the preview was generated",
                "technical_details": "Expected profile revision 17, but current revision is 18",
                "context": null,
                "recoverability": "retry_with_fresh_plan",
                "operation_id": null,
            })
        );
    }

    #[test]
    fn api_error_dto_keeps_a_conflict_code_and_summary_apart() {
        let dto = ApiErrorDto::from(AppError::game_running("stop the game first"));

        assert_eq!(dto.code, "GAME_RUNNING");
        assert_eq!(dto.category, AppErrorCategory::OperationConflict);
        assert_eq!(dto.summary, "Stardew Valley is already running");
        assert_eq!(dto.recoverability, Recoverability::Retryable);
        assert_ne!(dto.summary, dto.code);
        assert_eq!(
            dto.technical_details.as_deref(),
            Some("stop the game first")
        );
    }

    #[test]
    fn api_error_dto_round_trips_optional_fields_and_recovery_category() {
        let operation_id = OperationId::new();
        let dto = ApiErrorDto::from(AppError::recovery_required(
            "RECOVERY_REQUIRED",
            "A previous operation needs manual reconciliation",
            operation_id,
        ));

        let serialized = serde_json::to_string(&dto).expect("serialize ApiErrorDto");
        let restored: ApiErrorDto =
            serde_json::from_str(&serialized).expect("deserialize ApiErrorDto");

        assert_eq!(restored.category, AppErrorCategory::Recovery);
        assert_eq!(
            restored.recoverability,
            Recoverability::RequiresManualIntervention
        );
        assert_eq!(
            restored.operation_id.as_deref(),
            Some(operation_id.to_string().as_str())
        );
        assert_eq!(restored.technical_details, None);
        assert_eq!(restored.context, None);
    }
}

/// The state of a guided fault-isolation session.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "TroubleshootDto.ts")]
pub struct TroubleshootDto {
    pub active: bool,
    /// `all_off`, `testing`, `found` or `inconclusive`.
    pub phase: String,
    pub step: u32,
    /// Names of the mods that may still be the cause.
    pub suspects: Vec<String>,
    /// Names of the mods enabled for the test in progress.
    pub enabled_mods: Vec<String>,
    pub culprit: Option<String>,
    pub note: Option<String>,
}

impl TroubleshootDto {
    pub fn inactive() -> Self {
        Self {
            active: false,
            phase: "inactive".to_string(),
            step: 0,
            suspects: Vec::new(),
            enabled_mods: Vec::new(),
            culprit: None,
            note: None,
        }
    }
}

/// A finding the user chose to stop seeing, and what it said when they did.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "DismissedFindingDto.ts")]
pub struct DismissedFindingDto {
    pub fingerprint: String,
    /// Digest of the finding's content. A finding whose content differs from
    /// this is shown again.
    pub signature: String,
    /// What the finding said when it was dismissed, so a finding that comes
    /// back can say what changed. Absent for dismissals recorded before this
    /// was kept.
    pub previous: Option<DismissedSnapshotDto>,
}

/// The parts of a dismissed finding that are compared when it comes back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "DismissedSnapshotDto.ts")]
pub struct DismissedSnapshotDto {
    pub severity: String,
    pub summary: String,
    pub evidence: Vec<String>,
}

/// What happened to each mod in a bulk enable or disable.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "BulkToggleResultDto.ts")]
pub struct BulkToggleResultDto {
    /// Names of the mods now in the requested state.
    pub changed: Vec<String>,
    pub failed: Vec<BulkToggleFailureDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "BulkToggleFailureDto.ts")]
pub struct BulkToggleFailureDto {
    pub name: String,
    pub message: String,
}

/// What enabling or disabling a mod would touch, shown before it happens.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ToggleImpactDto.ts")]
pub struct ToggleImpactDto {
    /// Every mod that moves with it because they share one package folder.
    pub affected_mods: Vec<String>,
    /// Enabled mods that require it and would stop loading when it is disabled.
    pub dependents: Vec<String>,
    /// Requirements of what is being enabled that are not enabled.
    pub unmet_requirements: Vec<String>,
}

/// What a launch would be blocked or cautioned by, before the user clicks.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "PreflightDto.ts")]
pub struct PreflightDto {
    pub can_launch: bool,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 0,
        "error" => 1,
        "warning" => 2,
        "info" => 3,
        "recommendation" => 4,
        // An unfamiliar severity is never filed below a known harmless one.
        _ => 1,
    }
}

/// Orders findings so the most consequential come first, deterministically:
/// severity, then category, then fingerprint.
pub fn sort_findings(findings: &mut [FindingDto]) {
    findings.sort_by(|a, b| {
        severity_rank(&a.severity)
            .cmp(&severity_rank(&b.severity))
            .then_with(|| a.category.cmp(&b.category))
            .then_with(|| a.fingerprint.cmp(&b.fingerprint))
    });
}

#[cfg(test)]
mod finding_order_tests {
    use super::*;

    fn finding(severity: &str, category: &str, fingerprint: &str) -> FindingDto {
        FindingDto {
            id: fingerprint.to_string(),
            fingerprint: fingerprint.to_string(),
            code: fingerprint.to_string(),
            severity: severity.to_string(),
            category: category.to_string(),
            title: String::new(),
            summary: String::new(),
            affected_entities: Vec::new(),
            evidence: Vec::new(),
            observed_at: String::new(),
        }
    }

    #[test]
    fn findings_are_ordered_by_impact_then_category_then_fingerprint() {
        let mut findings = vec![
            finding("info", "runtime", "a"),
            finding("warning", "runtime", "b"),
            finding("critical", "recovery", "c"),
            finding("error", "dependency", "z"),
            finding("error", "dependency", "y"),
            finding("mystery", "runtime", "m"),
        ];
        sort_findings(&mut findings);
        let order: Vec<_> = findings.iter().map(|f| f.fingerprint.as_str()).collect();
        assert_eq!(order, ["c", "y", "z", "m", "b", "a"]);
    }
}

/// A profile held at the mods and versions it had when it was frozen.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ProfileFreezeDto.ts")]
pub struct ProfileFreezeDto {
    pub profile_id: String,
    pub frozen_at: String,
    pub reason: String,
    pub mods: Vec<FrozenModDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "FrozenModDto.ts")]
pub struct FrozenModDto {
    pub unique_id: String,
    pub name: String,
    pub version: String,
    pub artifact_hash: String,
    pub enabled: bool,
}

/// Why a mod is in the profile and how it relates to the others.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ModRelationsDto.ts")]
pub struct ModRelationsDto {
    /// "direct", "dependency" or "bundle_companion".
    pub installed_reason: String,
    /// The reason in plain words, including what the manager does not know.
    pub reason_detail: String,
    pub requires: Vec<ModRequirementDto>,
    pub required_by: Vec<ModDependentDto>,
    /// Chains such as ["Top", "Middle", "Missing.UniqueID"], each ending at a
    /// requirement that is not satisfied. Names are used where known.
    pub broken_chains: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ModRequirementDto.ts")]
pub struct ModRequirementDto {
    pub unique_id: String,
    /// The name of the mod in the profile that has this UniqueID.
    pub name: Option<String>,
    pub installed_version: Option<String>,
    pub minimum_version: Option<String>,
    /// "required", "optional" or "content_pack_for".
    pub kind: String,
    /// "satisfied", "missing", "disabled" or "too_old".
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ModDependentDto.ts")]
pub struct ModDependentDto {
    pub profile_component_id: String,
    pub name: String,
    pub unique_id: String,
    pub enabled: bool,
    pub minimum_version: Option<String>,
    /// "required", "optional" or "content_pack_for".
    pub kind: String,
}

/// What one operation did, for the Activity history.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "OperationDetailsDto.ts")]
pub struct OperationDetailsDto {
    pub operation_id: String,
    pub profile_name: Option<String>,
    /// The archive an install came from.
    pub original_filename: Option<String>,
    pub package_hash: Option<String>,
    /// The mod folder an install created or a removal took away.
    pub folder: Option<String>,
    pub changes: Vec<OperationChangeDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "OperationChangeDto.ts")]
pub struct OperationChangeDto {
    /// "added", "removed", "profile_created", "copied_from" or the recorded kind.
    pub change: String,
    pub name: Option<String>,
    pub unique_id: Option<String>,
    pub version: Option<String>,
}

/// A profile made to try changes without touching the one it came from.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ExperimentDto.ts")]
pub struct ExperimentDto {
    pub profile_id: String,
    pub source_profile_id: String,
    pub source_name: String,
    /// The source's revision when the experiment was made.
    #[ts(type = "number")]
    pub source_revision: u64,
    pub created_at: String,
}

/// A Stardew Valley save, its usual profile and the manager's backups of it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "SaveDto.ts")]
pub struct SaveDto {
    pub id: String,
    pub farm_name: Option<String>,
    pub farmer_name: Option<String>,
    pub game_version: Option<String>,
    pub modified_at: Option<String>,
    #[ts(type = "number")]
    pub size_bytes: u64,
    /// The profile the user said this save belongs with.
    pub profile_id: Option<String>,
    /// Its name, or None when that profile no longer exists.
    pub profile_name: Option<String>,
    pub backups: Vec<SaveBackupDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "SaveBackupDto.ts")]
pub struct SaveBackupDto {
    pub id: String,
    pub save_id: String,
    pub created_at: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "SavesDto.ts")]
pub struct SavesDto {
    /// Where the game keeps saves on this computer, if it could be found.
    pub saves_dir: Option<String>,
    pub saves: Vec<SaveDto>,
    /// Links to saves that are no longer found (moved, renamed or deleted).
    /// They are kept, never removed automatically.
    #[serde(default)]
    pub unavailable_links: Vec<SaveLinkDto>,
}

/// A save linked to a profile.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "SaveLinkDto.ts")]
pub struct SaveLinkDto {
    pub save_id: String,
    pub profile_id: String,
    /// The profile's name, or `None` when it no longer exists.
    pub profile_name: Option<String>,
}

/// The mods and runtime a profile last ran successfully with.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "KnownGoodDto.ts")]
pub struct KnownGoodDto {
    pub profile_id: String,
    pub recorded_at: String,
    pub game_version: Option<String>,
    pub smapi_version: Option<String>,
    pub mods: Vec<FrozenModDto>,
    /// The health findings at that moment. `None` for records made before
    /// findings were kept, so there is nothing to compare with.
    #[serde(default)]
    pub findings: Option<Vec<BaselineFindingDto>>,
}

/// One health finding as it was when a profile last worked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "BaselineFindingDto.ts")]
pub struct BaselineFindingDto {
    pub fingerprint: String,
    pub code: String,
    pub severity: String,
    pub title: String,
}

/// How a deployed mod's files compare with what was installed.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ModFilesCheckDto.ts")]
pub struct ModFilesCheckDto {
    pub deployment_id: String,
    /// Names of the mods in this folder.
    pub mods: Vec<String>,
    /// "unchanged", "changed", "missing_folder" or "no_record".
    pub status: String,
    /// Installed files that are gone.
    pub missing: Vec<String>,
    /// Installed files whose contents are different now (config.json excluded).
    pub modified: Vec<String>,
    /// Files that were not installed. Mods often create config.json themselves.
    pub added: Vec<String>,
    /// config.json files that differ from the installed copy; editing them is normal.
    pub config_changed: Vec<String>,
}

/// A shared recipe a profile is kept in step with, and the differences the
/// user has accepted for their group.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ReferenceRecipeDto.ts")]
pub struct ReferenceRecipeDto {
    pub recipe_json: String,
    pub attached_at: String,
    /// Keys of differences accepted as fine for this group. A key names both
    /// versions, so a changed difference is shown again.
    pub accepted: Vec<String>,
}

/// What a clean reinstall did.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ReinstallResultDto.ts")]
pub struct ReinstallResultDto {
    /// The mods reinstalled from the package, with versions.
    pub mods: Vec<String>,
    /// Settings files carried over from the old copy.
    pub kept_settings: Vec<String>,
    /// True when the mod was disabled before and was left disabled.
    pub left_disabled: bool,
    /// Where the settings were saved before the change, when there were any.
    #[serde(default)]
    pub settings_backup: Option<String>,
    /// The restore point saved automatically before the change.
    #[serde(default)]
    pub restore_point: Option<String>,
}

/// One installed mod a package would replace.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ReplacementDto.ts")]
pub struct ReplacementDto {
    pub profile_component_id: String,
    pub unique_id: String,
    pub name: String,
    pub installed_version: String,
    pub incoming_version: String,
    /// "upgrade", "downgrade", "same", or "different" when a version cannot
    /// be ordered.
    pub direction: String,
}

/// What replacing installed mods with another version did.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ReplaceResultDto.ts")]
pub struct ReplaceResultDto {
    pub replaced: Vec<ReplacementDto>,
    pub kept_settings: Vec<String>,
    pub left_disabled: bool,
    /// Where the settings were saved before the change, when there were any.
    #[serde(default)]
    pub settings_backup: Option<String>,
    /// The restore point saved automatically before the change.
    #[serde(default)]
    pub restore_point: Option<String>,
}

/// Space the manager uses. A null size could not be read.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "StorageUsageDto.ts")]
pub struct StorageUsageDto {
    pub profiles: Vec<ProfileStorageDto>,
    #[ts(type = "number | null")]
    pub packages_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub installer_cache_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub save_backups_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub trash_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ProfileStorageDto.ts")]
pub struct ProfileStorageDto {
    pub profile_id: String,
    pub name: String,
    pub archived: bool,
    /// Mods SMAPI loads.
    #[ts(type = "number | null")]
    pub live_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub disabled_bytes: Option<u64>,
    /// Prepared files and undo copies of operations.
    #[ts(type = "number | null")]
    pub operations_bytes: Option<u64>,
}

/// A saved copy of a mod's settings files.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ConfigBackupDto.ts")]
pub struct ConfigBackupDto {
    pub id: String,
    pub created_at: String,
    pub files: Vec<String>,
}

/// A mod whose required dependencies are not all met.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ModProblemDto.ts")]
pub struct ModProblemDto {
    pub profile_component_id: String,
    /// UniqueIDs of required mods or hosts that are missing, disabled or too old.
    pub unmet_requirements: Vec<String>,
}

/// A saved state of a profile's mods that it can be restored to.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "RestorePointDto.ts")]
pub struct RestorePointDto {
    pub id: String,
    pub label: String,
    pub created_at: String,
    pub mods: Vec<FrozenModDto>,
    /// For a point saved automatically before a change: the operations that
    /// change was made of, in order, as recorded in Activity.
    #[serde(default)]
    pub operations: Vec<String>,
}

/// What restoring a point would do, worked out before anything changes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "RestorePlanDto.ts")]
pub struct RestorePlanDto {
    pub point_id: String,
    /// False when a package the point needs is no longer kept or is damaged;
    /// the point is then not restored at all.
    pub available: bool,
    pub unavailable: Vec<String>,
    pub remove: Vec<String>,
    pub install: Vec<String>,
    pub change_version: Vec<String>,
    pub enable: Vec<String>,
    pub disable: Vec<String>,
}

/// What a restore did, step by step.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "RestoreResultDto.ts")]
pub struct RestoreResultDto {
    /// The restore point saved from the state before this restore.
    pub undo_point_id: String,
    pub done: Vec<String>,
    pub failed: Vec<String>,
}

/// A mod's settings files that could be included in a bundle.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ShareableSettingsDto.ts")]
pub struct ShareableSettingsDto {
    pub unique_id: String,
    pub name: String,
    pub files: Vec<String>,
    /// What the files might reveal if shared.
    pub warnings: Vec<String>,
}

/// How much recovery data storage cleanup keeps. Older items become
/// removable; nothing is removed until a cleanup is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "RetentionPolicyDto.ts")]
pub struct RetentionPolicyDto {
    /// Newest backups kept for each save.
    pub keep_save_backups: u32,
    /// Newest backups kept for each mod's settings.
    pub keep_settings_backups: u32,
    /// Days a deleted profile's folder stays in the trash.
    pub keep_trash_days: u32,
}

impl Default for RetentionPolicyDto {
    fn default() -> Self {
        Self {
            keep_save_backups: 5,
            keep_settings_backups: 5,
            keep_trash_days: 30,
        }
    }
}

/// How much space one installed mod takes. `None` when it could not be read
/// or, for the archive, when it is no longer stored.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export, export_to = "ModSizeDto.ts")]
pub struct ModSizeDto {
    #[ts(type = "number | null")]
    pub folder_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub archive_bytes: Option<u64>,
}

/// One place on disk the user may want to find, with what it is for.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "LocationDto.ts")]
pub struct LocationDto {
    /// Stable id, used to open it (the path itself is never sent back).
    pub id: String,
    pub label: String,
    /// `None` when it is not known, for example no game is selected.
    pub path: Option<String>,
    pub exists: bool,
    /// What it holds, and whether it is safe to clear.
    pub note: String,
}

/// One location setup needs, and whether it can be used.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "SetupAccessCheckDto.ts")]
pub struct SetupAccessCheckDto {
    pub label: String,
    pub path: String,
    /// "read and write" etc., in words.
    pub needs: String,
    pub ok: bool,
    pub problem: Option<String>,
    /// What to do about a failure, without asking for administrator rights.
    pub remedy: Option<String>,
}

/// What SMAPI setup will do, worked out from the same release policy and
/// locations the installation uses, before anything is changed.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "SetupPreviewDto.ts")]
pub struct SetupPreviewDto {
    pub game_path: String,
    pub smapi_version: String,
    pub smapi_source: String,
    pub smapi_sha256: String,
    pub supported_game_version: String,
    /// SMAPI already in the game folder, if any.
    pub installed_smapi: Option<String>,
    /// Changes to the game folder.
    pub modifies: Vec<String>,
    /// Folders the manager creates and owns.
    pub creates: Vec<String>,
    /// What is only looked at.
    pub reads: Vec<String>,
    /// Things already present that matter to setup.
    pub notices: Vec<String>,
    pub checks: Vec<SetupAccessCheckDto>,
    /// False when a required check failed; setup then does not start.
    pub can_proceed: bool,
}
