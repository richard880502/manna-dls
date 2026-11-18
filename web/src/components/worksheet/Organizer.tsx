import { Fragment, useRef, type ReactNode } from "react";
import type { WsLevel, WsOrganizer, WsOrganizerType, WsSheetOrganizer } from "../../api/types";
import { LEVEL_LABEL } from "../../lib/worksheet";
import {
  CHAIN_PER_ROW,
  ORGANIZER_LABEL,
  ORGANIZER_TYPES,
  addSection,
  convertOrganizer,
  givenIn,
  maxCells,
  organizerFamily,
  organizerRows,
  patchCell,
  patchSection,
  projectOrganizer,
  removeSection,
  sectionRange,
  togglePrefill,
  webRows,
  webSlots,
} from "../../lib/organizer";
import { useAutoGrow } from "../../lib/useAutoGrow";

/** What the editor puts in place of the printed texts. */
export interface OrganizerSlots {
  title: ReactNode;
  center: ReactNode;
  label: (si: number) => ReactNode;
  cell: (si: number, ci: number) => ReactNode;
  /** Under a section's cells: add or remove a cell, remove the section. */
  sectionTools: (si: number) => ReactNode;
}

type SheetCell = WsSheetOrganizer["sections"][number]["cells"][number];

/** A printed cell: pre-filled text, the teacher's answer (gold), or a line to write on. */
function PrintedCell({ cell, line = true }: { cell: SheetCell; line?: boolean }) {
  if (cell.given) return <span className="ws-org-given">{cell.given}</span>;
  if (cell.answer) return <span className="ws-org-key">{cell.answer}</span>;
  return line ? <span className="ws-org-blank" aria-hidden="true" /> : null;
}

/**
 * An organizer drawn the way the DOCX and PDF draw it (api/src/worksheet/export/organizer.rs):
 * headed columns, numbered boxes with arrows, two circles, a centre with branches, or labelled rows.
 * The editor passes `slots` to put its fields in place of the printed texts.
 */
