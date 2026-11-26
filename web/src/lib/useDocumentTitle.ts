import { useEffect } from "react";

const SITE = "Manna DLS";

/** The tab title for a page: 「{page}｜Manna DLS」, or just 「Manna DLS」 when there is no page name (yet). */
export function pageTitle(page?: string | null): string {
  const name = page?.trim();
  return name ? `${name}｜${SITE}` : SITE;
}

/** Sets `document.title` while the page is mounted (and whenever `page` changes); restores the previous title on leaving. */
export function useDocumentTitle(page?: string | null) {
  useEffect(() => {
    const before = document.title;
    document.title = pageTitle(page);
    return () => {
      document.title = before;
    };
  }, [page]);
}
