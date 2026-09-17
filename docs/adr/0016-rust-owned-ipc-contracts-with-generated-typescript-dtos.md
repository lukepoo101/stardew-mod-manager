# ADR-0016: Rust-Owned IPC Contracts with Generated TypeScript DTOs

## Status
Accepted — generated DTO contract and the typed error boundary are implemented

## Date
2026-09-13

## Context
In the MVP, TypeScript type definitions in `apps/desktop/src/lib/backend/types.ts` were manually duplicated from Rust structs in `manager-core::domain`. 

As the backend model evolves to support rich profiles, package artifacts, acquisitions, components, deployments, durable operations, step journals, health findings, and diagnostics, manually keeping types in sync becomes error-prone and risks runtime deserialization bugs. Furthermore, exposing internal domain aggregates directly across the IPC boundary couples frontend views to backend storage refactoring.

## Decision
Establish Rust-owned IPC contract DTOs and automate TypeScript type generation using **`ts-rs`**:

### 1. Dedicated IPC DTOs in `manager-app`
Create UI-oriented Data Transfer Objects in `crates/manager-app/src/api/dto/`:
- `BootstrapDto`
- `GameInstallationSummaryDto`
- `GameInspectionDto`
- `ProfileSummaryDto`
- `ProfileOverviewDto`
- `ModListItemDto`
- `ModDetailsDto`
- `OperationDto`
- `OperationStepDto`
- `OperationEffectDto`
- `OperationPreviewDto`
- `FindingDto`
- `SmapiStatusDto`
- `LaunchSessionDto`
- `DiagnosticsDto`
- `ApiErrorDto`

Domain entities are converted to DTOs in the application layer or queries before crossing the Tauri IPC boundary. Internal domain refactorings do not leak into the frontend contract.

### 2. Automated Generation via `ts-rs`
Annotate DTOs with `#[derive(Serialize, Deserialize, TS)]` and `#[ts(export)]`.
Generate TypeScript definitions into:
```text
apps/desktop/src/shared/api/generated/
```
A dedicated test / CLI generator (`cargo test export_typescript_bindings`) exports these files deterministically.

### 3. CI Drift Detection
CI runs the export tool and validates `git diff --exit-code apps/desktop/src/shared/api/generated/`. Any commit that changes Rust DTOs without checking in the corresponding updated TypeScript definitions fails CI.

### 4. Typed Error Boundary
Generic string errors are replaced across IPC. A failed command returns a serialized `ApiErrorDto` whose `category` and `recoverability` fields are the generated `AppErrorCategory` and `Recoverability` unions:

```json
{
  "code": "PREVIEW_STALE",
  "category": "operation_conflict",
  "summary": "Profile was modified since the preview was generated",
  "technical_details": "Expected profile revision 17, but current revision is 18",
  "context": null,
  "recoverability": "retry_with_fresh_plan",
  "operation_id": null
}
```

Fields and enum values use the workspace-wide snake_case IPC convention; the example above is the serialization that ships. The frontend formats user-facing messages from `summary` and determines recovery options from `code`, `recoverability` and `operation_id` without parsing arbitrary error strings.

The three text fields have distinct jobs and must not be substituted for one another: `code` is the stable program contract, `summary` is always user-facing prose, and `technical_details` carries diagnostics (filesystem paths, parser output, database detail) that are never the primary user-facing message. A machine code is never used as the summary, and recoverability states how the caller gets unstuck rather than being a property of the error category.

Conflicts are a family of distinct conditions, not one generic failure:

| Code | Summary | Category | Recoverability |
| --- | --- | --- | --- |
| `PREVIEW_STALE` | Profile was modified since the preview was generated | `operation_conflict` | `retry_with_fresh_plan` |
| `PROFILE_REVISION_MISMATCH` | Profile was modified since the preview was generated | `operation_conflict` | `retry_with_fresh_plan` |
| `PROFILE_OPERATION_UNRESOLVED` | This profile has an unresolved operation that must be reconciled before it can change | `operation_conflict` | `requires_manual_intervention` |
| `DEPLOYMENT_DESTINATION_EXISTS` | The profile already has a folder where this mod would be published | `operation_conflict` | `requires_manual_intervention` |
| `GAME_RUNNING` | Stardew Valley is already running | `operation_conflict` | `retryable` |
| `INSTANCE_LOCKED` | Another instance of Stardew Mod Manager is using these files | `operation_conflict` | `retryable` |
| `OPERATION_NOT_CANCELLABLE` | This operation is already running and can no longer be cancelled | `operation_conflict` | `terminal` |
| `INVALID_OPERATION_STATE`, `INVALID_OPERATION_TRANSITION` | The operation is no longer in the state this step requires | `operation_conflict` | `terminal` |

`PREVIEW_STALE` is the application-layer preflight check and `PROFILE_REVISION_MISMATCH` is the same condition detected inside the atomic database mutation; both ask the caller to regenerate the preview rather than to retry the same plan.

#### Ownership

| Layer | Owns |
| --- | --- |
| `manager-app` | `AppError`: code, category, summary, technical details, context, recoverability and operation id. This type is internal and is not exported to TypeScript. |
| `src-tauri` | `IpcResult<T>` and the `IntoIpcResult` conversion. It also mints structured errors for request-boundary facts (malformed identifiers, missing active context, unsupported command argument values, native-dialog channel failures, unusable local paths) with stable codes, but never invents product policy. |
| `shared/api` | `invokeApi`, the single Tauri call site, which normalizes every rejection into `ApiClientError` backed by the generated `ApiErrorDto`. |
| React features | Presentation and recovery UX driven by `summary`, `code`, `recoverability` and `operation_id`. |

`ApiErrorDto` is the only error model in the frontend contract: the internal `AppError` struct is no longer exported as a generated TypeScript type.

## Consequences
### Positive
- Compile-time type safety across the IPC boundary.
- Zero manual TypeScript interface maintenance.
- Protects internal domain models from becoming rigid public webview APIs.
- Structured, user-friendly error handling in the UI.

### Negative
- Requires generating and committing TypeScript artifacts whenever IPC DTOs change.
- Requires `ts-rs` macros and attributes on DTO definitions.
