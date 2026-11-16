import { useEffect, useRef } from "react";
import { adminLibraryConfig } from "./prefill";

/**
 * While `active` (a template being read, a scan being recognised), the admin list is asked for
 * again every few seconds (one request at a time; the next only after the answer) and its news
 * handed to `apply`.
 */
export function useListPoll<T>(active: boolean, load: () => Promise<{ items: T[] }>, apply: (items: T[]) => void) {
  const loadRef = useRef(load);
  const applyRef = useRef(apply);
  useEffect(() => {
    loadRef.current = load;
    applyRef.current = apply;
  });
  useEffect(() => {
    if (!active) return undefined;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const next = () => {
      timer = setTimeout(() => {
        loadRef
          .current()
          .then((res) => {
            if (stopped) return;
            applyRef.current(res.items);
            next();
          })
          .catch(() => {
            // A missed poll is not a failure; the next one tries again.
            if (!stopped) next();
          });
      }, adminLibraryConfig.pollMs);
    };
    next();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [active]);
}