export function OrganizerView({ org, slots }: { org: WsSheetOrganizer; slots?: OrganizerSlots }) {
  const family = organizerFamily(org.type);
  const label = (si: number) => slots?.label(si) ?? <strong>{org.sections[si]?.label}</strong>;
  const cells = (si: number, line = true) =>
    (org.sections[si]?.cells ?? []).map((c, ci) => (
      // oxlint-disable-next-line react/no-array-index-key -- cells are positions, in print order
      <div key={ci} className="ws-org-cell">
        {slots ? slots.cell(si, ci) : <PrintedCell cell={c} line={line} />}
      </div>
    ));
  const tools = (si: number) => slots?.sectionTools(si) ?? null;
  const box = (si: number, head: ReactNode = label(si)) => (
    <div className="ws-org-box">
      <div className="ws-org-label">{head}</div>
      {cells(si)}
      {tools(si)}
    </div>
  );

  let body: ReactNode;
  if (family === "columns") {
    const rows = organizerRows(org);
    body = (
      <div className="ws-org-columns" style={{ gridTemplateColumns: `repeat(${org.sections.length}, minmax(0, 1fr))` }}>
        {org.sections.map((_, si) => (
          // oxlint-disable-next-line react/no-array-index-key -- sections are positions
          <div key={`h${si}`} className="ws-org-head">
            {label(si)}
          </div>
        ))}
        {Array.from({ length: rows }, (_, r) =>
          org.sections.map((s, si) => (
            // oxlint-disable-next-line react/no-array-index-key -- grid positions
            <div key={`${r}-${si}`} className="ws-org-cell is-grid">
              {s.cells[r] ? (slots ? slots.cell(si, r) : <PrintedCell cell={s.cells[r]} line={false} />) : null}
              {r === rows - 1 && tools(si)}
            </div>
          )),
        )}
      </div>
    );
  } else if (family === "rows") {
    body = (
      <div className="ws-org-rows">
        {org.sections.map((_, si) => (
          // oxlint-disable-next-line react/no-array-index-key -- sections are positions
          <Fragment key={si}>
            <div className="ws-org-head is-row">{label(si)}</div>
            <div className="ws-org-row-cells">
              {cells(si)}
              {tools(si)}
            </div>
          </Fragment>
        ))}
      </div>
    );
  } else if (family === "chain") {
    const chunks: number[][] = [];
    org.sections.forEach((_, si) => {
      if (si % CHAIN_PER_ROW === 0) chunks.push([]);
      chunks[chunks.length - 1]?.push(si);
    });
    body = (
      <div className="ws-org-chain">
        {chunks.map((chunk) => (
          <div key={chunk[0]} className="ws-org-chain-row">
            {chunk.map((si, k) => (
              <Fragment key={si}>
                {k > 0 && (
                  <span className="ws-org-arrow" aria-hidden="true">
                    →
                  </span>
                )}
                {box(
                  si,
                  <>
                    <span className="ws-org-no">{si + 1}.</span> {label(si)}
                  </>,
                )}
              </Fragment>
            ))}
          </div>
        ))}
      </div>
    );
  } else if (family === "venn") {
    body = (
      <div className="ws-org-venn">
        <div className="ws-org-venn-labels">
          {[0, 1, 2].map((si) => (
            <div key={si} className="ws-org-label">
              {org.sections[si] ? label(si) : null}
            </div>
          ))}
        </div>
        <div className="ws-org-venn-body">
          <svg className="ws-org-venn-circles" viewBox="0 0 100 60" preserveAspectRatio="none" aria-hidden="true">
            <ellipse cx="31" cy="30" rx="30.5" ry="29.4" />
            <ellipse cx="69" cy="30" rx="30.5" ry="29.4" />
          </svg>
          {[0, 1, 2].map((si) => (
            <div key={si} className="ws-org-venn-region">
              {cells(si)}
              {tools(si)}
            </div>
          ))}
        </div>
      </div>
    );
  } else {
    const [top, bottom] = webRows(org.sections.length);
    const branchRow = (indices: number[]) => {
      const slotsOf = webSlots(indices.length);
      return [0, 1, 2].map((col) => {
        const k = slotsOf.indexOf(col);
        const si = indices[k];
        return si === undefined ? <div key={col} /> : <Fragment key={col}>{box(si)}</Fragment>;
      });
    };
    const connector = (indices: number[], up: boolean) => (
      <svg className="ws-org-web-lines" viewBox="0 0 300 24" preserveAspectRatio="none" aria-hidden="true">
        {webSlots(indices.length).map((col) => (
          <line key={col} x1="150" y1={up ? 24 : 0} x2={50 + col * 100} y2={up ? 0 : 24} />
        ))}
      </svg>
    );
    body = (
      <div className="ws-org-web">
        <div className="ws-org-web-row">{branchRow(top)}</div>
        {connector(top, true)}
        <div className="ws-org-web-row">
          <div />
          <div className="ws-org-box is-centre">{slots?.center ?? <strong>{org.center}</strong>}</div>
          <div />
        </div>
        {bottom.length > 0 && (
          <>
            {connector(bottom, false)}
            <div className="ws-org-web-row">{branchRow(bottom)}</div>
          </>
        )}
      </div>
    );
  }

  return (
    <figure className={`ws-org is-${family}`} aria-label={org.name} data-organizer={org.type}>
      {slots ? (
        <figcaption className="ws-org-title">{slots.title}</figcaption>
      ) : (
        org.title && <figcaption className="ws-org-title">{org.title}</figcaption>
      )}
      {body}
    </figure>
  );
}

