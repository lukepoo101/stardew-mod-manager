import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ModReviewDialog } from "@/features/mods/ModReviewDialog";
import { backend } from "@/lib/backend/client";
import { ArchiveInspectionResult, InstalledMod } from "@/lib/backend/types";

describe("ModReviewDialog", () => {
  const mockOnClose = vi.fn();
  const mockOnModInstalled = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  const createInspectionResult = (overrides?: Partial<ArchiveInspectionResult["plan"]>): ArchiveInspectionResult => ({
    selection_id: "sel-1",
    package_hash: "hash123",
    original_filename: "AwesomeMod.zip",
    byte_size: 1024 * 100,
    plan: {
      plan_id: "plan-1",
      setup_id: "setup-1",
      package_hash: "hash123",
      original_filename: "AwesomeMod.zip",
      mod_folder_name: "AwesomeMod",
      manifest: {
        unique_id: "Author.AwesomeMod",
        name: "Awesome Mod",
        author: "AwesomeAuthor",
        version: "2.0.0",
        description: "An awesome mod adding cool features.",
        entry_dll: "AwesomeMod.dll",
        dependencies: [],
      },
      raw_manifest: "{}",
      file_inventory: ["manifest.json", "AwesomeMod.dll", "assets/icon.png"],
      dependency_report: {
        is_installable: true,
        smapi_compatible: true,
        duplicate_id: false,
        findings: [],
      },
      ...overrides,
    },
  });

  it("renders null when inspection is null", () => {
    const { container } = render(
      <ModReviewDialog
        inspection={null}
        onClose={mockOnClose}
        onModInstalled={mockOnModInstalled}
      />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders manifest details correctly for a valid mod", () => {
    const inspection = createInspectionResult();

    render(
      <ModReviewDialog
        inspection={inspection}
        onClose={mockOnClose}
        onModInstalled={mockOnModInstalled}
      />
    );

    expect(screen.getByText("Awesome Mod")).toBeInTheDocument();
    expect(screen.getByText(/by AwesomeAuthor • Version 2.0.0/i)).toBeInTheDocument();
    expect(screen.getByText("An awesome mod adding cool features.")).toBeInTheDocument();
    expect(screen.getByText("Author.AwesomeMod")).toBeInTheDocument();
    expect(screen.getByText("AwesomeMod.zip")).toBeInTheDocument();
    expect(screen.getByText("3 files")).toBeInTheDocument();
    expect(screen.getByText(/No external mod dependencies required/i)).toBeInTheDocument();

    const installBtn = screen.getByRole("button", { name: /Install Mod/i });
    expect(installBtn).toBeEnabled();
  });

  it("displays component manifests when archive contains multiple sub-mods", () => {
    const inspection = createInspectionResult({
      component_manifests: [
        {
          manifest: {
            unique_id: "Author.AwesomeMod.Core",
            name: "Awesome Mod Core",
            author: "AwesomeAuthor",
            version: "2.0.0",
            dependencies: [],
          },
          raw_manifest: "{}",
          relative_subfolder: "Core",
        },
        {
          manifest: {
            unique_id: "Author.AwesomeMod.Content",
            name: "Awesome Mod Content",
            author: "AwesomeAuthor",
            version: "2.0.0",
            dependencies: [],
          },
          raw_manifest: "{}",
          relative_subfolder: "Content",
        },
      ],
    });

    render(
      <ModReviewDialog
        inspection={inspection}
        onClose={mockOnClose}
        onModInstalled={mockOnModInstalled}
      />
    );

    expect(screen.getByText(/Included Mod Components \(2\)/i)).toBeInTheDocument();
    expect(screen.getByText("Awesome Mod Core")).toBeInTheDocument();
    expect(screen.getByText("Awesome Mod Content")).toBeInTheDocument();
    expect(screen.getByText("Author.AwesomeMod.Core")).toBeInTheDocument();
    expect(screen.getByText("Author.AwesomeMod.Content")).toBeInTheDocument();
  });

  it("disables installation when dependencies are missing", () => {
    const inspection = createInspectionResult({
      dependency_report: {
        is_installable: false,
        smapi_compatible: true,
        duplicate_id: false,
        findings: [
          {
            unique_id: "Pathoschild.ContentPatcher",
            is_required: true,
            required_version: "2.0.0",
            satisfied: false,
            is_content_pack_framework: false,
            reason: "Required dependency 'Pathoschild.ContentPatcher' is not installed",
          },
        ],
      },
    });

    render(
      <ModReviewDialog
        inspection={inspection}
        onClose={mockOnClose}
        onModInstalled={mockOnModInstalled}
      />
    );

    expect(screen.getByText("Pathoschild.ContentPatcher")).toBeInTheDocument();
    expect(screen.getByText(/Missing/i)).toBeInTheDocument();

    const installBtn = screen.getByRole("button", { name: /Install Mod/i });
    expect(installBtn).toBeDisabled();
  });

  it("warns about duplicate mod ID", () => {
    const inspection = createInspectionResult({
      dependency_report: {
        is_installable: false,
        smapi_compatible: true,
        duplicate_id: true,
        findings: [],
      },
    });

    render(
      <ModReviewDialog
        inspection={inspection}
        onClose={mockOnClose}
        onModInstalled={mockOnModInstalled}
      />
    );

    expect(screen.getByText(/is already installed/i)).toBeInTheDocument();
    const installBtn = screen.getByRole("button", { name: /Install Mod/i });
    expect(installBtn).toBeDisabled();
  });

  it("installs mod successfully when user clicks Install Mod", async () => {
    const inspection = createInspectionResult();
    const installedMod: InstalledMod = {
      id: "mod-99",
      setup_id: "setup-1",
      package_id: "hash123",
      unique_id: "Author.AwesomeMod",
      name: "Awesome Mod",
      author: "AwesomeAuthor",
      version: "2.0.0",
      raw_manifest: "{}",
      relative_target_path: "AwesomeMod",
      file_inventory: ["manifest.json", "AwesomeMod.dll"],
      installed_at: new Date().toISOString(),
    };

    vi.spyOn(backend, "installMod").mockResolvedValueOnce(installedMod);

    render(
      <ModReviewDialog
        inspection={inspection}
        onClose={mockOnClose}
        onModInstalled={mockOnModInstalled}
      />
    );

    const installBtn = screen.getByRole("button", { name: /Install Mod/i });
    fireEvent.click(installBtn);

    await waitFor(() => {
      expect(backend.installMod).toHaveBeenCalledWith(inspection.plan);
      expect(mockOnModInstalled).toHaveBeenCalledWith(installedMod);
      expect(mockOnClose).toHaveBeenCalled();
    });
  });

  it("displays error banner when installMod fails", async () => {
    const inspection = createInspectionResult();
    vi.spyOn(backend, "installMod").mockRejectedValueOnce(new Error("Disk full: cannot commit mod"));

    render(
      <ModReviewDialog
        inspection={inspection}
        onClose={mockOnClose}
        onModInstalled={mockOnModInstalled}
      />
    );

    const installBtn = screen.getByRole("button", { name: /Install Mod/i });
    fireEvent.click(installBtn);

    await waitFor(() => {
      expect(screen.getByText(/Disk full: cannot commit mod/i)).toBeInTheDocument();
      expect(mockOnClose).not.toHaveBeenCalled();
    });
  });

  it("calls onClose when Cancel button is clicked", async () => {
    const inspection = createInspectionResult();

    render(
      <ModReviewDialog
        inspection={inspection}
        onClose={mockOnClose}
        onModInstalled={mockOnModInstalled}
      />
    );

    const cancelBtn = screen.getByRole("button", { name: /Cancel/i });
    fireEvent.click(cancelBtn);

    await waitFor(() => {
      expect(mockOnClose).toHaveBeenCalled();
    });
  });
});

