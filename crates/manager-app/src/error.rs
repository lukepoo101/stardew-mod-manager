use manager_core::ids::{OperationId, ProfileId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "AppErrorCategory.ts")]
#[serde(rename_all = "snake_case")]
pub enum AppErrorCategory {
    Validation,
    Filesystem,
    Permission,
    Storage,
    Dependency,
    Runtime,
    Provider,
    Network,
    OperationConflict,
    Recovery,
    Unsupported,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "Recoverability.ts")]
#[serde(rename_all = "snake_case")]
pub enum Recoverability {
    Terminal,
    Retryable,
    RetryWithFreshPlan,
    RequiresManualIntervention,
}

/// Internal application-layer error.
///
/// This type is not part of the frontend IPC contract: commands convert it into
/// ApiErrorDto at the Tauri boundary, and only the category/recoverability enums
/// plus the DTO are exported as generated TypeScript.
#[derive(Debug, Clone, Error, Serialize, Deserialize)]
#[error("{summary} ({code})")]
pub struct AppError {
    pub code: String,
    pub category: AppErrorCategory,
    pub summary: String,
    pub technical_details: Option<String>,
    pub context: Option<String>,
    pub recoverability: Recoverability,
    pub operation_id: Option<String>,
}

impl AppError {
    pub fn new(
        code: impl Into<String>,
        category: AppErrorCategory,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            category,
            summary: summary.into(),
            technical_details: None,
            context: None,
            recoverability: Recoverability::Terminal,
            operation_id: None,
        }
    }

    pub fn validation(code: impl Into<String>, summary: impl Into<String>) -> Self {
        Self::new(code, AppErrorCategory::Validation, summary)
    }

    pub fn filesystem(summary: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            code: "FILESYSTEM_ERROR".to_string(),
            category: AppErrorCategory::Filesystem,
            summary: summary.into(),
            technical_details: Some(details.into()),
            context: None,
            recoverability: Recoverability::Terminal,
            operation_id: None,
        }
    }

    /// A request that is well formed but conflicts with the current state of the
    /// world, so it cannot proceed until something else changes.
    ///
    /// The four inputs are deliberately separate: the code is the stable program
    /// contract, the summary is the user-facing message, the details are
    /// diagnostic evidence, and the recoverability states how the caller may get
    /// unstuck. Prefer a named constructor below when the condition already
    /// exists; this generic form is the seam for new conflict conditions.
    pub fn conflict(
        code: impl Into<String>,
        summary: impl Into<String>,
        details: impl Into<String>,
        recoverability: Recoverability,
    ) -> Self {
        Self {
            code: code.into(),
            category: AppErrorCategory::OperationConflict,
            summary: summary.into(),
            technical_details: Some(details.into()),
            context: None,
            recoverability,
            operation_id: None,
        }
    }

    /// Another process holds the cross-process instance lock.
    ///
    /// Nothing is wrong with the request itself, so it stays retryable once the
    /// other process is done.
    pub fn instance_locked(details: impl Into<String>) -> Self {
        Self::conflict(
            "INSTANCE_LOCKED",
            "Another instance of Stardew Mod Manager is using these files",
            details,
            Recoverability::Retryable,
        )
    }

    /// The game is running, so managed files must not change.
    ///
    /// Closing the game and retrying the same request is meaningful, so this
    /// stays retryable rather than asking for a fresh plan.
    pub fn game_running(details: impl Into<String>) -> Self {
        Self::conflict(
            "GAME_RUNNING",
            "Stardew Valley is already running",
            details,
            Recoverability::Retryable,
        )
    }

    /// The profile carries an unresolved operation from an earlier attempt.
    ///
    /// Regenerating a preview cannot clear this: the unresolved operation must
    /// be reconciled first, so the frontend is told to send the user to recovery
    /// instead of retrying the mutation.
    pub fn profile_operation_unresolved(
        profile_id: &ProfileId,
        operation_id: &OperationId,
    ) -> Self {
        Self::conflict(
            "PROFILE_OPERATION_UNRESOLVED",
            "This profile has an unresolved operation that must be reconciled before it can change",
            format!(
                "Profile {} has unresolved operation {}; reconcile it before mutating the profile",
                profile_id, operation_id
            ),
            Recoverability::RequiresManualIntervention,
        )
    }

    /// The operation already entered its mutation lifecycle.
    ///
    /// Cancelling the same request again cannot succeed, so it is terminal.
    pub fn operation_not_cancellable(operation_id: &OperationId) -> Self {
        Self::conflict(
            "OPERATION_NOT_CANCELLABLE",
            "This operation is already running and can no longer be cancelled",
            format!(
                "Operation {} is already in the mutation lifecycle",
                operation_id
            ),
            Recoverability::Terminal,
        )
    }

    /// The prepared plan expects a profile revision that is no longer current.
    ///
    /// The commit itself is fine, so the recovery is to regenerate the preview
    /// and retry against the new revision.
    pub fn preview_stale(expected_revision: u64, current_revision: u64) -> Self {
        Self::conflict(
            "PREVIEW_STALE",
            "Profile was modified since the preview was generated",
            format!(
                "Expected profile revision {}, but current revision is {}",
                expected_revision, current_revision
            ),
            Recoverability::RetryWithFreshPlan,
        )
    }

    pub fn system(code: impl Into<String>, summary: impl Into<String>) -> Self {
        Self::new(code, AppErrorCategory::Internal, summary)
    }

    pub fn network(code: impl Into<String>, summary: impl Into<String>) -> Self {
        Self::new(code, AppErrorCategory::Network, summary)
    }

    pub fn storage(code: impl Into<String>, summary: impl Into<String>) -> Self {
        Self::new(code, AppErrorCategory::Storage, summary)
    }

    pub fn runtime(code: impl Into<String>, summary: impl Into<String>) -> Self {
        Self::new(code, AppErrorCategory::Runtime, summary)
    }

    pub fn recovery_required(
        code: impl Into<String>,
        summary: impl Into<String>,
        op_id: OperationId,
    ) -> Self {
        Self {
            code: code.into(),
            category: AppErrorCategory::Recovery,
            summary: summary.into(),
            technical_details: None,
            context: None,
            recoverability: Recoverability::RequiresManualIntervention,
            operation_id: Some(op_id.to_string()),
        }
    }

    pub fn internal(summary: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            code: "INTERNAL_ERROR".to_string(),
            category: AppErrorCategory::Internal,
            summary: summary.into(),
            technical_details: Some(details.into()),
            context: None,
            recoverability: Recoverability::Terminal,
            operation_id: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.technical_details = Some(details.into());
        self
    }

    pub fn with_operation_id(mut self, op_id: OperationId) -> Self {
        self.operation_id = Some(op_id.to_string());
        self
    }

    pub fn with_recoverability(mut self, rec: Recoverability) -> Self {
        self.recoverability = rec;
        self
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_conditions_keep_code_prose_and_recoverability_separate() {
        let profile_id = ProfileId::new();
        let operation_id = OperationId::new();

        let cases = [
            (
                AppError::game_running("Stop Stardew Valley before changing managed files"),
                "GAME_RUNNING",
                Recoverability::Retryable,
            ),
            (
                AppError::instance_locked("lock file is held by another process"),
                "INSTANCE_LOCKED",
                Recoverability::Retryable,
            ),
            (
                AppError::profile_operation_unresolved(&profile_id, &operation_id),
                "PROFILE_OPERATION_UNRESOLVED",
                Recoverability::RequiresManualIntervention,
            ),
            (
                AppError::operation_not_cancellable(&operation_id),
                "OPERATION_NOT_CANCELLABLE",
                Recoverability::Terminal,
            ),
            (
                AppError::preview_stale(17, 18),
                "PREVIEW_STALE",
                Recoverability::RetryWithFreshPlan,
            ),
        ];

        for (error, expected_code, expected_recoverability) in cases {
            assert_eq!(error.code, expected_code);
            assert_eq!(error.category, AppErrorCategory::OperationConflict);
            assert_eq!(error.recoverability, expected_recoverability);
            assert_ne!(
                error.summary, expected_code,
                "{expected_code} must carry user-facing prose, not the machine code"
            );
            assert!(
                error.summary.chars().next().is_some_and(char::is_uppercase),
                "{expected_code} summary should read as a sentence"
            );
            assert!(
                error
                    .technical_details
                    .as_deref()
                    .is_some_and(|details| !details.is_empty()),
                "{expected_code} keeps diagnostic evidence"
            );
        }
    }

    #[test]
    fn only_the_stale_preview_conflict_asks_for_a_fresh_plan() {
        let profile_id = ProfileId::new();
        let operation_id = OperationId::new();

        let conflicts = [
            AppError::game_running("stop the game"),
            AppError::instance_locked("lock held"),
            AppError::profile_operation_unresolved(&profile_id, &operation_id),
            AppError::operation_not_cancellable(&operation_id),
        ];

        for error in conflicts {
            assert_ne!(
                error.recoverability,
                Recoverability::RetryWithFreshPlan,
                "{} must not be presented as a stale plan",
                error.code
            );
        }

        assert_eq!(
            AppError::preview_stale(17, 18).recoverability,
            Recoverability::RetryWithFreshPlan
        );
    }

    #[test]
    fn preview_stale_reports_both_revisions_as_diagnostics() {
        let error = AppError::preview_stale(17, 18);

        assert_eq!(error.code, "PREVIEW_STALE");
        assert_eq!(
            error.summary,
            "Profile was modified since the preview was generated"
        );
        assert_eq!(
            error.technical_details.as_deref(),
            Some("Expected profile revision 17, but current revision is 18")
        );
        assert_eq!(error.operation_id, None);
    }

    #[test]
    fn unresolved_operation_ids_stay_out_of_the_summary() {
        let profile_id = ProfileId::new();
        let operation_id = OperationId::new();
        let error = AppError::profile_operation_unresolved(&profile_id, &operation_id);

        let details = error.technical_details.unwrap_or_default();
        assert!(details.contains(&profile_id.to_string()));
        assert!(details.contains(&operation_id.to_string()));
        assert!(!error.summary.contains(&operation_id.to_string()));
        assert!(!error.summary.contains(&profile_id.to_string()));
    }
}
