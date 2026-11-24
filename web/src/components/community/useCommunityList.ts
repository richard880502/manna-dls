import { useCallback, useEffect, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { CommunityCard, CommunityQuery } from "../../api/types";

interface Loaded {
  /** The query (and retry count) this was loaded for: a different one is loading again. */
  key: string;
  items: CommunityCard[];
  total: number;
  unavailable: number;
  error: string | null;
}

/** The pages of 探索 or 我的收藏: the first page for a query, and 顯示更多 after it. */
export function useCommunityList(kind: "explore" | "saved", query: CommunityQuery) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [moreError, setMoreError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const fetcher = kind === "explore" ? api.community.explore : api.community.saved;
  const { q, sort, output, level } = query;
  const key = JSON.stringify([kind, q, sort, output, level, attempt]);

  useEffect(() => {
    let alive = true;
    fetcher({ q, sort, output, level })
      .then((page) => {
        if (alive)
          setLoaded({ key, items: page.items, total: page.total, unavailable: page.unavailable ?? 0, error: null });
      })
      .catch((err: unknown) => {
        if (alive) setLoaded({ key, items: [], total: 0, unavailable: 0, error: errorMessage(err) });
      });
    return () => {
      alive = false;
    };
  }, [fetcher, key, q, sort, output, level]);

  const current = loaded !== null && loaded.key === key ? loaded : null;

  const loadMore = useCallback(async () => {
    if (current === null) return;
    setLoadingMore(true);
    setMoreError(null);
    try {
      const page = await fetcher({ q, sort, output, level, offset: current.items.length });
      setLoaded((now) =>
        now === null || now.key !== key
          ? now
          : {
              ...now,
              total: page.total,
              items: [...now.items, ...page.items.filter((c) => !now.items.some((x) => x.id === c.id))],
            },
      );
    } catch (err) {
      setMoreError(errorMessage(err));
    } finally {
      setLoadingMore(false);
    }
  }, [fetcher, current, key, q, sort, output, level]);

  /** Changes one tile in place (a save toggled), or takes it out (`null`: no longer there). */
  const patch = useCallback((id: number, change: Partial<CommunityCard> | null) => {
    setLoaded((now) =>
      now === null
        ? now
        : {
            ...now,
            items:
              change === null
                ? now.items.filter((c) => c.id !== id)
                : now.items.map((c) => (c.id === id ? { ...c, ...change } : c)),
          },
    );
  }, []);

  return {
    items: current?.items ?? null,
    total: current?.total ?? 0,
    unavailable: current?.unavailable ?? 0,
    error: current?.error ?? moreError,
    loadingMore,
    loadMore,
    patch,
    retry: () => setAttempt((n) => n + 1),
  };
}
