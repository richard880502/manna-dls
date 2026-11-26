import { useCallback, useEffect, useId, useState } from "react";
import { api, errorMessage } from "../../../api/client";
import type { WsAudience, WsLevel, WsSheet } from "../../../api/types";
import { LEVEL_LABEL } from "../../../lib/worksheet";
import { Dialog } from "../../Dialog";
import { ErrorText } from "../../common";
import { SheetPreview } from "../../prep/SheetPreview";

const AUDIENCES: Array<[WsAudience, string]> = [
  ["student", "學生版"],
  ["teacher", "教師版"],
];

/**
 * 列印樣式預覽: the sheet as the server assembles it (the same projection the exports use), read
 * only, for the version the teacher is on. The workspace has saved before it opens.
 */
export function PrintPreviewDialog({
  worksheetId,
  levels,
  level: initialLevel,
  audience: initialAudience,
  returnFocus,
  onClose,
}: {
  worksheetId: number;
  levels: WsLevel[];
  level: WsLevel;
  audience: WsAudience;
  returnFocus: () => HTMLElement | null;
  onClose: () => void;
}) {
  const ids = useId();
  const [audience, setAudience] = useState<WsAudience>(initialAudience);
  const [level, setLevel] = useState<WsLevel>(initialLevel);
  const [sheet, setSheet] = useState<WsSheet | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .worksheetSheet(worksheetId, audience, level)
      .then((s) => {
        if (!alive) return;
        setSheet(s);
        setError(null);
      })
      .catch((err: unknown) => {
        if (alive) setError(errorMessage(err));
      });
    return () => {
      alive = false;
    };
  }, [worksheetId, audience, level]);

  // Stable: the preview reads the pictures through it.
  const loadImage = useCallback((imageId: number) => api.worksheetImage(worksheetId, imageId), [worksheetId]);

  return (
    <Dialog title="列印樣式預覽" size="large" returnFocus={returnFocus} onClose={onClose}>
      <div className="ws-print-preview" lang="zh-Hant">
        <p className="quiet-note">這是匯出、列印出來的樣子（只能看，要改請回學習單）。</p>
        <div className="ws-tool-row">
          <div className="ws-seg" role="radiogroup" aria-label="學生版或教師版">
            {AUDIENCES.map(([a, label]) => (
              <label key={a}>
                <input type="radio" name={`${ids}-aud`} checked={audience === a} onChange={() => setAudience(a)} />
                <span>{label}</span>
              </label>
            ))}
          </div>
          {levels.length > 1 && (
            <div className="ws-tabs" role="tablist" aria-label="版本程度">
              {levels.map((l) => (
                <button
                  key={l}
                  type="button"
                  role="tab"
                  aria-selected={l === level}
                  className={l === level ? "is-current" : ""}
                  onClick={() => setLevel(l)}
                >
                  {LEVEL_LABEL[l]}
                </button>
              ))}
            </div>
          )}
        </div>
        <ErrorText message={error} />
        {!sheet && !error && <p className="quiet-note">載入中…</p>}
        {sheet && (
          <SheetPreview
            sheet={sheet}
            pictures={{ worksheetId, load: loadImage }}
          />
        )}
      </div>
    </Dialog>
  );
}
