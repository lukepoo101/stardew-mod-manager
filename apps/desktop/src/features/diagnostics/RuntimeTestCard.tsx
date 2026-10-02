import React from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { useGuardedLaunch } from "@/shared/launch/useGuardedLaunch";

/**
 * Starts SMAPI with an empty Mods folder to see whether SMAPI itself works,
 * apart from any mod. The profile's own Mods folder is not used or changed;
 * the result appears with the other sessions.
 */
export const RuntimeTestCard: React.FC = () => {
  const launch = useGuardedLaunch();
  return (
    <Card className="space-y-2 text-sm">
      <h3 className="font-bold">Test SMAPI on its own</h3>
      <p className="text-xs text-[var(--fg-muted)]">
        Starts the game through SMAPI with an empty Mods folder the manager
        keeps for this. Your profile's mods are not loaded, moved or changed. If
        SMAPI starts here but not with your mods, a mod is the likely cause.
        Close the game when it reaches the title screen; the result is shown
        with your sessions.
      </p>
      {launch.error && (
        <p role="alert" className="text-xs text-[var(--danger)]">
          {launch.error}
        </p>
      )}
      <Button
        variant="secondary"
        size="sm"
        isLoading={launch.isPending}
        onClick={() => void launch.request("RuntimeTest")}
      >
        Start SMAPI without mods
      </Button>
      {launch.dialog}
    </Card>
  );
};
