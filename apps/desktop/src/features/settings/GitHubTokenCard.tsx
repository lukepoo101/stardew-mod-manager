import React, { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import type { GitHubRateLimitDto } from "@/shared/api/generated";
import { KeyRound } from "lucide-react";

const NEW_TOKEN_PAGE = "https://github.com/settings/personal-access-tokens/new";

const limitText = (limit: GitHubRateLimitDto) =>
  `${limit.with_token ? "With this token" : "Without a token"} GitHub allows ${limit.limit.toLocaleString()} requests an hour; ${limit.remaining.toLocaleString()} are left${
    limit.resets_at
      ? `, refilled at ${new Date(limit.resets_at).toLocaleTimeString()}`
      : ""
  }.`;

/**
 * An optional GitHub token, which raises GitHub's limit for reading release
 * lists from 60 requests an hour to 5000. It is kept in the system keychain
 * and never comes back to the window: the card only learns whether one is
 * saved.
 */
export const GitHubTokenCard: React.FC = () => {
  const client = useQueryClient();
  const queryKey = ["github-token-status"];
  const { data: status } = useQuery({
    queryKey,
    queryFn: () => api.getGitHubTokenStatus(),
  });
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState<"save" | "clear" | "test" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [limit, setLimit] = useState<GitHubRateLimitDto | null>(null);

  const run = async (kind: "save" | "clear" | "test") => {
    setBusy(kind);
    setError(null);
    setLimit(null);
    try {
      if (kind === "test") {
        setLimit(await api.testGitHubToken(token.trim() || null));
      } else {
        const next =
          kind === "save"
            ? await api.setGitHubToken(token)
            : await api.clearGitHubToken();
        setToken("");
        client.setQueryData(queryKey, next);
      }
    } catch (failure) {
      setError(
        errorSummary(
          failure,
          kind === "test" ? "GitHub could not be asked" : "Nothing was changed",
        ),
      );
    } finally {
      setBusy(null);
      if (kind === "test") await client.refetchQueries({ queryKey });
    }
  };

  return (
    <Card className="space-y-3 text-xs">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <KeyRound className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">GitHub token (optional)</h3>
      </div>
      <p>
        Update checks read public GitHub release lists without an account, which
        GitHub limits to 60 requests an hour, so a check reads at most 20
        repositories. A token raises the limit to 5000. Create a fine-grained
        token with{" "}
        <span className="font-semibold">Public repositories (read-only)</span>{" "}
        access and no permissions; nothing more is needed.{" "}
        <button
          type="button"
          className="underline text-[var(--accent-primary)]"
          onClick={() => void api.openExternalPage(NEW_TOKEN_PAGE)}
        >
          Create one on GitHub
        </button>
      </p>
      <p role="status">
        {status?.keychain_error
          ? status.keychain_error
          : status?.set
            ? status.rejected
              ? "A token is saved, but GitHub rejected it. Releases are read without it until you replace it."
              : "A token is saved in your system keychain."
            : "No token is saved."}
      </p>
      <form
        className="flex flex-wrap gap-2 items-end"
        onSubmit={(event) => {
          event.preventDefault();
          void run("save");
        }}
      >
        <label className="space-y-1 flex-1 min-w-[16rem]">
          <span className="block">
            {status?.set ? "Replace the token" : "Paste a token"}
          </span>
          <input
            type="password"
            autoComplete="off"
            spellCheck={false}
            className="w-full px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--bg-surface)] font-mono"
            value={token}
            placeholder="github_pat_…"
            onChange={(event) => setToken(event.target.value)}
          />
        </label>
        <Button
          size="sm"
          type="submit"
          isLoading={busy === "save"}
          disabled={!token.trim() || busy !== null}
        >
          Save
        </Button>
        <Button
          size="sm"
          variant="secondary"
          type="button"
          isLoading={busy === "test"}
          disabled={busy !== null}
          onClick={() => void run("test")}
        >
          Test
        </Button>
        {status?.set && (
          <Button
            size="sm"
            variant="secondary"
            type="button"
            isLoading={busy === "clear"}
            disabled={busy !== null}
            onClick={() => void run("clear")}
          >
            Remove
          </Button>
        )}
      </form>
      <p className="text-[var(--fg-muted)]">
        Test checks the pasted token, or the saved one when nothing is pasted.
        The token is sent only to api.github.com, never with file downloads, and
        is not written to the manager's settings or logs.
      </p>
      {limit && <p role="status">{limitText(limit)}</p>}
      {error && (
        <p role="alert" className="text-[var(--danger)]">
          {error}
        </p>
      )}
    </Card>
  );
};
