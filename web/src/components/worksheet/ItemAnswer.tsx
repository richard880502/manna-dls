// Where a question is answered on the sheet, by its 作答方式 (§5.3), drawn the way the printed
// sheet draws it (api/src/worksheet/export.rs): the choices of 選擇題／是非題／配對題, the table of
// 表格整理, the box of 圖像組織 (its chart, when it has one, is Organizer.tsx's) and the writing lines of 簡答題／段落寫作. Everything the teacher
// can change here is part of the question and saved with it.
import type { WsItem, WsItemTable } from "../../api/types";
import {
  MAX_TABLE_COLUMNS,
  MAX_TABLE_ROWS,
  hasRowLabels,
  namedColumns,
  optionLabel,
  tableOf,
  usesChoices,
} from "../../lib/worksheet";
import { newOrganizer } from "../../lib/organizer";
import { SheetText } from "./SheetText";

type ItemChange = (change: (it: WsItem) => WsItem) => void;

const MAX_OPTIONS = 12;
const letter = (i: number) => String.fromCharCode(65 + (i % 26));

/** What the prompt field says while it is empty: what this format asks for. */
const PROMPT_PLACEHOLDER: Record<string, string> = {
  填空題: "A sentence with a ______ to fill in",
  選擇題: "Question (students choose one answer)",
  是非題: "Statement (students mark it true or false)",
  配對題: "Items to match: 1. …  2. …  3. …",
  簡答題: "Question",
  表格整理: "Complete a table with two columns: Pros and Cons. (what each row asks for)",
  圖像組織: "What to draw or fill in",
  段落寫作: "Writing task",
};
export const promptPlaceholder = (format: string | null) => (format && PROMPT_PLACEHOLDER[format]) || "Question";

/**
 * The choices under a question, printed (A), (B), …: edited one by one, for the formats that use
 * them. A format that does not keeps them unprinted, and says so; a 是非題 says what it set aside.
 */
export function ChoicesArea({
  item,
  no,
  disabled,
  onChange,
}: {
  item: WsItem;
  no: number;
  disabled: boolean;
  onChange: ItemChange;
}) {
  const format = item.answer_format;
  const kept = item.kept_options ?? [];
  const keptNote = kept.length > 0 && (
    <p className="quiet-note ws-kept" lang="zh-Hant">
      改成是非題前的 {kept.length} 個選項先收起來了（不會印出）；改回選擇題或配對題就會放回來。
    </p>
  );
  if (!usesChoices(format) || (format === null && item.options.length === 0)) {
    const hidden = item.options.filter((o) => o.trim() !== "");
    return (
      <>
        {hidden.length > 0 && (
          <p className="quiet-note ws-kept" lang="zh-Hant">
            這題原本的 {hidden.length} 個選項還留著，{format ?? "這個作答方式"}不會印出；改回選擇題、是非題或配對題就會出現。
            <button
              type="button"
              className="text-link is-plain"
              disabled={disabled}
              onClick={() => onChange((it) => ({ ...it, options: [] }))}
            >
              刪掉這些選項
            </button>
          </p>
        )}
        {keptNote}
      </>
    );
  }
  const setOptions = (options: string[]) => onChange((it) => ({ ...it, options }));
  return (
    <div className="ws-choices" role="group" aria-label={`選項 ${no}`}>
      <ol>
        {item.options.map((o, i) => (
          // oxlint-disable-next-line react/no-array-index-key -- plain strings, edited in place
          <li key={i}>
            <span className="ws-choice-letter" aria-hidden="true">
              {optionLabel(i)}
            </span>
            <SheetText
              label={`選項 ${letter(i)}（題目 ${no}）`}
              value={o}
              placeholder={`Option ${letter(i)}`}
              disabled={disabled}
              onChange={(text) => setOptions(item.options.map((x, j) => (j === i ? text : x)))}
            />
            <button
              type="button"
              className="text-link is-plain"
              lang="zh-Hant"
              aria-label={`刪除選項 ${letter(i)}（題目 ${no}）`}
              disabled={disabled}
              onClick={() => setOptions(item.options.filter((_, j) => j !== i))}
            >
              刪除
            </button>
          </li>
        ))}
      </ol>
      <p className="ws-add" lang="zh-Hant">
        <button
          type="button"
          className="text-link is-plain"
          aria-label={`加一個選項（題目 ${no}）`}
          disabled={disabled || item.options.length >= MAX_OPTIONS}
          onClick={() => setOptions([...item.options, ""])}
        >
          加一個選項
        </button>
      </p>
      {keptNote}
    </div>
  );
}

/**
 * 表格整理: the table students fill in, with its headings and row labels editable and its size
 * adjustable. The headings name what is compared and the row labels what each row asks for
 * (a first column, printed under an empty corner); a heading left empty or still 「Column 1」 does not
 * print, and says so here.
 */
