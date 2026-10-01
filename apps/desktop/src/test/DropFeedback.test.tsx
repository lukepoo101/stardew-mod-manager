import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ModDropZone, chooseDropped } from "@/features/mods/ModDropZone";

describe("dropped files", () => {
  it("opens exactly one .zip", () => {
    expect(chooseDropped(["/dl/Mod.zip"])).toEqual({ path: "/dl/Mod.zip" });
    expect(chooseDropped(["C:\\\\dl\\\\Mod.ZIP"])).toEqual({
      path: "C:\\\\dl\\\\Mod.ZIP",
    });
  });

  it("explains a non-archive instead of ignoring it", () => {
    const choice = chooseDropped(["/dl/readme.txt"]);
    expect("error" in choice && choice.error).toMatch(
      /readme\.txt is not a \.zip mod archive/,
    );
  });

  it("refuses several files at once, naming them", () => {
    const choice = chooseDropped(["/dl/A.zip", "/dl/B.zip"]);
    expect("error" in choice && choice.error).toMatch(
      /Drop one mod archive at a time\. 2 files were dropped \(A\.zip, B\.zip\); none were opened\./,
    );
  });

  it("shows the reason in the drop zone and opens nothing", () => {
    const onArchiveSelected = vi.fn();
    render(<ModDropZone onArchiveSelected={onArchiveSelected} />);
    const zone = screen.getByText(/drop/i).closest("div") as HTMLElement;
    fireEvent.drop(zone, {
      dataTransfer: {
        files: [new File(["a"], "A.zip"), new File(["b"], "B.zip")],
      },
    });
    expect(
      screen.getByText(/Drop one mod archive at a time/),
    ).toBeInTheDocument();
    expect(onArchiveSelected).not.toHaveBeenCalled();
  });
});

describe("dropping several archives where batches are offered", () => {
  it("starts a batch with every .zip", () => {
    const onArchivesSelected = vi.fn();
    render(
      <ModDropZone
        onArchiveSelected={vi.fn()}
        onArchivesSelected={onArchivesSelected}
      />,
    );
    const zone = screen.getByText(/drop/i).closest("div") as HTMLElement;
    fireEvent.drop(zone, {
      dataTransfer: {
        files: [new File(["a"], "A.zip"), new File(["b"], "B.zip")],
      },
    });
    expect(onArchivesSelected).toHaveBeenCalledWith(["A.zip", "B.zip"]);
  });

  it("refuses a mix that includes a non-archive", () => {
    const onArchivesSelected = vi.fn();
    render(
      <ModDropZone
        onArchiveSelected={vi.fn()}
        onArchivesSelected={onArchivesSelected}
      />,
    );
    const zone = screen.getByText(/drop/i).closest("div") as HTMLElement;
    fireEvent.drop(zone, {
      dataTransfer: {
        files: [new File(["a"], "A.zip"), new File(["b"], "notes.txt")],
      },
    });
    expect(screen.getByText(/notes\.txt is not/)).toBeInTheDocument();
    expect(onArchivesSelected).not.toHaveBeenCalled();
  });
});
