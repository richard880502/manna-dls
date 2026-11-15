import { createContext, useCallback, useContext, useEffect, useId, useMemo, useState, type ReactNode } from "react";

interface Scope {
  owners: string[];
  claim: (id: string) => void;
  release: (id: string) => void;
}

const OnceContext = createContext<Scope | null>(null);

/**
 * A page where something (the privacy reminder) may appear in several places but should be said
 * only once: the first place that asks shows it, the others stay quiet until it is gone.
 */
export function OncePerPage({ children }: { children: ReactNode }) {
  const [owners, setOwners] = useState<string[]>([]);
  const claim = useCallback((id: string) => setOwners((o) => (o.includes(id) ? o : [...o, id])), []);
  const release = useCallback((id: string) => setOwners((o) => o.filter((x) => x !== id)), []);
  const value = useMemo(() => ({ owners, claim, release }), [owners, claim, release]);
  return <OnceContext.Provider value={value}>{children}</OnceContext.Provider>;
}

/** True when this place should show the once-per-page thing (always true outside a scope). */
export function useFirstOnPage(): boolean {
  const scope = useContext(OnceContext);
  const id = useId();
  const claim = scope?.claim;
  const release = scope?.release;
  useEffect(() => {
    if (!claim || !release) return undefined;
    claim(id);
    return () => release(id);
  }, [claim, release, id]);
  if (!scope) return true;
  return scope.owners.length === 0 || scope.owners[0] === id;
}
