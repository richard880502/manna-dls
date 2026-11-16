import { EXTRACTING_LINE } from "../../lib/templates";

/** The line shown while Manna reads a worksheet's design. */
export function ExtractingLine() {
  return (
    <p className="manna-label template-extracting" role="status">
      <span className="star is-working" aria-hidden="true" />
      {EXTRACTING_LINE}
    </p>
  );
}
