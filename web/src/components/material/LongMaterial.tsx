import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { api, errorMessage } from "../../api/client";
import type { MaterialDetail } from "../../api/types";
import {
  BACK_STEP,
  CHANGE_MATERIAL,
  EDIT_TEXT,
  GENERATE_ANYWAY,
  PICK_PARAGRAPHS,
  frontLabel,
  groupLabel,
  initialSelection,
  longMaterialLine,
  paragraphRuns,
  selectionToSave,
  tickState,
  toggleMany,
  toggleRange,
} from "../../lib/longMaterial";
import { Dialog, DialogCancel } from "../Dialog";
import { ErrorText, Loading } from "../common";

/**
 * Before generating a material of more than 40 paragraphs: it is only said, never refused. The
 * three ways on are the teacher's: fix the text, use part of it, or go on as it is.
 */
export function LongMaterialNotice({
  paragraphs,
  disabled,
  onEdit,
  onPick,
  onGo,
  goLabel = GENERATE_ANYWAY,
}: {
  paragraphs: number;
  disabled?: boolean;
  onEdit: () => void;
  onPick: () => void;
  /** 照樣產生: going on as it is. */
  onGo: () => void;
  goLabel?: string;
}) {
  const ids = useId();
  return (
    <div className="notice notice-info long-material-notice" role="group" aria-labelledby={`${ids}-line`}>
      <p id={`${ids}-line`}>{longMaterialLine(paragraphs)}</p>
      <div className="button-row">
        <button type="button" className="secondary-button small-button" disabled={disabled} onClick={onEdit}>
          {EDIT_TEXT}
        </button>
        <button type="button" className="secondary-button small-button" disabled={disabled} onClick={onPick}>
          {PICK_PARAGRAPHS}
        </button>
        <button type="button" className="primary-button small-button" disabled={disabled} onClick={onGo}>
          {goLabel}
        </button>
      </div>
    </div>
  );
}

/**
 * The buttons next to a message that says the material is the trouble. 回上一步 goes back to
 * where the material and the request were entered; 修改課文 and 只選要用的段落 change what the
 * next try uses; 再試一次 (or 重新產生) runs again. Every one of them keeps what the teacher typed.
 */
export function MaterialFixButtons({
  disabled,
  onBack,
  onEdit,
  onPick,
  onChange,
  onRetry,
  retryLabel,
}: {
  disabled?: boolean;
  onBack?: () => void;
  onEdit?: () => void;
  onPick?: () => void;
  /** 換一份教材 */
  onChange?: () => void;
  onRetry?: () => void;
  retryLabel?: string;
}) {
  return (
    <div className="button-row material-fix">
      {onBack && (
        <button type="button" className="secondary-button small-button" disabled={disabled} onClick={onBack}>
          {BACK_STEP}
        </button>
      )}
      {onEdit && (
        <button type="button" className="secondary-button small-button" disabled={disabled} onClick={onEdit}>
          {EDIT_TEXT}
        </button>
      )}
      {onPick && (
        <button type="button" className="secondary-button small-button" disabled={disabled} onClick={onPick}>
          {PICK_PARAGRAPHS}
        </button>
      )}
      {onChange && (
        <button type="button" className="secondary-button small-button" disabled={disabled} onClick={onChange}>
          {CHANGE_MATERIAL}
        </button>
      )}
      {onRetry && (
        <button type="button" className="secondary-button small-button" disabled={disabled} onClick={onRetry}>
          {retryLabel}
        </button>
      )}
    </div>
  );
}

function useMaterialDetail(materialId: number) {
  const [detail, setDetail] = useState<MaterialDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .getLibraryMaterial(materialId)
      .then((d) => alive && setDetail(d))
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [materialId]);
  return { detail, error };
}

/** The preview of a paragraph in the picker. */
const preview = (text: string) => (text.length > 110 ? `${text.slice(0, 110)}…` : text);

/** The checkbox in front of a 題組 or of the cover section: indeterminate when only some are chosen. */
function PickerGroupBox({
  label,
  state,
  onToggle,
}: {
  label: string;
  state: "all" | "some" | "none";
  onToggle: (on: boolean) => void;
}) {
  return (
    <label className="paragraph-picker-group-label">
      <input
        type="checkbox"
        checked={state === "all"}
        ref={(el) => {
          if (el) el.indeterminate = state === "some";
        }}
        onChange={(e) => onToggle(e.target.checked)}
      />
      <span>{label}</span>
    </label>
  );
}

/**
 * 只選要用的段落: tick single paragraphs or a range. The choice belongs to this worksheet or
 * conversation (the caller stores it); the material itself is never changed.
 */
