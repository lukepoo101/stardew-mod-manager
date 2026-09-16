import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ModDropZone } from "@/features/mods/ModDropZone";
import { backend } from "@/lib/backend/client";
import { ArchiveInspectionResult } from "@/lib/backend/types";

describe("ModDropZone", () => {
  const mockOnInspectionReady = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  const mockInspection: ArchiveInspectionResult = {
    selection_id: "sel-1",
    package_hash: "hash123",
    original_filename: "TestMod.zip",
    byte_size: 1024,
    plan: {
      plan_id: "plan-1",
      setup_id: "setup-1",
      package_hash: "hash123",
      original_filename: "TestMod.zip",
      mod_folder_name: "TestMod",
      manifest: {
        unique_id: "Test.Mod",
        name: "Test Mod",
        author: "Tester",
        version: "1.0.0",
        dependencies: [],
      },
      raw_manifest: "{}",
      file_inventory: ["manifest.json"],
      dependency_report: {
        is_installable: true,
        smapi_compatible: true,
        duplicate_id: false,
        findings: [],
      },
    },
  };

  it("renders upload card and manual path input", () => {
    render(
      <ModDropZone
        setupId="setup-1"
        onInspectionReady={mockOnInspectionReady}
      />,
    );

    expect(screen.getByText("Add your mod ZIP")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Choose mod ZIP/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByPlaceholderText(/Or paste path or file name/i),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Inspect/i })).toBeDisabled();
  });

  it("picks file using native file picker when Choose mod ZIP clicked", async () => {
    vi.spyOn(backend, "pickModFile").mockResolvedValueOnce(
      "/home/user/Downloads/TestMod.zip",
    );
    vi.spyOn(backend, "inspectMod").mockResolvedValueOnce(mockInspection);

    render(
      <ModDropZone
        setupId="setup-1"
        onInspectionReady={mockOnInspectionReady}
      />,
    );

    const chooseBtn = screen.getByRole("button", { name: /Choose mod ZIP/i });
    fireEvent.click(chooseBtn);

    await waitFor(() => {
      expect(backend.pickModFile).toHaveBeenCalled();
      expect(backend.inspectMod).toHaveBeenCalledWith(
        "/home/user/Downloads/TestMod.zip",
        "setup-1",
      );
      expect(mockOnInspectionReady).toHaveBeenCalledWith(mockInspection);
    });
  });

  it("handles manual path submission with Inspect button", async () => {
    vi.spyOn(backend, "inspectMod").mockResolvedValueOnce(mockInspection);

    render(
      <ModDropZone
        setupId="setup-1"
        onInspectionReady={mockOnInspectionReady}
      />,
    );

    const input = screen.getByPlaceholderText(/Or paste path or file name/i);
    const inspectBtn = screen.getByRole("button", { name: /Inspect/i });

    fireEvent.change(input, { target: { value: "TestMod.zip" } });
    expect(inspectBtn).toBeEnabled();

    fireEvent.click(inspectBtn);

    await waitFor(() => {
      expect(backend.inspectMod).toHaveBeenCalledWith("TestMod.zip", "setup-1");
      expect(mockOnInspectionReady).toHaveBeenCalledWith(mockInspection);
    });
  });

  it("submits manual path on Enter key press", async () => {
    vi.spyOn(backend, "inspectMod").mockResolvedValueOnce(mockInspection);

    render(
      <ModDropZone
        setupId="setup-1"
        onInspectionReady={mockOnInspectionReady}
      />,
    );

    const input = screen.getByPlaceholderText(/Or paste path or file name/i);
    fireEvent.change(input, { target: { value: "TestMod.zip" } });
    fireEvent.keyDown(input, { key: "Enter", code: "Enter" });

    await waitFor(() => {
      expect(backend.inspectMod).toHaveBeenCalledWith("TestMod.zip", "setup-1");
      expect(mockOnInspectionReady).toHaveBeenCalledWith(mockInspection);
    });
  });

  it("handles drag over and drop events", async () => {
    vi.spyOn(backend, "inspectMod").mockResolvedValueOnce(mockInspection);

    render(
      <ModDropZone
        setupId="setup-1"
        onInspectionReady={mockOnInspectionReady}
      />,
    );

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
      expect(backend.inspectMod).toHaveBeenCalledWith(
        "/home/user/Downloads/DroppedMod.zip",
        "setup-1",
      );
      expect(mockOnInspectionReady).toHaveBeenCalledWith(mockInspection);
    });
  });

  it("displays inspection error message when inspection fails", async () => {
    vi.spyOn(backend, "inspectMod").mockRejectedValueOnce(
      new Error("File 'missing.zip' does not exist"),
    );

    render(
      <ModDropZone
        setupId="setup-1"
        onInspectionReady={mockOnInspectionReady}
      />,
    );

    const input = screen.getByPlaceholderText(/Or paste path or file name/i);
    const inspectBtn = screen.getByRole("button", { name: /Inspect/i });

    fireEvent.change(input, { target: { value: "missing.zip" } });
    fireEvent.click(inspectBtn);

    await waitFor(() => {
      expect(
        screen.getByText(/File 'missing.zip' does not exist/i),
      ).toBeInTheDocument();
      expect(mockOnInspectionReady).not.toHaveBeenCalled();
    });
  });
});
