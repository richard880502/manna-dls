import { useId, useState, type ReactNode } from "react";
import type { SaveWorksheetMaterialRequest, WorksheetMaterial } from "../../api/types";
import { EDIT_TEXT, PICK_PARAGRAPHS, selectedLine } from "../../lib/longMaterial";
import { SelectionLine } from "../material/LongMaterial";
import { MaterialViewer } from "../material/MaterialViewer";

/** What the page hands to whatever fills the material slot. */
export interface MaterialPaneProps {
  material: WorksheetMaterial;
  /** Stores pasted text as this worksheet's material. Resolves to false when it was not stored. */
  onReplace: (body: SaveWorksheetMaterialRequest) => Promise<boolean>;
  /** The worksheet is being generated: the material cannot change now. */
  locked: boolean;
  /** 修改課文: opens the text of the library material for editing. */
  onEditText?: () => void;
  /** 只選要用的段落: opens the paragraph picker. */
  onPickParagraphs?: () => void;
}

/** 修改課文, 只選要用的段落 and how much of the material is used, under the material. */
function MaterialTools({ material, locked, onEditText, onPickParagraphs }: MaterialPaneProps) {
  const selected = material.source.selected_ids;
  const total = material.source.total_paragraphs;
  return (
    <>
      {selected && typeof total === "number" && (
        <SelectionLine disabled={locked} onChange={() => onPickParagraphs?.()}>
          {selectedLine(selected.length, total)}
        </SelectionLine>
      )}
      <p className="quiet-note material-tools">
        {onEditText && (
          <button type="button" className="text-link is-plain" disabled={locked} onClick={onEditText}>
            {EDIT_TEXT}
          </button>
        )}
        {onPickParagraphs && !selected && (
          <button type="button" className="text-link is-plain" disabled={locked} onClick={onPickParagraphs}>
            {PICK_PARAGRAPHS}
          </button>
        )}
      </p>
    </>
  );
}

/**
 * The slot's type. The worksheet pages render the material column through one function of this
 * shape (default: [`WorksheetMaterialPane`]). The materials branch's `MaterialViewer` (library
 * material, annotations with the 提供給 AI flag) plugs in here: pass a function that renders it
 * from the same `WorksheetMaterial`; nothing else on the page knows what fills the column.
 */
export type MaterialPaneSlot = (props: MaterialPaneProps) => ReactNode;

function PasteForm({
  material,
  onReplace,
  onDone,
  locked,
}: MaterialPaneProps & { onDone?: () => void }) {
  const ids = useId();
  const [title, setTitle] = useState(material.title);
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);

  async function submit() {
    setSending(true);
    const ok = await onReplace({ title: title.trim(), text });
    setSending(false);
    if (ok) onDone?.();
  }

  return (
    <form
      className="ws-paste"
      onSubmit={(e) => {
        e.preventDefault();
        void submit();
      }}
    >
      <label className="field-label" htmlFor={`${ids}-title`}>
        教材標題
      </label>
      <input id={`${ids}-title`} type="text" value={title} onChange={(e) => setTitle(e.target.value)} />
      <label className="field-label" htmlFor={`${ids}-text`}>
        貼上教材文字
      </label>
      <textarea
        id={`${ids}-text`}
        className="print-input"
        rows={10}
        lang="en"
        value={text}
        onChange={(e) => setText(e.target.value)}
      />
      <div className="actions">
        <button type="submit" className="secondary-button" disabled={sending || locked || text.trim() === ""}>
          {sending ? "儲存中…" : "使用這份教材"}
        </button>
        {onDone && (
          <button type="button" className="text-link is-plain" onClick={onDone}>
            取消
          </button>
        )}
      </div>
    </form>
  );
}

/**
 * The default material column: the text as print, read-only, with paragraph numbers in the
 * margin. It can be folded away, and pasted material can be replaced.
 */
export function WorksheetMaterialPane(props: MaterialPaneProps) {
  const { material, locked } = props;
  const [open, setOpen] = useState(true);
  const [replacing, setReplacing] = useState(false);
  const empty = material.paragraphs.length === 0;
  const fromLesson = material.source.kind === "lesson_sop";
  const name = material.title.trim() || "教材";

  const libraryId = material.source.material_id ?? null;
  if (libraryId && !empty && !replacing) {
    // The library's viewer: the text with the teacher's annotations; notes marked 「提供給 AI」
    // are what the generator passes to the model.
    return (
      <div className="ws-material ws-material-library">
        <MaterialViewer mode="pane" materialId={libraryId} />
        <p className="quiet-note">
          {fromLesson
            ? "從課例帶入的教材。"
            : material.source.kind === "library"
              ? "從你的教材庫取用的教材。"
              : "你貼上的教材，已放進你的教材庫。"}
          勾了「提供給 AI」的註記會在產生時參考；註記不會出現在學習單上。
        </p>
        <MaterialTools {...props} />
        <p className="quiet-note">
          <button type="button" className="text-link is-plain" disabled={locked} onClick={() => setReplacing(true)}>
            換教材
          </button>
        </p>
      </div>
    );
  }

  return (
    <section className="ws-material" aria-label="教材">
      <div className="ws-material-head">
        <h2>教材{material.title.trim() ? `：${material.title.trim()}` : ""}</h2>
        {!empty && (
          <button type="button" className="text-link is-plain" aria-expanded={open} onClick={() => setOpen(!open)}>
            {open ? "收起" : "展開"}
          </button>
        )}
      </div>

      {empty && !replacing && (
        <>
          <p className="quiet-note">還沒有教材。貼上這份學習單要用的文字，Manna 才有東西可以讀。</p>
          <PasteForm {...props} />
        </>
      )}

      {!empty && open && !replacing && (
        <>
          <article className="material-sheet" lang="en" aria-label={`${name}原文`}>
            {material.title.trim() && <p className="material-title">{material.title.trim()}</p>}
            {material.paragraphs.map((p, i) => (
              <p key={p.id} data-n={i + 1}>
                {p.text}
              </p>
            ))}
          </article>
          <p className="quiet-note">
            {fromLesson ? "從課例帶入的教材（帶入當時的內容）。" : "你貼上的教材。"}
            {material.source.note ? `來源：${material.source.note}。` : ""}
            教材不會出現在學習單上，也不會被改動。
          </p>
          <p className="quiet-note">
            <button type="button" className="text-link is-plain" disabled={locked} onClick={() => setReplacing(true)}>
              換教材
            </button>
          </p>
        </>
      )}

      {replacing && (
        <>
          <p className="quiet-note">貼上新的教材會取代目前這一份；已經產生的學習單不會自動改變。</p>
          <PasteForm {...props} onDone={() => setReplacing(false)} />
        </>
      )}
    </section>
  );
}