export function ParagraphPickerDialog({
  materialId,
  initial,
  onSave,
  onClose,
  returnFocus,
}: {
  materialId: number;
  /** The ids used now; `null`: all of the paragraphs (an exam paper's cover text excepted). */
  initial: readonly string[] | null;
  /** `null`: use all of them. */
  onSave: (ids: string[] | null) => Promise<void> | void;
  onClose: () => void;
  returnFocus?: () => HTMLElement | null;
}) {
  const ids = useId();
  const { detail, error } = useMaterialDetail(materialId);
  const [picked, setPicked] = useState<Set<string> | null>(null);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const paragraphs = useMemo(() => detail?.paragraphs ?? [], [detail]);
  const chosen = useMemo(() => picked ?? initialSelection(paragraphs, initial), [picked, initial, paragraphs]);
  const runs = useMemo(() => paragraphRuns(paragraphs), [paragraphs]);
  const lastClicked = useRef<string | null>(null);
  const dirty = picked !== null;

  function change(next: Set<string>) {
    setPicked(next);
  }

  function selectRange() {
    const a = Number(from);
    const b = Number(to);
    if (!Number.isInteger(a) || !Number.isInteger(b) || a < 1 || b < 1) return;
    const [lo, hi] = a <= b ? [a, b] : [b, a];
    change(new Set(paragraphs.filter((p) => p.no >= lo && p.no <= hi).map((p) => p.id)));
  }

  function clickParagraph(id: string, on: boolean, shift: boolean) {
    const last = lastClicked.current;
    lastClicked.current = id;
    if (shift && last !== null) change(toggleRange(paragraphs, chosen, last, id, on));
    else change(toggleMany(chosen, [id], on));
  }

  function renderParagraph(p: (typeof paragraphs)[number]) {
    return (
      <li key={p.id}>
        <label>
          <input
            type="checkbox"
            checked={chosen.has(p.id)}
            onChange={(e) => clickParagraph(p.id, e.target.checked, (e.nativeEvent as MouseEvent).shiftKey === true)}
          />
          <span className="paragraph-picker-no">第 {p.no} 段</span>
          <span lang="en">{preview(p.text)}</span>
        </label>
      </li>
    );
  }

  async function save() {
    if (chosen.size === 0 || saving) return;
    setSaving(true);
    setSaveError(null);
    try {
      await onSave(selectionToSave(paragraphs, chosen));
      onClose();
    } catch (err) {
      setSaveError(errorMessage(err));
      setSaving(false);
    }
  }

  return (
    <Dialog title={PICK_PARAGRAPHS} size="large" returnFocus={returnFocus} onClose={onClose} busy={saving} dirty={dirty}>
      {error && <ErrorText message={error} />}
      {!detail && !error && <Loading />}
      {detail && (
        <div className="paragraph-picker">
          <p className="field-hint">
            勾選這一次要用的段落。課文本身不會改變，也可以隨時改回全部使用。按住 Shift 再點，可以一次選取連續的段落。
          </p>
          <div className="paragraph-picker-tools button-row">
            <label htmlFor={`${ids}-from`}>從第</label>
            <input
              id={`${ids}-from`}
              type="text"
              inputMode="numeric"
              size={4}
              value={from}
              onChange={(e) => setFrom(e.target.value.replace(/\D/g, "").slice(0, 4))}
            />
            <label htmlFor={`${ids}-to`}>段到第</label>
            <input
              id={`${ids}-to`}
              type="text"
              inputMode="numeric"
              size={4}
              value={to}
              onChange={(e) => setTo(e.target.value.replace(/\D/g, "").slice(0, 4))}
            />
            <span>段</span>
            <button type="button" className="secondary-button small-button" onClick={selectRange}>
              選這一段範圍
            </button>
            <button
              type="button"
              className="text-link is-plain"
              onClick={() => change(new Set(paragraphs.map((p) => p.id)))}
            >
              全選
            </button>
            <button type="button" className="text-link is-plain" onClick={() => change(new Set())}>
              全部取消
            </button>
          </div>
          <ol className="paragraph-picker-list" aria-label="課文段落">
            {runs.map((run) => {
              if (run.kind === "plain") return renderParagraph(run.items[0]);
              const runIds = run.items.map((p) => p.id);
              const state = tickState(runIds, chosen);
              const head = (
                <PickerGroupBox
                  label={run.kind === "group" ? groupLabel(run.group, run.items.length) : frontLabel(run.items.length)}
                  state={state}
                  onToggle={(on) => change(toggleMany(chosen, runIds, on))}
                />
              );
              const list = <ol className="paragraph-picker-list">{run.items.map(renderParagraph)}</ol>;
              return run.kind === "group" ? (
                <li key={run.key} className="paragraph-picker-group">
                  <div className="paragraph-picker-group-head">{head}</div>
                  {list}
                </li>
              ) : (
                <li key={run.key} className="paragraph-picker-group">
                  <details className="paragraph-picker-front">
                    <summary>{head}</summary>
                    {list}
                  </details>
                </li>
              );
            })}
          </ol>
          <p className="field-hint" role="status">
            已選 {chosen.size} 段（共 {paragraphs.length} 段）
          </p>
          <ErrorText message={saveError} />
          <div className="button-row">
            <button type="button" className="primary-button" disabled={chosen.size === 0 || saving} onClick={() => void save()}>
              {saving ? "儲存中…" : `使用選的 ${chosen.size} 段`}
            </button>
            <DialogCancel disabled={saving} />
          </div>
        </div>
      )}
    </Dialog>
  );
}

