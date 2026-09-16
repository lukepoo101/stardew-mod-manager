use crate::operation::model::OperationState;

/// Validates whether a state transition is legal according to the operation lifecycle.
pub fn is_valid_transition(from: OperationState, to: OperationState) -> bool {
    if from == to {
        return true;
    }

    match from {
        OperationState::Draft => matches!(
            to,
            OperationState::Prepared | OperationState::Cancelled | OperationState::Failed
        ),
        OperationState::Prepared => matches!(
            to,
            OperationState::Running
                | OperationState::CancellationRequested
                | OperationState::Cancelled
                | OperationState::Failed
        ),
        OperationState::Running => matches!(
            to,
            OperationState::Committing
                | OperationState::CancellationRequested
                | OperationState::Cancelling
                | OperationState::Failed
                | OperationState::RecoveryRequired
        ),
        OperationState::Committing => matches!(
            to,
            OperationState::Succeeded | OperationState::Failed | OperationState::RecoveryRequired
        ),
        OperationState::CancellationRequested => matches!(
            to,
            OperationState::Cancelling | OperationState::Cancelled | OperationState::Failed
        ),
        OperationState::Cancelling => matches!(
            to,
            OperationState::Cancelled | OperationState::Failed | OperationState::RecoveryRequired
        ),
        OperationState::RollingBack => matches!(
            to,
            OperationState::RolledBack | OperationState::Failed | OperationState::RecoveryRequired
        ),
        OperationState::RecoveryRequired => matches!(
            to,
            OperationState::Succeeded
                | OperationState::Prepared
                | OperationState::Running
                | OperationState::Committing
                | OperationState::RollingBack
                | OperationState::Cancelled
                | OperationState::Failed
        ),
        OperationState::Succeeded | OperationState::Cancelled | OperationState::RolledBack => false,
        OperationState::Failed => {
            matches!(to, OperationState::RecoveryRequired | OperationState::Draft)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_operation_transitions() {
        assert!(is_valid_transition(
            OperationState::Draft,
            OperationState::Prepared
        ));
        assert!(is_valid_transition(
            OperationState::Prepared,
            OperationState::Running
        ));
        assert!(is_valid_transition(
            OperationState::Running,
            OperationState::Committing
        ));
        assert!(is_valid_transition(
            OperationState::Committing,
            OperationState::Succeeded
        ));
        assert!(is_valid_transition(
            OperationState::RecoveryRequired,
            OperationState::Succeeded
        ));

        // Direct Draft to Succeeded is illegal
        assert!(!is_valid_transition(
            OperationState::Draft,
            OperationState::Succeeded
        ));
        // Succeeded is terminal
        assert!(!is_valid_transition(
            OperationState::Succeeded,
            OperationState::Running
        ));
    }
}
