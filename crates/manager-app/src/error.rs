use manager_core::ids::OperationId;
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

#[derive(Debug, Clone, Error, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "AppError.ts")]
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

    pub fn conflict(summary: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            code: "OPERATION_CONFLICT".to_string(),
            category: AppErrorCategory::OperationConflict,
            summary: summary.into(),
            technical_details: Some(details.into()),
            context: None,
            recoverability: Recoverability::RetryWithFreshPlan,
            operation_id: None,
        }
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
