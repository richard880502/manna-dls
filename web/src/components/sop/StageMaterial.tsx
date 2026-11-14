import { useEffect, useRef, useState } from "react";
import type { SopMaterial } from "../../api/types";
import { PrivacyHint } from "../common";
import type { SopController } from "./useSop";

/** The material as a printed sheet: serif, paragraph numbers in the margin. Never editable here. */
export function MaterialSheet({ material }: { material: SopMaterial }) {
  return (
    <div className="material-sheet" lang="en">
      {material.title && <p className="material-title">{material.title}</p>}
      {material.paragraphs.map((p) => (
        <p key={p.id} data-n={p.no}>
          <span className="sr-only">第 {p.no} 段：</span>
          {p.text}
        </p>
      ))}
      {material.source_note && (
        <p className="material-source" lang="zh-Hant">
          來源：{material.source_note}
        </p>
      )}
    </div>
  );
}

/**
 * Step 1, 閱讀教材. With no material the step is a place to paste it; once there is one, the
 * step shows it the way it will be read, split into numbered paragraphs, and 修改教材 brings the
 * fields back. The server does the splitting and keeps a paragraph's id while its text stays.
 */
export function StageMaterial({ sop }: { sop: SopController }) {
  const { material, draft, setDraft } = sop;
  const [editing, setEditing] = useState(material === null);
  const textRef = useRef<HTMLTextAreaElement>(null);
  const openedEditor = useRef(false);

  // After 修改教材 the cursor goes to the text.
  useEffect(() => {
    if (editing && openedEditor.current) textRef.current?.focus();
  }, [editing]);

  const paragraphCount = material?.paragraphs.length ?? 0;

  return (
    <>
      <p className="lead">把這節課要讀的文章貼上來。之後的每一步都會一段一段對照它。</p>

      {editing || material === null ? (
        <section className="stage-block" aria-label="教材">
          <label className="field-label" htmlFor="sop-material-title">
            教材標題
          </label>
          <input
            id="sop-material-title"
            type="text"
            value={draft.title}
            onChange={(e) => setDraft({ title: e.target.value })}
            placeholder="例如：L3 The Umbrella Stand"
          />
          <label className="field-label" htmlFor="sop-material-text">
            教材文字
          </label>
          <textarea
            id="sop-material-text"
            ref={textRef}
            className="material-input"
            rows={14}
            lang="en"
            value={draft.text}
            aria-describedby="sop-material-hint"
            onChange={(e) => setDraft({ text: e.target.value })}
            placeholder="貼上課文或文章。段落之間空一行。"
          />
          <p className="field-hint" id="sop-material-hint">
            段落之間空一行，Manna 會照空行分段；沒有空行時每一行算一段。
          </p>
          <PrivacyHint />
          <label className="field-label" htmlFor="sop-material-source">
            來源說明
          </label>
          <input
            id="sop-material-source"
            type="text"
            value={draft.source_note}
            onChange={(e) => setDraft({ source_note: e.target.value })}
            placeholder="例如：龍騰版第二冊第三課；或文章的出處"
          />
          {material !== null && (
            <p className="stage-inline-actions">
              <button
                type="button"
                className="text-link"
                onClick={() => {
                  void sop.flush();
                  setEditing(false);
                }}
              >
                改好了，看分段結果
              </button>
            </p>
          )}
        </section>
      ) : (
        <section className="stage-block" aria-label="教材">
          <div className="material-head">
            <p className="quiet-note">
              共 {paragraphCount} 段、{material.word_count} 字。段落編號在左邊。
            </p>
            <button
              type="button"
              className="text-link is-plain"
              onClick={() => {
                openedEditor.current = true;
                setEditing(true);
              }}
            >
              修改教材
            </button>
          </div>
          <MaterialSheet material={material} />
        </section>
      )}
      {material !== null && editing && paragraphCount > 0 && (
        <p className="quiet-note" role="status">
          目前分成 {paragraphCount} 段。
        </p>
      )}
    </>
  );
}

interface MaterialViewerProps {
  material: SopMaterial | null;
  open: boolean;
  onClose: () => void;
  onGoToMaterial: () => void;
}

/**
 * 「對照教材」: the material slides in from the left and stays beside the step (it covers the
 * progress rail; on a phone it covers the page). The same sheet as in step 1, read-only.
 */
export function MaterialViewer({ material, open, onClose, onGoToMaterial }: MaterialViewerProps) {
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    closeRef.current?.focus();
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  if (!open) return null;
  return (
    <aside id="material-viewer" className="material-viewer" aria-label="教材">
      <div className="material-viewer-head">
        <h2>教材</h2>
        <button type="button" className="text-link is-plain" ref={closeRef} onClick={onClose}>
          收起教材
        </button>
      </div>
      {material ? (
        <MaterialSheet material={material} />
      ) : (
        <p className="quiet-note">
          還沒有教材。
          <button type="button" className="text-link is-plain" onClick={onGoToMaterial}>
            到第 1 步貼上教材
          </button>
        </p>
      )}
    </aside>
  );
}
