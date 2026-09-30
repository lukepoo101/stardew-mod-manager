import React, { useState } from "react";
import { Card } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { api } from "@/shared/api/client";
import { errorSummary } from "@/shared/api/errors";
import { redactText } from "@/shared/support/redact";
import { copyText } from "@/shared/support/actions";
import { ExternalLink } from "lucide-react";

export const LOG_PARSER_URL = "https://smapi.io/log";

/**
 * Hands the SMAPI log to the community log parser at smapi.io. The manager
 * uploads nothing: the user copies a redacted log and pastes it into the
 * parser page themselves, so they see exactly what leaves the computer.
 */
export const LogParserCard: React.FC<{ rawLog: string }> = ({ rawLog }) => {
  const [status, setStatus] = useState<string | null>(null);
  const redaction = redactText(rawLog);

  if (!rawLog.trim()) return null;

  return (
    <Card className="space-y-3">
      <div className="flex items-center gap-2 border-b border-[var(--border)] pb-3">
        <ExternalLink className="w-4 h-4 text-[var(--accent-primary)]" />
        <h3 className="font-bold text-sm">
          Get help with the SMAPI log parser
        </h3>
      </div>
      <div className="text-xs space-y-2">
        <p>
          The SMAPI log parser at <strong>smapi.io/log</strong> is a website run
          by the SMAPI project. It turns a log into a readable report you can
          share when asking for help.
        </p>
        <p className="text-[var(--fg-muted)]">
          Nothing is sent automatically. Copy the log below (your home folder
          and anything that looks like a key or token are replaced
          {redaction.replacements.secrets > 0
            ? `; ${redaction.replacements.secrets} item(s) were hidden`
            : ""}
          ), open the parser, paste it and choose Upload. Whatever you paste is
          stored on that site.
        </p>
        <ol className="list-decimal pl-4 space-y-1">
          <li>
            <Button
              size="sm"
              variant="secondary"
              onClick={async () =>
                setStatus(
                  (await copyText(redaction.text))
                    ? "Redacted log copied. Paste it on the parser page."
                    : "Could not access the clipboard.",
                )
              }
            >
              Copy redacted log
            </Button>
          </li>
          <li>
            <Button
              size="sm"
              variant="secondary"
              onClick={async () => {
                try {
                  await api.openExternalPage(LOG_PARSER_URL);
                } catch (openError) {
                  setStatus(
                    `${errorSummary(openError, "Could not open the browser")}. Visit ${LOG_PARSER_URL} yourself.`,
                  );
                }
              }}
            >
              Open smapi.io/log
            </Button>
          </li>
        </ol>
        {status && <p role="status">{status}</p>}
      </div>
    </Card>
  );
};
