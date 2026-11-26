import { useCallback, useState } from "react";
import { useNavigate } from "react-router";
import { ApiError, api, errorMessage } from "../../api/client";
import type { CommunityMeta } from "../../api/types";

/**
 * 收藏 and 拿來改 for a tile or the preview. `onChange` hears the new numbers (a save toggled) or
 * `null` when the worksheet is no longer shared (the server says 404).
 */
export function useCardActions(onChange: (id: number, change: Partial<CommunityMeta> | null) => void) {
  const navigate = useNavigate();
  const [busyId, setBusyId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = useCallback(
    async (card: CommunityMeta, action: () => Promise<void>) => {
      setBusyId(card.id);
      setError(null);
      try {
        await action();
      } catch (err) {
        setError(errorMessage(err));
        if (err instanceof ApiError && err.status === 404) onChange(card.id, null);
      } finally {
        setBusyId(null);
      }
    },
    [onChange],
  );

  // Optimistic, like the 有幫助／不合用 ratings: the tile changes at once and goes back when the
  // server refuses (a 404 removes it instead, as the worksheet is no longer shared).
  const toggleSave = useCallback(
    async (card: CommunityMeta) => {
      const saving = !card.saved;
      setBusyId(card.id);
      setError(null);
      onChange(card.id, { saved: saving, saves: saving ? card.saves + 1 : Math.max(0, card.saves - 1) });
      try {
        await (saving ? api.community.save(card.id) : api.community.unsave(card.id));
      } catch (err) {
        if (err instanceof ApiError && err.status === 404) onChange(card.id, null);
        else onChange(card.id, { saved: card.saved, saves: card.saves });
        setError(errorMessage(err));
      } finally {
        setBusyId(null);
      }
    },
    [onChange],
  );

  const remix = useCallback(
    (card: CommunityMeta) =>
      run(card, async () => {
        const { worksheet_id } = await api.community.remix(card.id);
        void navigate(`/worksheets/${worksheet_id}`);
      }),
    [run, navigate],
  );

  return { busyId, error, toggleSave, remix };
}
