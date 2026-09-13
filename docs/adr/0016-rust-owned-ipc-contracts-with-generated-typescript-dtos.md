# ADR-0016: Rust-Owned IPC Contracts with Generated TypeScript DTOs

## Status
Accepted

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
Replace generic string errors across IPC. When an error occurs, commands return a structured `ApiErrorDto`:
```json
{
  "code": "OPERATION_CONFLICT",
  "category": "OperationConflict",
  "summary": "Profile was modified since preview was generated",
  "technicalDetails": "Expected revision 17, but found 18",
  "recoverability": "RetryWithFreshPlan",
  "operationId": "018f3a..."
}
```
The frontend formats user-friendly messages and determines recovery options without parsing arbitrary error strings.

## Consequences
### Positive
- Compile-time type safety across the IPC boundary.
- Zero manual TypeScript interface maintenance.
- Protects internal domain models from becoming rigid public webview APIs.
- Structured, user-friendly error handling in the UI.

### Negative
- Requires generating and committing TypeScript artifacts whenever IPC DTOs change.
- Requires `ts-rs` macros and attributes on DTO definitions.
