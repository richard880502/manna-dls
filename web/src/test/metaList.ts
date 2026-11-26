import { waitFor } from "@testing-library/react";

/** The text of each item of one MetaList element. */
export const itemTexts = (ul: Element): string[] =>
  [...ul.children].map((li) => (li.textContent ?? "").replace(/\s+/g, " ").trim());

/** The items of every 「MetaList」 under `scope`, as the text of each list item. */
export function metaItems(scope: ParentNode = document.body): string[][] {
  return [...scope.querySelectorAll("ul.meta-list")].map((ul) =>
    [...ul.children].map((li) => (li.textContent ?? "").replace(/\s+/g, " ").trim()),
  );
}

const same = (a: string[], b: string[]) => a.length === b.length && a.every((x, i) => x === b[i]);

/** The MetaList under `scope` whose items are exactly `items`; throws when there is none. */
export function getMeta(items: string[], scope: ParentNode = document.body): HTMLElement {
  const lists = [...scope.querySelectorAll<HTMLElement>("ul.meta-list")];
  const hit = lists.find((ul) => same([...ul.children].map((li) => (li.textContent ?? "").replace(/\s+/g, " ").trim()), items));
  if (!hit) throw new Error(`No meta list with ${JSON.stringify(items)}; found ${JSON.stringify(metaItems(scope))}`);
  return hit;
}

/** Like `getMeta`, but waits for it to appear. */
export const findMeta = (items: string[], scope: ParentNode = document.body) => waitFor(() => getMeta(items, scope));

/** The row items of a list, leaving out the items of any MetaList inside them. */
export const rowItems = (els: HTMLElement[]) => els.filter((el) => !el.closest("ul.meta-list"));