function TableArea({ item, no, disabled, onChange }: { item: WsItem; no: number; disabled: boolean; onChange: ItemChange }) {
  const t = tableOf(item);
  const fromPrompt = !item.table && namedColumns(item.prompt) !== null;
  const set = (next: WsItemTable) => onChange((it) => ({ ...it, table: next }));
  const cols = t.columns.length;
  const labelled = hasRowLabels(t);
  const labels = Array.from({ length: t.rows }, (_, r) => t.row_labels?.[r] ?? "");
  const blankHeading = t.columns.some((c) => c.trim() === "");
  const setRows = (rows: number) =>
    set({ ...t, rows, ...(labelled ? { row_labels: Array.from({ length: rows }, (_, r) => t.row_labels?.[r] ?? "") } : {}) });
  return (
    <div className="ws-answer-table">
      <table aria-label={`作答表格 ${no}`}>
        <thead>
          <tr>
            {labelled && (
              // oxlint-disable-next-line jsx-a11y/control-has-associated-label -- the corner over the row labels
              <td className="ws-table-corner" />
            )}
            {t.columns.map((c, i) => (
              // oxlint-disable-next-line react/no-array-index-key -- columns are plain strings in order
              <th key={i} scope="col">
                <SheetText
                  label={`欄名 ${i + 1}（題目 ${no}）`}
                  value={c}
                  placeholder="寫上要比較的事物"
                  disabled={disabled}
                  onChange={(text) => set({ ...t, columns: t.columns.map((x, j) => (j === i ? text : x)) })}
                />
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {Array.from({ length: t.rows }, (_, r) => (
            // oxlint-disable-next-line react/no-array-index-key -- rows to write in
            <tr key={r}>
              {labelled && (
                <th scope="row">
                  <SheetText
                    label={`列標籤 ${r + 1}（題目 ${no}）`}
                    value={labels[r] ?? ""}
                    placeholder="這一列要寫什麼"
                    disabled={disabled}
                    onChange={(text) => set({ ...t, row_labels: labels.map((x, j) => (j === r ? text : x)) })}
                  />
                </th>
              )}
              {t.columns.map((_, c) => (
                // oxlint-disable-next-line react/no-array-index-key, jsx-a11y/control-has-associated-label -- empty cells the students write in
                <td key={c} />
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      <p className="ws-table-tools" lang="zh-Hant">
        <span>
          {cols} 欄 × {t.rows} 列
        </span>
        <button
          type="button"
          className="text-link is-plain"
          aria-label={`加一欄（題目 ${no}）`}
          disabled={disabled || cols >= MAX_TABLE_COLUMNS}
          onClick={() => set({ ...t, columns: [...t.columns, ""] })}
        >
          加一欄
        </button>
        <button
          type="button"
          className="text-link is-plain"
          aria-label={`少一欄（題目 ${no}）`}
          disabled={disabled || cols <= 1}
          onClick={() => set({ ...t, columns: t.columns.slice(0, -1) })}
        >
          少一欄
        </button>
        <button
          type="button"
          className="text-link is-plain"
          aria-label={`加一列（題目 ${no}）`}
          disabled={disabled || t.rows >= MAX_TABLE_ROWS}
          onClick={() => setRows(t.rows + 1)}
        >
          加一列
        </button>
        <button
          type="button"
          className="text-link is-plain"
          aria-label={`少一列（題目 ${no}）`}
          disabled={disabled || t.rows <= 1}
          onClick={() => setRows(t.rows - 1)}
        >
          少一列
        </button>
        <button
          type="button"
          className="text-link is-plain"
          aria-label={labelled ? `拿掉列標籤（題目 ${no}）` : `加列標籤（題目 ${no}）`}
          disabled={disabled}
          onClick={() => set({ ...t, row_labels: labelled ? [] : Array.from({ length: t.rows }, () => "") })}
        >
          {labelled ? "拿掉列標籤" : "加列標籤"}
        </button>
        {fromPrompt && !blankHeading && <span className="quiet-note">欄名取自題目；改了欄名就照你的。</span>}
      </p>
      {blankHeading && (
        <p className="ws-table-warning" role="note" lang="zh-Hant">
          有欄名還是空白（沒有填的欄名不會印出）：請寫上這一欄要比較的事物，學生才知道要比什麼。
        </p>
      )}
      {!labelled && cols === 2 && (
        <p className="ws-table-warning" role="note" lang="zh-Hant">
          還沒有列標籤：兩欄的表格通常要說明每一列比什麼（例如 Claim、Supporting detail），可以按「加列標籤」。
        </p>
      )}
    </div>
  );
}

/**
 * The room to answer under a question (and its support): the table of 表格整理, the box of
 * 圖像組織 without an organizer (with the way to make it a chart), else `lines` writing lines (none for the formats answered on the choices or in the
 * blanks).
 */
export function WritingArea({
  item,
  no,
  lines,
  disabled,
  onChange,
}: {
  item: WsItem;
  no: number;
  lines: number;
  disabled: boolean;
  onChange: ItemChange;
}) {
  if (item.answer_format === "表格整理") return <TableArea item={item} no={no} disabled={disabled} onChange={onChange} />;
  if (item.answer_format === "圖像組織") {
    // A question with an organizer is drawn by OrganizerEditor instead; one without (made before
    // organizers) prints this box until the teacher turns it into a chart.
    return (
      <>
        <div className="ws-organizer" role="img" aria-label={`圖像組織作答框 ${no}`} />
        <p className="ws-org-missing" lang="zh-Hant">
          這一題印成空白方框。
          <button
            type="button"
            className="text-link is-plain"
            disabled={disabled}
            onClick={() => onChange((it) => ({ ...it, organizer: newOrganizer() }))}
          >
            改成圖表
          </button>
        </p>
      </>
    );
  }
  if (lines === 0) return null;
  return (
    <div className="ws-lines" role="img" aria-label={`作答線 ${no}：${lines} 行`}>
      {Array.from({ length: lines }, (_, i) => (
        <span key={i} className="ws-line" />
      ))}
    </div>
  );
}
