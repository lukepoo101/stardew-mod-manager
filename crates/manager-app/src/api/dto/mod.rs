use crate::error::{AppError, AppErrorCategory, Recoverability};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "BootstrapDto.ts")]
pub struct BootstrapDto {
    pub onboarding_disposition: String,
    pub active_game_installation_id: Option<String>,
    pub active_profile_id: Option<String>,
    pub recovery_summary: Option<String>,
    pub app_version: String,
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
    pub findings: Vec<FindingDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "SmapiStatusDto.ts")]
pub struct SmapiStatusDto {
    pub is_installed: bool,
    pub observed_version: Option<String>,
    pub tested_version: String,
    pub is_compatible: bool,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "LaunchSessionDto.ts")]
pub struct LaunchSessionDto {
    pub id: String,
    pub profile_id: String,
    pub state: String,
    pub launched_at: String,
    pub ended_at: Option<String>,
    pub pid: Option<u32>,
    pub verified_mods: Vec<String>,
    pub verification_details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "DiagnosticsDto.ts")]
pub struct DiagnosticsDto {
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
