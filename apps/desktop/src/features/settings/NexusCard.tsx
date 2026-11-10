import React, { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { useNexusStatus } from "@/shared/api/hooks";
import type { NexusStatusDto } from "@/shared/api/generated";
import { Download } from "lucide-react";

const API_KEYS_PAGE = "https://www.nexusmods.com/settings/api-keys";

function accountText(status: NexusStatusDto | undefined): string {
  if (status?.keychain_error) return status.keychain_error;
  if (!status?.key_set) return "No key is saved.";
  if (status.rejected)
    return "A key is saved, but Nexus Mods refused it. Replace it to download from Nexus.";
  const account = status.account;
  if (!account) return "A key is saved in your system keychain.";
  const allowance =
    account.hourly_remaining !== null
      ? ` Nexus allows ${account.hourly_remaining.toLocaleString()} more requests this hour${
          account.daily_remaining !== null
            ? ` and ${account.daily_remaining.toLocaleString()} today`
            : ""
        }.`
      : "";
  return `Connected as ${account.name}${
    account.is_premium
      ? " (Premium: updates can be downloaded directly)."
      : ' (not Premium: use "Mod manager download" on a mod\'s page).'
  }${allowance}`;
}

/**
 * Nexus Mods. Until the manager is registered with Nexus, only a
 * development build can connect, with the user's personal API key, which
 * Nexus allows for testing and personal use. A release build says so and
 * points to downloading by hand.
 */
export const NexusCard: React.FC = () => {
  const client = useQueryClient();
  const { data: status } = useNexusStatus();
  const { data: handler } = useQuery({
    queryKey: ["nxm-handler"],
    queryFn: () => api.getNxmHandlerStatus(),
  });
  const [key, setKey] = useState("");
  const [link, setLink] = useState("");
  const [busy, setBusy] = useState<
    "connect" | "test" | "remove" | "handler" | "link" | null
  >(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const run = async (
    kind: NonNullable<typeof busy>,
    action: () => Promise<void>,
  ) => {
    setBusy(kind);
    setError(null);
    setNotice(null);
    try {
      await action();
    } catch (failure) {
      setError(errorSummary(failure, "Nothing was changed"));
    } finally {
      setBusy(null);
    }
  };

  const setStatus = (next: NexusStatusDto) => {
    client.setQueryData(["nexus-status"], next);
  };

  if (!status?.personal_key_allowed) {
    return (
      <Card className="space-y-3 text-xs">
        <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
          <Download className="w-4 h-4 text-[var(--accent-primary)]" />
          <h3 className="font-bold text-sm">Nexus Mods</h3>
        </div>
        <p>
          This version of the manager does not download from Nexus Mods yet:
          that needs the manager to be registered with Nexus. Updates found on
          Nexus offer "Open mod page"; download the file there and install the
          archive here, and the review says which update it is.
        </p>
      </Card>
    );
  }

  return (
    <Card className="space-y-3 text-xs">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <Download className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">Nexus Mods (development build)</h3>
      </div>
      <p>
        This build can download from Nexus Mods with your personal API key,
        which Nexus allows for testing and personal use. Builds for everyone
        will sign in through Nexus instead, once the manager is registered.{" "}
        <button
          type="button"
          className="underline text-[var(--accent-primary)]"
          onClick={() => void api.openExternalPage(API_KEYS_PAGE)}
        >
          Find your key on Nexus Mods
        </button>
      </p>
      <p role="status">{accountText(status)}</p>
      <form
        className="flex flex-wrap gap-2 items-end"
        onSubmit={(event) => {
          event.preventDefault();
          void run("connect", async () => {
            setStatus(await api.connectNexus(key));
            setKey("");
          });
        }}
      >
        <label className="space-y-1 flex-1 min-w-[16rem]">
          <span className="block">
            {status.key_set ? "Replace the key" : "Paste your personal API key"}
          </span>
          <input
            type="password"
            autoComplete="off"
            spellCheck={false}
            className="w-full px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)] font-mono"
            value={key}
            onChange={(event) => setKey(event.target.value)}
          />
        </label>
        <Button
          size="sm"
          type="submit"
          isLoading={busy === "connect"}
          disabled={!key.trim() || busy !== null}
        >
          Connect
        </Button>
        {status.key_set && (
          <>
            <Button
              size="sm"
              variant="secondary"
              type="button"
              isLoading={busy === "test"}
              disabled={busy !== null}
              onClick={() =>
                void run("test", async () => setStatus(await api.testNexus()))
              }
            >
              Test
            </Button>
            <Button
              size="sm"
              variant="secondary"
              type="button"
              isLoading={busy === "remove"}
              disabled={busy !== null}
              onClick={() =>
                void run("remove", async () =>
                  setStatus(await api.disconnectNexus()),
                )
              }
            >
              Remove
            </Button>
          </>
        )}
      </form>
      <p className="text-[var(--fg-muted)]">
        Connect checks the key with Nexus before saving it in your system
        keychain. It is sent only to api.nexusmods.com, never with file
        downloads, and is not written to the manager's settings or logs.
      </p>

      {handler?.supported && (
        <div className="space-y-1 border-t border-[var(--border)] pt-3">
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={handler.registered}
              disabled={busy !== null}
              onChange={(event) => {
                const enabled = event.target.checked;
                void run("handler", async () => {
                  const next = await api.setNxmHandler(enabled);
                  client.setQueryData(["nxm-handler"], next);
                  if (next.error) setError(next.error);
                });
              }}
            />
            <span>Open "Mod manager download" links from Nexus Mods here</span>
          </label>
          <p className="text-[var(--fg-muted)]">
            Vortex and Mod Organizer 2 use these links too. Turning this off
            gives them back to whichever app had them before.
          </p>
          {handler.error && !error && (
            <p role="alert" className="text-[var(--danger)]">
              {handler.error}
            </p>
          )}
        </div>
      )}

      <form
        className="flex flex-wrap gap-2 items-end border-t border-[var(--border)] pt-3"
        onSubmit={(event) => {
          event.preventDefault();
          void run("link", async () => {
            const received = await api.receiveNxmLink(link);
            setLink("");
            setNotice(
              `Nexus mod ${received.mod_id}, file ${received.file_id} is waiting on the Mods page.`,
            );
            await client.refetchQueries({ queryKey: ["nxm-links"] });
          });
        }}
      >
        <label className="space-y-1 flex-1 min-w-[16rem]">
          <span className="block">Or paste an nxm:// link</span>
          <input
            type="text"
            autoComplete="off"
            spellCheck={false}
            className="w-full px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)] font-mono"
            value={link}
            placeholder="nxm://stardewvalley/mods/…"
            onChange={(event) => setLink(event.target.value)}
          />
        </label>
        <Button
          size="sm"
          variant="secondary"
          type="submit"
          isLoading={busy === "link"}
          disabled={!link.trim() || busy !== null}
        >
          Add
        </Button>
      </form>

      {notice && <p role="status">{notice}</p>}
      {error && (
        <p role="alert" className="text-[var(--danger)]">
          {error}
        </p>
      )}
    </Card>
  );
};
