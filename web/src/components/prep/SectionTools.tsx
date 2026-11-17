import { useEffect, useState, type ReactNode } from "react";
import { api, errorMessage } from "../../api/client";
import type { Thumbs } from "../../api/types";
import { copyText } from "../../lib/download";
import { ErrorText } from "../common";

/**
 * The tools under one section of a result: 複製, and 有幫助／不合用 (recorded on the AI call that
 * produced it, as a skip decision with a rating). Without an AI call there is nothing to rate.
 */
export function SectionTools({
  name,
  text,
  interactionId,
  children,
}: {
  /** What the section is called, for the buttons' accessible names. */
  name: string;
  text: string;
  interactionId: number | null;
  /** More tools of the section (a worksheet block: 改這一區、加註記…). */
  children?: ReactNode;
}) {
  const [copied, setCopied] = useState<boolean | null>(null);
  const [thumbs, setThumbs] = useState<Thumbs | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (copied === null) return undefined;
    const timer = setTimeout(() => setCopied(null), 2000);
    return () => clearTimeout(timer);
  }, [copied]);

  async function rate(next: Thumbs) {
    if (interactionId === null) return;
    const before = thumbs;
    setThumbs(next);
    setError(null);
    try {
      await api.sendDecision(interactionId, { decision: "skip", thumbs: next });
    } catch (err) {
      setThumbs(before);
      setError(errorMessage(err));
    }
  }

  return (
    <div className="prep-tools">
      <button
        type="button"
        className="decision-button"
        aria-label={`複製${name}`}
        onClick={() => void copyText(text).then(setCopied)}
      >
        {copied === true ? "已複製" : copied === false ? "沒有複製成功" : "複製"}
      </button>
      {interactionId !== null && (
        <>
          <button
            type="button"
            className="decision-button"
            aria-label={`${name}有幫助`}
            aria-pressed={thumbs === "up"}
            onClick={() => void rate("up")}
          >
            有幫助
          </button>
          <button
            type="button"
            className="decision-button"
            aria-label={`${name}不合用`}
            aria-pressed={thumbs === "down"}
            onClick={() => void rate("down")}
          >
            不合用
          </button>
        </>
      )}
      {children}
      <ErrorText message={error} />
    </div>
  );
}
