import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it } from "vitest";
import { Sidebar } from "@/components/layout/Sidebar";

describe("sidebar on narrow windows", () => {
  it("keeps every link named when only icons show", () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter>
          <Sidebar activeProfileName="Main" />
        </MemoryRouter>
      </QueryClientProvider>,
    );
    for (const name of [
      "Overview",
      "Mods",
      "Profiles",
      "Diagnostics",
      "Activity",
      "Settings",
    ]) {
      const link = screen.getByRole("link", { name });
      expect(link).toHaveAttribute("title", name);
      // The visible label is hidden below the lg breakpoint, not removed.
      expect(link.querySelector("span")?.className).toMatch(/hidden lg:inline/);
    }
  });
});
