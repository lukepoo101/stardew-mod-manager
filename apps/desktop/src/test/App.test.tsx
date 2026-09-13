import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { App } from "@/app/App";

describe("Stardew Mod Manager UI", () => {
  it("renders the initial discovery screen", async () => {
    render(<App />);

    // Initially loads snapshot from mock backend
    await waitFor(() => {
      expect(screen.getByText(/Set up modding/i)).toBeInTheDocument();
    });

    expect(screen.getByText(/SMAPI is the open-source mod loader/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Install SMAPI/i })).toBeInTheDocument();
  });

  it("advances to dashboard after SMAPI installation", async () => {
    render(<App />);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Install SMAPI/i })).toBeInTheDocument();
    });

    const installBtn = screen.getByRole("button", { name: /Install SMAPI/i });
    fireEvent.click(installBtn);

    // After install, moves to main dashboard
    await waitFor(
      () => {
        expect(screen.getByText(/Ready to Play/i)).toBeInTheDocument();
      },
      { timeout: 3000 }
    );

    expect(screen.getByText(/Add your mod ZIP/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Play/i })).toBeInTheDocument();
  });

  it("toggles light and dark themes", async () => {
    render(<App />);

    await waitFor(() => {
      expect(screen.getByText(/Stardew Mod Manager/i)).toBeInTheDocument();
    });

    const themeToggle = screen.getByLabelText(/Toggle theme/i);
    expect(themeToggle).toBeInTheDocument();

    fireEvent.click(themeToggle);
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    fireEvent.click(themeToggle);
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });
});
