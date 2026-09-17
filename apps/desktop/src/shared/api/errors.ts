import type { ApiErrorDto, Recoverability } from "./generated";

/**
 * Stable code for rejections that never reached the Rust error boundary, such
 * as framework or runtime failures. It is the only code the frontend invents.
 */
export const IPC_UNEXPECTED_ERROR = "IPC_UNEXPECTED_ERROR";

const UNEXPECTED_SUMMARY = "An unexpected desktop communication error occurred";

/**
 * The single JavaScript error abstraction for a failed IPC call.
 *
 * Every field comes from the Rust-generated ApiErrorDto, so product code can
 * choose a message and a recovery affordance from structured fields instead of
 * parsing backend prose.
 */
export class ApiClientError extends Error {
  readonly dto: ApiErrorDto;

  constructor(dto: ApiErrorDto) {
    super(dto.summary);
    this.name = "ApiClientError";
    this.dto = dto;
  }

  /** Stable machine-readable code owned by the backend. */
  get code(): string {
    return this.dto.code;
  }

  /** How the caller may recover, for example retry_with_fresh_plan. */
  get recoverability(): Recoverability {
    return this.dto.recoverability;
  }

  /** The operation this failure belongs to, when the backend provided one. */
  get operationId(): string | null {
    return this.dto.operation_id;
  }
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

/**
 * Structural guard for a rejection that carries the generated error contract.
 *
 * Category and recoverability are checked as non-empty strings rather than
 * against a frozen list, so a backend that adds a new enum value degrades into
 * a readable ApiClientError instead of an unexpected-communication error.
 */
export function isApiErrorDto(value: unknown): value is ApiErrorDto {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" &&
    candidate.code.length > 0 &&
    typeof candidate.category === "string" &&
    candidate.category.length > 0 &&
    typeof candidate.summary === "string" &&
    candidate.summary.length > 0 &&
    typeof candidate.recoverability === "string" &&
    candidate.recoverability.length > 0 &&
    isNullableString(candidate.technical_details) &&
    isNullableString(candidate.context) &&
    isNullableString(candidate.operation_id)
  );
}

/**
 * A JSON-encoded object is still the structured contract. Tauri normally
 * resolves the error as an object, but an older or alternative transport may
 * deliver the same payload as a JSON string; recovering it here keeps the
 * boundary typed without ever parsing error prose.
 */
function recoverEmbeddedDto(value: string): ApiErrorDto | null {
  const trimmed = value.trim();
  if (!trimmed.startsWith("{")) {
    return null;
  }
  try {
    const parsed: unknown = JSON.parse(trimmed);
    return isApiErrorDto(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

function describeUnexpected(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  if (value instanceof Error) {
    return `${value.name}: ${value.message}`;
  }
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return "Unserializable IPC rejection";
  }
}

/**
 * Normalizes any rejected IPC value into an ApiClientError.
 *
 * The fallback exists for framework and runtime failures only; it never throws,
 * so a malformed rejection cannot break the caller's own error handling.
 */
export function normalizeApiError(value: unknown): ApiClientError {
  if (value instanceof ApiClientError) {
    return value;
  }
  if (typeof value === "string") {
    const embedded = recoverEmbeddedDto(value);
    if (embedded) {
      return new ApiClientError(embedded);
    }
  }
  if (isApiErrorDto(value)) {
    return new ApiClientError(value);
  }
  return new ApiClientError({
    code: IPC_UNEXPECTED_ERROR,
    category: "internal",
    summary: UNEXPECTED_SUMMARY,
    technical_details: describeUnexpected(value),
    context: null,
    recoverability: "terminal",
    operation_id: null,
  });
}

/**
 * The user-facing message for a failure.
 *
 * Structured failures always present the backend summary. Anything else is a
 * frontend-authored error, whose message is safe to show.
 */
export function errorSummary(
  error: unknown,
  fallback = "An unexpected error occurred",
): string {
  if (error instanceof ApiClientError) {
    return error.dto.summary;
  }
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return fallback;
}

/** The ApiClientError behind a value, if the IPC boundary produced one. */
export function apiErrorOf(error: unknown): ApiClientError | null {
  return error instanceof ApiClientError ? error : null;
}

/** The stable backend code for a failure, if there is one. */
export function errorCode(error: unknown): string | null {
  return apiErrorOf(error)?.code ?? null;
}

/** How the caller may recover from a failure, if the backend said. */
export function errorRecoverability(error: unknown): Recoverability | null {
  return apiErrorOf(error)?.recoverability ?? null;
}

/** The operation a failure belongs to, if the backend said. */
export function errorOperationId(error: unknown): string | null {
  return apiErrorOf(error)?.operationId ?? null;
}
