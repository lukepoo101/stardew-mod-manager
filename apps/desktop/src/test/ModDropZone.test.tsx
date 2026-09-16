import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { ModDropZone } from "@/features/mods/ModDropZone";
import { api } from "@/shared/api/client";

describe("ModDropZone", () => {
  const onArchiveSelected = vi.fn(async () => {});

  beforeEach(() => {
    vi.clearAllMocks();
  });
  afterEach(() => vi.restoreAllMocks());

  it("renders upload card and manual path input", () => {
    render(<ModDropZone onArchiveSelected={onArchiveSelected} />);

    expect(screen.getByText("Add your mod ZIP")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Choose mod ZIP/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByPlaceholderText(/Or paste path or file name/i),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Inspect/i })).toBeDisabled();
  });

  it("hands the natively picked archive to the owning feature", async () => {
    vi.spyOn(api, "pickArchiveDialog").mockResolvedValueOnce(
      "/home/user/Downloads/TestMod.zip",
    );

    render(<ModDropZone onArchiveSelected={onArchiveSelected} />);

    fireEvent.click(screen.getByRole("button", { name: /Choose mod ZIP/i }));

    await waitFor(() => {
      expect(api.pickArchiveDialog).toHaveBeenCalled();
      expect(onArchiveSelected).toHaveBeenCalledWith(
        "/home/user/Downloads/TestMod.zip",
      );
    });
  });

  it("submits the manual path with the Inspect button", async () => {
    render(<ModDropZone onArchiveSelected={onArchiveSelected} />);

    const input = screen.getByPlaceholderText(/Or paste path or file name/i);
    const inspectBtn = screen.getByRole("button", { name: /Inspect/i });

    fireEvent.change(input, { target: { value: "TestMod.zip" } });
    expect(inspectBtn).toBeEnabled();

    fireEvent.click(inspectBtn);

    await waitFor(() => {
      expect(onArchiveSelected).toHaveBeenCalledWith("TestMod.zip");
    });
  });

  it("submits the manual path on Enter key press", async () => {
    render(<ModDropZone onArchiveSelected={onArchiveSelected} />);

    const input = screen.getByPlaceholderText(/Or paste path or file name/i);
    fireEvent.change(input, { target: { value: "TestMod.zip" } });
    fireEvent.keyDown(input, { key: "Enter", code: "Enter" });

    await waitFor(() => {
      expect(onArchiveSelected).toHaveBeenCalledWith("TestMod.zip");
    });
  });

  it("handles drag over and drop events", async () => {
    render(<ModDropZone onArchiveSelected={onArchiveSelected} />);

    const dropArea = screen
      .getByText("Add your mod ZIP")
      .closest("div[class*='border-dashed']");
    if (!dropArea) {
      throw new Error("Mod drop area was not rendered");
    }

    fireEvent.dragOver(dropArea, { dataTransfer: {} });
    expect(dropArea.className).toContain("border-[var(--accent-primary)]");

    fireEvent.dragLeave(dropArea);
    expect(dropArea.className).not.toContain("border-[var(--accent-primary)]");

    const fakeFile = new File(["dummy zip"], "DroppedMod.zip", {
      type: "application/zip",
    });
    Object.defineProperty(fakeFile, "path", {
      value: "/home/user/Downloads/DroppedMod.zip",
    });

    fireEvent.drop(dropArea, {
      dataTransfer: {
        files: [fakeFile],
      },
    });

    await waitFor(() => {
      expect(onArchiveSelected).toHaveBeenCalledWith(
        "/home/user/Downloads/DroppedMod.zip",
      );
    });
  });

  it("displays inspection error message when inspection fails", async () => {
    const failing = vi.fn(async () => {
      throw new Error("File 'missing.zip' does not exist");
    });

    render(<ModDropZone onArchiveSelected={failing} />);

    const input = screen.getByPlaceholderText(/Or paste path or file name/i);
    fireEvent.change(input, { target: { value: "missing.zip" } });
    fireEvent.click(screen.getByRole("button", { name: /Inspect/i }));

    await waitFor(() => {
      expect(
        screen.getByText(/File 'missing.zip' does not exist/i),
      ).toBeInTheDocument();
    });
  });
});