/**
 * 修改課文: the pasted text of a library material, edited in place (`PUT /api/materials/:id`).
 * An uploaded file's text cannot be edited; the dialog then says so and offers the way out the
 * caller gives (換一份教材 or 回上一步).
 */
export function MaterialTextDialog({
  materialId,
  from,
  onSaved,
  onClose,
  otherWay,
  returnFocus,
}: {
  materialId: number;
  from: "home" | "prep_session" | "worksheet";
  onSaved: (detail: MaterialDetail) => Promise<void> | void;
  onClose: () => void;
  /** For a text that cannot be edited: where to go instead (換一份教材, 回上一步). */
  otherWay?: { label: string; onClick: () => void };
  returnFocus?: () => HTMLElement | null;
}) {
  const ids = useId();
  const { detail, error } = useMaterialDetail(materialId);
  const [title, setTitle] = useState<string | null>(null);
  const [text, setText] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const editable = detail?.origin === "paste";
  const shownTitle = title ?? detail?.title ?? "";
  const shownText = text ?? detail?.text ?? "";
  const dirty = title !== null || text !== null;

  async function save() {
    if (!detail || shownText.trim() === "" || saving) return;
    setSaving(true);
    setSaveError(null);
    try {
      const saved = await api.updateLibraryMaterial(detail.id, {
        title: shownTitle.trim(),
        text: shownText,
        edit_from: from,
      });
      await onSaved(saved);
      onClose();
    } catch (err) {
      setSaveError(errorMessage(err));
      setSaving(false);
    }
  }

  return (
    <Dialog title={EDIT_TEXT} returnFocus={returnFocus} onClose={onClose} busy={saving} dirty={dirty}>
      {error && <ErrorText message={error} />}
      {!detail && !error && <Loading />}
      {detail && !editable && (
        <div className="attach-step">
          <p>這是上傳的檔案，讀出來的文字不能直接修改。你可以只選要用的段落，或換一份教材。</p>
          <div className="button-row">
            {otherWay && (
              <button
                type="button"
                className="primary-button"
                onClick={() => {
                  onClose();
                  otherWay.onClick();
                }}
              >
                {otherWay.label}
              </button>
            )}
            <DialogCancel label="關閉" />
          </div>
        </div>
      )}
      {detail && editable && (
        <form
          className="attach-paste"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <label className="field-label" htmlFor={`${ids}-title`}>
            教材標題 <span className="optional-label">可以不填</span>
          </label>
          <input
            id={`${ids}-title`}
            type="text"
            maxLength={200}
            value={shownTitle}
            onChange={(e) => setTitle(e.target.value)}
          />
          <label className="field-label" htmlFor={`${ids}-text`}>
            課文
          </label>
          <textarea
            id={`${ids}-text`}
            className="print-input"
            rows={14}
            lang="en"
            maxLength={50000}
            value={shownText}
            aria-describedby={`${ids}-hint`}
            onChange={(e) => setText(e.target.value)}
          />
          <p className="field-hint" id={`${ids}-hint`}>
            段落之間空一行。沒有空行時，Manna 會把一行一行折斷的文字（從 PDF 複製的）接回段落。沒有改的段落會保留原來的註記。
          </p>
          <ErrorText message={saveError} />
          <div className="button-row">
            <button type="submit" className="primary-button" disabled={saving || shownText.trim() === ""}>
              {saving ? "儲存中…" : "儲存課文"}
            </button>
            <DialogCancel disabled={saving} />
          </div>
        </form>
      )}
    </Dialog>
  );
}

/** A line saying how much of a material is used (「只用其中 12 段（共 52 段）」) with its change link. */
export function SelectionLine({
  children,
  onChange,
  disabled,
}: {
  children: ReactNode;
  onChange: () => void;
  disabled?: boolean;
}) {
  return (
    <p className="quiet-note selection-line" role="status">
      {children}{" "}
      <button type="button" className="text-link is-plain" disabled={disabled} onClick={onChange}>
        改選段落
      </button>
    </p>
  );
}
