import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  LOG_PARSER_URL,
  LogParserCard,
} from "@/features/diagnostics/LogParserCard";
import { api } from "@/shared/api/client";

afterEach(() => vi.restoreAllMocks());

describe("SMAPI log parser handoff", () => {
  it("copies only the redacted log and opens the parser on request", async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: write },
      configurable: true,
    });
    const open = vi.spyOn(api, "openExternalPage").mockResolvedValue();
    render(
      <LogParserCard
        rawLog={"[SMAPI] Loaded from /home/alice/.local/share/Steam"}
      />,
    );
    expect(open).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Copy redacted log" }));
    await waitFor(() => expect(write).toHaveBeenCalled());
    expect(write.mock.calls[0][0]).not.toContain("alice");
    fireEvent.click(screen.getByRole("button", { name: "Open smapi.io/log" }));
    await waitFor(() => expect(open).toHaveBeenCalledWith(LOG_PARSER_URL));
  });

  it("shows nothing without a log", () => {
    const { container } = render(<LogParserCard rawLog="  " />);
    expect(container.textContent).toBe("");
  });
});