/** A field on the organizer that reads as text until it is focused. */
function OrgText({
  label,
  value,
  onChange,
  disabled,
  placeholder,
  className = "",
}: {
  label: string;
  value: string;
  onChange: (next: string) => void;
  disabled: boolean;
  placeholder?: string;
  className?: string;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useAutoGrow(ref, value);
  return (
    <textarea
      ref={ref}
      className={`ws-text ${className}`}
      rows={1}
      lang="en"
      aria-label={label}
      value={value}
      placeholder={placeholder}
      disabled={disabled}
      // One line per field: the server prints it as one.
      onChange={(e) => onChange(e.target.value.replace(/\n/g, " "))}
    />
  );
}

/**
 * The organizer of question `no` in the editor: its type, title, labels and cells, edited in place
 * on the drawing the sheet prints. The student view edits what each cell prints pre-filled in the
 * version shown; the teacher view edits the answers. Every change goes to `onChange` at once, so
 * the preview and the exports follow.
 */
export function OrganizerEditor({
  org,
  no,
  level,
  levels,
  perVersion,
  teacher,
  disabled,
  onChange,
}: {
  org: WsOrganizer;
  no: number;
  /** The version shown. */
  level: WsLevel;
  /** The worksheet's versions. */
  levels: readonly WsLevel[];
  /** A 差異化學習單 question: pre-filling is per version. */
  perVersion: boolean;
  teacher: boolean;
  disabled: boolean;
  onChange: (next: WsOrganizer) => void;
}) {
  const view = projectOrganizer(org, level, teacher);
  const [min, max] = sectionRange(org.type);
  const cap = maxCells(org.type);
  const where = (si: number, ci?: number) => `${no}-${si + 1}${ci === undefined ? "" : `-${ci + 1}`}`;
  const slots: OrganizerSlots = {
    title: (
      <OrgText
        label={`圖表標題 ${no}`}
        className="is-org-title"
        value={org.title}
        placeholder="Title (optional)"
        disabled={disabled}
        onChange={(title) => onChange({ ...org, title })}
      />
    ),
    center: (
      <OrgText
        label={`中心詞 ${no}`}
        value={org.center}
        placeholder="Main idea"
        disabled={disabled}
        onChange={(center) => onChange({ ...org, center })}
      />
    ),
    label: (si) => (
      <OrgText
        label={`格子標題 ${where(si)}`}
        className="is-org-label"
        value={org.sections[si]?.label ?? ""}
        placeholder="Heading"
        disabled={disabled}
        onChange={(label) => onChange(patchSection(org, si, (s) => ({ ...s, label })))}
      />
    ),
    cell: (si, ci) => {
      const cell = org.sections[si]?.cells[ci];
      if (!cell) return null;
      const given = givenIn(cell, level);
      const name = where(si, ci);
      return (
        <div className="ws-org-edit-cell">
          {teacher && !given ? (
            <OrgText
              label={`參考答案 ${name}`}
              className="ws-org-key"
              value={cell.answer}
              placeholder="Answer"
              disabled={disabled}
              onChange={(answer) => onChange(patchCell(org, si, ci, (c) => ({ ...c, answer })))}
            />
          ) : given ? (
            <OrgText
              label={`預填 ${name}`}
              value={cell.given}
              disabled={disabled}
              onChange={(text) => onChange(patchCell(org, si, ci, (c) => ({ ...c, given: text })))}
            />
          ) : (
            <span className="ws-org-blank" aria-hidden="true" />
          )}
          <span className="ws-org-cell-tools" lang="zh-Hant">
            <label>
              <input
                type="checkbox"
                checked={given}
                disabled={disabled}
                aria-label={`預填在學生版 ${name}`}
                onChange={() =>
                  onChange(patchCell(org, si, ci, (c) => togglePrefill(c, level, levels, perVersion)))
                }
              />
              <span>預填</span>
            </label>
            <button
              type="button"
              className="text-link is-plain"
              disabled={disabled || (org.sections[si]?.cells.length ?? 0) <= 1}
              aria-label={`刪除空格 ${name}`}
              onClick={() =>
                onChange(patchSection(org, si, (s) => ({ ...s, cells: s.cells.filter((_, i) => i !== ci) })))
              }
            >
              ×
            </button>
          </span>
        </div>
      );
    },
    sectionTools: (si) => {
      const count = org.sections[si]?.cells.length ?? 0;
      return (
        <p className="ws-org-section-tools" lang="zh-Hant">
          <button
            type="button"
            className="text-link is-plain"
            disabled={disabled || count >= cap}
            aria-label={`加一個空格 ${where(si)}`}
            onClick={() =>
              onChange(
                patchSection(org, si, (s) => ({ ...s, cells: [...s.cells, { answer: "", given: "", prefilled: [] }] })),
              )
            }
          >
            ＋空格
          </button>
          {org.sections.length > min && (
            <button
              type="button"
              className="text-link is-plain"
              disabled={disabled}
              aria-label={`刪除這一格 ${where(si)}`}
              onClick={() => onChange(removeSection(org, si))}
            >
              刪除這一格
            </button>
          )}
        </p>
      );
    },
  };
  return (
    <div className="ws-org-editor" role="group" aria-label={`圖像組織 ${no}`}>
      <p className="ws-org-tools" lang="zh-Hant">
        <label>
          <span>圖表類型</span>
          <select
            aria-label={`圖表類型 ${no}`}
            value={org.type}
            disabled={disabled}
            onChange={(e) => onChange(convertOrganizer(org, e.target.value as WsOrganizerType))}
          >
            {ORGANIZER_TYPES.map((t) => (
              <option key={t} value={t}>
                {ORGANIZER_LABEL[t]}
              </option>
            ))}
          </select>
        </label>
        {max > min && (
          <button
            type="button"
            className="text-link is-plain"
            disabled={disabled || org.sections.length >= max}
            aria-label={`加一格 ${no}`}
            onClick={() => onChange(addSection(org))}
          >
            加一格
          </button>
        )}
        <span className="ws-org-hint">
          {teacher ? "教師版：空白格印出參考答案。" : `勾「預填」的格子會印在${LEVEL_LABEL[level]}學生版當作示範。`}
        </span>
      </p>
      <OrganizerView org={view} slots={slots} />
    </div>
  );
}
