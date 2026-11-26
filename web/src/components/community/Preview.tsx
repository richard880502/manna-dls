import { useCallback, useEffect, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { CommunityMeta, CommunityPreview, WsLevel } from "../../api/types";
import { LEVEL_LABEL } from "../../lib/worksheet";
import { Dialog } from "../Dialog";
import { ErrorText, Loading } from "../common";
import { SheetPreview } from "../prep/SheetPreview";
import { Counts } from "./Counts";
import { outputLabel } from "./labels";


/**
 * The near full-screen preview of one shared worksheet: what is printed on it (student version,
 * read-only) beside the owner's name as they chose to show it, the counts and the actions.
 * Focus stays inside, Esc and × close it and focus goes back to the tile that opened it.
 */
export function PreviewOverlay({
  card,
  busy,
  onClose,
  onToggleSave,
  onRemix,
}: {
  card: CommunityMeta;
  busy: boolean;
  onClose: () => void;
  onToggleSave: (card: CommunityMeta) => void;
  onRemix: (card: CommunityMeta) => void;
}) {
  const [level, setLevel] = useState<WsLevel | null>(null);
  const [loaded, setLoaded] = useState<{ key: string; preview: CommunityPreview | null; error: string | null } | null>(
    null,
  );
  const key = `${card.id}:${level ?? ""}`;
  useEffect(() => {
    let alive = true;
    api.community
      .preview(card.id, level ?? undefined)
      .then((preview) => alive && setLoaded({ key, preview, error: null }))
      .catch((err: unknown) => alive && setLoaded({ key, preview: null, error: errorMessage(err) }));
    return () => {
      alive = false;
    };
  }, [card.id, level, key]);
  const current = loaded?.key === key ? loaded : null;
  const preview = current?.preview ?? null;
  const load = useCallback((imageId: number) => api.community.picture(card.id, imageId), [card.id]);
  const levels = card.levels;
  const kind = outputLabel(card.output);

  return (
    <Dialog title={card.title || "未命名的學習單"} size="large" className="com-dialog" onClose={onClose}>
      <div className="com-ov">
        <div className="com-ov-sheet">
          {!current && <Loading label="正在打開學習單…" />}
          <ErrorText message={current?.error} />
          {preview && (
            <SheetPreview
              sheet={preview.sheet}
              pictures={{ worksheetId: card.id, load }}
            />
          )}
        </div>
        <aside className="com-ov-side" aria-label="這份學習單">
          <p className="com-by">
            <span className="com-owner">{card.is_mine ? `${card.owner_name}（你）` : card.owner_name}</span>
            {kind && <span className="soft-tag">{kind}</span>}
          </p>
          <Counts saves={card.saves} remixes={card.remixes} />
          {levels.length > 1 && (
            <div className="com-levels" role="group" aria-label="版本">
              {levels.map((l) => (
                <button
                  key={l}
                  type="button"
                  className="lib-filter"
                  aria-pressed={(level ?? preview?.level ?? levels[0]) === l}
                  onClick={() => setLevel(l)}
                >
                  {LEVEL_LABEL[l]}
                </button>
              ))}
            </div>
          )}
          {card.is_mine ? (
            <p className="field-hint">這是你分享的學習單，其他老師看到的就是這個樣子。</p>
          ) : (
            <div className="button-row">
              <button
                type="button"
                className="secondary-button"
                aria-pressed={card.saved}
                aria-disabled={busy || undefined}
                onClick={() => {
                  if (!busy) onToggleSave(card);
                }}
              >
                收藏
              </button>
              {card.allow_remix ? (
                <button type="button" className="primary-button" disabled={busy} onClick={() => onRemix(card)}>
                  拿來改
                </button>
              ) : (
                <p className="field-hint">這份學習單的老師沒有開放改編。</p>
              )}
            </div>
          )}
          {!card.is_mine && card.allow_remix && (
            <p className="field-hint">「拿來改」會在你的作品裡做一份自己的副本，原作者不會知道是誰改的。</p>
          )}
        </aside>
      </div>
    </Dialog>
  );
}
