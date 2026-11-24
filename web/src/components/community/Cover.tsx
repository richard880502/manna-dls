import { useEffect, useRef, useState } from "react";
import type { WsSheet } from "../../api/types";
import { SheetPreview } from "../prep/SheetPreview";

/** The width the sheet is laid out in before it is scaled down to the cover. */
const DESIGN_WIDTH = 640;
/** Covers have no pictures: a loader that is never asked. */
const NO_PICTURES = { worksheetId: 0, load: () => Promise.reject(new Error("no pictures on a cover")) };

/**
 * The top of a worksheet's student sheet, drawn with the real sheet component and scaled down to
 * fit the box (CSS transform). It is a picture of the content, not a control: the card's button
 * around it carries the name.
 */
export function Cover({ sheet, tint }: { sheet: WsSheet | null; tint: number }) {
  const box = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(0.4);
  useEffect(() => {
    const el = box.current;
    if (!el) return undefined;
    const fit = () => setScale(Math.max(0.1, (el.clientWidth * 0.8) / DESIGN_WIDTH));
    fit();
    if (typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return (
    <div className={`com-cover com-tint-${tint % 6}`} ref={box} aria-hidden="true">
      {sheet ? (
        <div className="com-cover-stage" style={{ width: DESIGN_WIDTH, transform: `scale(${scale})` }}>
          <SheetPreview sheet={sheet} highlight={NO_HIGHLIGHT} interactionFor={NO_INTERACTION} readOnly={NO_PICTURES} />
        </div>
      ) : (
        <p className="com-cover-empty">還沒有內容</p>
      )}
    </div>
  );
}

const NO_HIGHLIGHT: ReadonlySet<string> = new Set();
const NO_INTERACTION = () => null;
