import { describe, expect, it } from "vitest";
import {
  ApiClientError,
  apiErrorOf,
  errorCode,
  errorOperationId,
  errorRecoverability,
  errorSummary,
  IPC_UNEXPECTED_ERROR,
  isApiErrorDto,
  normalizeApiError,
} from "@/shared/api/errors";
import type { ApiErrorDto } from "@/shared/api/generated";

const conflictDto: ApiErrorDto = {
  code: "PREVIEW_STALE",
  category: "operation_conflict",
  summary: "Profile was modified since the preview was generated",
  technical_details: "Expected profile revision 17, but current revision is 18",
  context: "profile 018f3b",
  recoverability: "retry_with_fresh_plan",
  operation_id: "018f3a-0000-0000-0000-000000000000",
};

describe("structured IPC errors", () => {
  it("accepts the structured rejection the Rust boundary produces", () => {
    expect(isApiErrorDto(conflictDto)).toBe(true);

    const error = normalizeApiError(conflictDto);

    expect(error).toBeInstanceOf(ApiClientError);
    expect(error.code).toBe("PREVIEW_STALE");
    expect(error.recoverability).toBe("retry_with_fresh_plan");
    expect(error.operationId).toBe("018f3a-0000-0000-0000-000000000000");
    expect(error.message).toBe(
      "Profile was modified since the preview was generated",
    );
    // The summary keeps the backend's words and adds the operation and the
    // next step for its class.
    expect(errorSummary(error)).toMatch(
      /^Profile was modified since the preview was generated \(operation 018f3a-0\)\. Nothing was changed/,
    );
  });

  it("accepts a dto whose optional fields are null", () => {
    const dto: ApiErrorDto = {
      ...conflictDto,
      technical_details: null,
      context: null,
      operation_id: null,
    };

    expect(isApiErrorDto(dto)).toBe(true);
    const error = normalizeApiError(dto);
    expect(error.code).toBe("PREVIEW_STALE");
    expect(error.operationId).toBeNull();
    expect(error.dto.technical_details).toBeNull();
  });

  it("keeps structured accessors available for recovery decisions", () => {
    const error = normalizeApiError({
      ...conflictDto,
      code: "LEGACY_INSTALL_RECONCILIATION_FAILED",
      category: "recovery",
      recoverability: "requires_manual_intervention",
      operation_id: "operation-1",
      summary: "Legacy recovery reconciliation failed",
    });

    expect(errorCode(error)).toBe("LEGACY_INSTALL_RECONCILIATION_FAILED");
    expect(errorRecoverability(error)).toBe("requires_manual_intervention");
    expect(errorOperationId(error)).toBe("operation-1");
    expect(apiErrorOf(error)).toBe(error);
    expect(errorCode(new Error("plain"))).toBeNull();
    expect(errorRecoverability(undefined)).toBeNull();
    expect(errorOperationId("nope")).toBeNull();
  });

  it("rejects a malformed object without crashing", () => {
    const error = normalizeApiError({ code: 42, summary: null, category: {} });

    expect(error.code).toBe(IPC_UNEXPECTED_ERROR);
    expect(error.dto.category).toBe("internal");
    expect(error.dto.recoverability).toBe("terminal");
    expect(error.dto.summary).toBe(
      "An unexpected desktop communication error occurred",
    );
    expect(error.dto.operation_id).toBeNull();
    expect(error.dto.technical_details).toContain("42");
    expect(errorSummary(error)).toMatch(
      /^An unexpected desktop communication error occurred\. If this happens again/,
    );
  });

  it("normalizes a legacy plain-text rejection without showing it to users", () => {
    const error = normalizeApiError("Profile not found");

    expect(error).toBeInstanceOf(ApiClientError);
    expect(error.code).toBe(IPC_UNEXPECTED_ERROR);
    expect(error.dto.technical_details).toBe("Profile not found");
    expect(errorSummary(error)).not.toContain("Profile not found");
  });

  it("recovers a structured dto that was transported as a JSON string", () => {
    const error = normalizeApiError(JSON.stringify(conflictDto));

    expect(error.code).toBe("PREVIEW_STALE");
    expect(error.recoverability).toBe("retry_with_fresh_plan");
  });

  it("normalizes ordinary JavaScript errors and unexpected values", () => {
    const values: unknown[] = [
      new Error("boom"),
      undefined,
      null,
      42,
      [],
      { nested: true },
    ];

    for (const value of values) {
      const error = normalizeApiError(value);
      expect(error).toBeInstanceOf(ApiClientError);
      expect(error.code).toBe(IPC_UNEXPECTED_ERROR);
      expect(errorSummary(error)).toMatch(
        /^An unexpected desktop communication error occurred\./,
      );
      expect(typeof error.dto.technical_details).toBe("string");
    }
  });

  it("is idempotent for an already normalized error", () => {
    const normalized = normalizeApiError(conflictDto);

    expect(normalizeApiError(normalized)).toBe(normalized);
  });

  it("falls back to a caller-provided message for non-structural failures", () => {
    expect(errorSummary(new Error("plain failure"), "fallback")).toBe(
      "plain failure",
    );
    expect(errorSummary({ odd: true }, "fallback")).toBe("fallback");
  });
});

import { nextStep } from "@/shared/api/errors";

describe("next steps for failures", () => {
  const dto = (category: string, recoverability = "terminal") =>
    ({
      code: "X",
      category,
      summary: "s",
      recoverability,
      technical_details: null,
      context: null,
      operation_id: null,
    }) as never;

  it("names a next step by class, and adds nothing to validation messages", () => {
    expect(nextStep(dto("permission"))).toMatch(/read and write that folder/);
    expect(nextStep(dto("storage"))).toMatch(/free space/);
    expect(nextStep(dto("validation"))).toBeNull();
    expect(nextStep(dto("filesystem", "requires_manual_intervention"))).toMatch(
      /needs recovery/,
    );
  });
});
