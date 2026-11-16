import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "../../api/client";

/** How long typing rests before the search is sent, and how many cards a page asks for. Tests shorten the delay. */
export const libraryConfig = { searchDelayMs: 300, pageSize: 24, maxPageSize: 60 };

export interface BrowsePage<T, F> {
  items: T[];
  next_cursor: string | null;
  facets: F;
  total: number;
}

export interface BrowseResult<T, F> {
  /** null until the first answer for this query. */
  items: T[] | null;
  facets: F | null;
  total: number;
  hasMore: boolean;
  /** A new query is on its way (the cards shown are the last query's). */
  loading: boolean;
  loadingMore: boolean;
  error: string | null;
  loadMore: () => void;
  /** Asks again for what is shown (as many cards as are loaded, up to the page cap): polling. */
  refresh: () => void;
  retry: () => void;
  /** Replaces one card in place (e.g. a template that is being read again). */
  update: (match: (item: T) => boolean, next: T) => void;
}

/**
 * One browse list (cards with a cursor): the first page whenever the query changes (`key`),
 * 載入更多 for the next, answers that come back out of order are dropped.
 */
export function useBrowse<T, F>(
  load: (cursor: string | null, limit: number) => Promise<BrowsePage<T, F>>,
  key: string,
): BrowseResult<T, F> {
  const loadRef = useRef(load);
  useEffect(() => {
    loadRef.current = load;
  }, [load]);

  const [items, setItems] = useState<T[] | null>(null);
  const [facets, setFacets] = useState<F | null>(null);
  const [total, setTotal] = useState(0);
  const [next, setNext] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [seq, setSeq] = useState(0);
  /** Which query (and retry) the shown answer is for: loading while it is not the current one. */
  const wanted = `${key}#${seq}`;
  const [answered, setAnswered] = useState<string | null>(null);
  /** The latest request; an older answer is ignored. */
  const latest = useRef(0);
  const shown = useRef(0);
  useEffect(() => {
    shown.current = items?.length ?? 0;
  }, [items]);

  const take = useCallback((page: BrowsePage<T, F>) => {
    setItems(page.items);
    setFacets(page.facets);
    setTotal(page.total);
    setNext(page.next_cursor);
    setError(null);
  }, []);

  useEffect(() => {
    const mine = latest.current + 1;
    latest.current = mine;
    let alive = true;
    const query = `${key}#${seq}`;
    loadRef
      .current(null, libraryConfig.pageSize)
      .then((page) => {
        if (!alive || latest.current !== mine) return;
        take(page);
      })
      .catch((err: unknown) => {
        if (!alive || latest.current !== mine) return;
        setError(errorMessage(err));
      })
      .finally(() => {
        if (alive && latest.current === mine) {
          setAnswered(query);
          setLoadingMore(false);
        }
      });
    return () => {
      alive = false;
    };
  }, [key, seq, take]);

  const loadMore = useCallback(() => {
    if (!next || loadingMore) return;
    const mine = latest.current;
    setLoadingMore(true);
    loadRef
      .current(next, libraryConfig.pageSize)
      .then((page) => {
        if (latest.current !== mine) return;
        setItems((list) => [...(list ?? []), ...page.items]);
        setNext(page.next_cursor);
        setError(null);
      })
      .catch((err: unknown) => {
        if (latest.current === mine) setError(errorMessage(err));
      })
      .finally(() => setLoadingMore(false));
  }, [next, loadingMore]);

  const refresh = useCallback(() => {
    const mine = latest.current + 1;
    latest.current = mine;
    const limit = Math.min(libraryConfig.maxPageSize, Math.max(libraryConfig.pageSize, shown.current));
    loadRef
      .current(null, limit)
      .then((page) => {
        if (latest.current === mine) take(page);
      })
      .catch(() => {
        // A missed poll is not a failure; the next one tries again.
      });
  }, [take]);

  const retry = useCallback(() => setSeq((n) => n + 1), []);

  const update = useCallback((match: (item: T) => boolean, nextItem: T) => {
    setItems((list) => list?.map((item) => (match(item) ? nextItem : item)) ?? list);
  }, []);

  return {
    items,
    facets,
    total,
    hasMore: next !== null,
    loading: answered !== wanted,
    loadingMore,
    error,
    loadMore,
    refresh,
    retry,
    update,
  };
}

/** The search box's words, sent once typing rests (or at once on Enter). */
export function useSearchText() {
  const [text, setText] = useState("");
  const [q, setQ] = useState("");
  useEffect(() => {
    const trimmed = text.trim();
    if (trimmed === q) return undefined;
    const timer = setTimeout(() => setQ(trimmed), libraryConfig.searchDelayMs);
    return () => clearTimeout(timer);
  }, [text, q]);
  const submit = useCallback(() => setQ(text.trim()), [text]);
  const clear = useCallback(() => {
    setText("");
    setQ("");
  }, []);
  return { text, setText, q, submit, clear };
}
