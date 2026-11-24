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

  const toggleSave = useCallback(
    (card: CommunityMeta) =>
      run(card, async () => {
        if (card.saved) {
          await api.community.unsave(card.id);
          onChange(card.id, { saved: false, saves: Math.max(0, card.saves - 1) });
        } else {
          await api.community.save(card.id);
          onChange(card.id, { saved: true, saves: card.saves + 1 });
        }
      }),
    [run, onChange],
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
