import { useEffect, useId, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { WsAudience, WsExportFormat, WsLevel } from "../../api/types";
import { saveBlob } from "../../lib/download";

/** The four exports, in the order they are listed: students' copy first. */
const ITEMS: Array<{ audience: WsAudience; format: WsExportFormat; label: string }> = [
  { audience: "student", format: "docx", label: "學生版 Word" },
  { audience: "student", format: "pdf", label: "學生版 PDF" },
  { audience: "teacher", format: "docx", label: "教師版 Word" },
  { audience: "teacher", format: "pdf", label: "教師版 PDF" },
];

/**
 * 匯出: one button that opens a menu of the four files (學生版／教師版 × Word／PDF), so the teacher
 * never has to wonder which version she gets. Used by the prep session's worksheet card and by the
 * editor page. The file is for the level tab being viewed; with several levels the menu says which.
 */
export function ExportMenu({
  worksheetId,
  level,
  levelLabel = null,
  disabled = false,
  beforeExport,
  onError,
  footnote,
}: {
  worksheetId: number;
  level: WsLevel;
  /** Set when the worksheet has several levels: the menu's first line says 「匯出：基礎版」. */
  levelLabel?: string | null;
  disabled?: boolean;
  /** Runs before each export; returns a message when the export must not go on (the editor saves first). */
  beforeExport?: () => Promise<string | null | undefined | void>;
  /** The failure to show (null clears it); the page puts it where it fits. */
  onError: (message: string | null) => void;
  /** A short note under the items. */
  footnote?: string;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const root = useRef<HTMLSpanElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const items = useRef<Array<HTMLButtonElement | null>>([]);

  // Click outside closes (focus stays where the click put it).
  useEffect(() => {
    if (!open) return undefined;
    const away = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", away);
    return () => document.removeEventListener("mousedown", away);
  }, [open]);

  function openAt(index: number) {
    setOpen(true);
    // The menu is rendered by the time the frame runs.
    requestAnimationFrame(() => items.current[index]?.focus());
  }

  function close(refocus: boolean) {
    setOpen(false);
    if (refocus) button.current?.focus();
  }

  function moveFocus(to: number) {
    const n = ITEMS.length;
    items.current[((to % n) + n) % n]?.focus();
  }

  async function run(format: WsExportFormat, audience: WsAudience) {
    close(true);
    setBusy(true);
    onError(null);
    try {
      const failed = await beforeExport?.();
      if (failed) throw new Error(failed);
      const { blob, filename } = await api.exportWorksheet(worksheetId, { format, level, audience });
      saveBlob(blob, filename);
    } catch (err) {
      onError(err instanceof Error && !("status" in err) ? `沒有匯出：${err.message}` : errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  const inactive = disabled || busy;
  return (
    <span className="export-menu" ref={root}>
      <button
        ref={button}
        type="button"
        className="secondary-button small-button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? `${id}-menu` : undefined}
        disabled={inactive}
        onClick={() => (open ? close(false) : setOpen(true))}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            openAt(0);
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            openAt(ITEMS.length - 1);
          }
        }}
      >
        {busy ? "匯出中…" : "匯出"}
        <span className="export-caret" aria-hidden="true">
          ▾
        </span>
      </button>
      {open && !inactive && (
        <div
          id={`${id}-menu`}
          className="export-popup"
          role="menu"
          tabIndex={-1}
          aria-label="匯出"
          onKeyDown={(e) => {
            const at = items.current.findIndex((el) => el === document.activeElement);
            if (e.key === "ArrowDown") {
              e.preventDefault();
              moveFocus(at + 1);
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              moveFocus(at - 1);
            } else if (e.key === "Home") {
              e.preventDefault();
              moveFocus(0);
            } else if (e.key === "End") {
              e.preventDefault();
              moveFocus(ITEMS.length - 1);
            } else if (e.key === "Escape") {
              e.preventDefault();
              close(true);
            } else if (e.key === "Tab") {
              setOpen(false);
            }
          }}
        >
          {levelLabel && (
            <div className="export-head" role="presentation">
              匯出：{levelLabel}
            </div>
          )}
          {ITEMS.map((item, i) => (
            <button
              key={item.label}
              ref={(el) => {
                items.current[i] = el;
              }}
              type="button"
              role="menuitem"
              tabIndex={-1}
              className="export-item"
              onClick={() => void run(item.format, item.audience)}
            >
              {item.label}
            </button>
          ))}
          {footnote && (
            <div className="export-foot" role="presentation">
              {footnote}
            </div>
          )}
        </div>
      )}
    </span>
  );
}
