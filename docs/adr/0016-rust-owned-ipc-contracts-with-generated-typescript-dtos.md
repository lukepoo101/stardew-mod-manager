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
  "code": "OPERATION_CONFLICT",
  "category": "operation_conflict",
  "summary": "Profile was modified since the preview was generated",
  "technical_details": "Expected revision 17, but found 18",
  "context": null,
  "recoverability": "retry_with_fresh_plan",
  "operation_id": "018f3a..."
}
```

Fields and enum values use the workspace-wide snake_case IPC convention; the example above is the serialization that ships. The frontend formats user-facing messages from `summary` and determines recovery options from `code`, `recoverability` and `operation_id` without parsing arbitrary error strings.

`technical_details` carries diagnostics (filesystem paths, parser output, database detail) and is never the primary user-facing message.

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
