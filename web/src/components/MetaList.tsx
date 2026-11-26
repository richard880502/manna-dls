import type { ReactNode } from "react";

/**
 * A line of independent facts (「PDF」「7 頁」「約 2,900 字」): one list item each, spaced by the
 * stylesheet instead of a separator glyph, so a screen reader reads them one by one. Empty items
 * are dropped; with nothing left nothing is rendered.
 */
export function MetaList({
  items,
  className,
  ...rest
}: { items: ReadonlyArray<ReactNode | null | undefined | false | "">; className?: string } & Omit<
  React.HTMLAttributes<HTMLUListElement>,
  "children" | "className"
>) {
  const shown = items.filter((x) => x !== null && x !== undefined && x !== false && x !== "");
  if (shown.length === 0) return null;
  return (
    <ul {...rest} className={className ? `meta-list ${className}` : "meta-list"}>
      {shown.map((x, i) => (
        <li key={i}>{x}</li>
      ))}
    </ul>
  );
}
