import { useEffect, useRef, useState } from "react";
import type { Material } from "../../api/types";
import { KIND_LABEL } from "../../lib/materials";
import { ErrorText, Loading, Notice, PrivacyHint } from "../common";
import { MaterialAddForm, MaterialPicker, useMaterialLibrary } from "../material/MaterialLibrary";
import { MaterialContent } from "../material/MaterialViewer";
import type { SopController } from "./useSop";

type Way = "paste" | "library" | "upload";

const WAYS: Array<{ key: Way; label: string }> = [
  { key: "paste", label: "貼上文字" },
  { key: "library", label: "從教材庫選" },
  { key: "upload", label: "上傳檔案" },
];

/** 從教材庫選 and 上傳檔案: both end with the lesson using a library material. */
function LibraryWays({ way, sop, lessonId, onDone }: { way: "library" | "upload"; sop: SopController; lessonId: number; onDone: () => void }) {
  const { library, error, reload } = useMaterialLibrary();
  const [busy, setBusy] = useState(false);
  const [pickError, setPickError] = useState<string | null>(null);

  async function pick(material: Pick<Material, "id">) {
    setBusy(true);
    setPickError(null);
    const failed = await sop.pickMaterial(material.id);
    setBusy(false);
    if (failed) setPickError(failed);
    else onDone();
  }

  if (error) {
    return (
      <div>
        <ErrorText message={error} />
        <button type="button" className="text-link is-plain" onClick={reload}>
          再試一次
        </button>
      </div>
    );
  }
  if (!library) return <Loading />;
  return (
    <>
      {way === "library" ? (
        <MaterialPicker library={library} currentId={sop.material?.id ?? null} busy={busy} onPick={(m) => void pick(m)} />
      ) : (
        <MaterialAddForm way="upload" library={library} lessonId={lessonId} onAdded={(m) => void pick(m)} />
      )}
      <ErrorText message={pickError} />
    </>
  );
}

/**
 * Step 1, 閱讀教材. With no material the step offers three ways to get one: paste it, pick one
 * from the teacher's library, or upload a file. Once there is one, the step shows it the way
 * it will be read (numbered paragraphs, the teacher's notes) and offers to edit a pasted text,
 * switch to another material, or take it off the lesson. The server does the splitting and
 * keeps a paragraph's id while its text stays.
 */
export function StageMaterial({ sop, lessonId }: { sop: SopController; lessonId: number }) {
  const { material, draft, setDraft } = sop;
  const [editing, setEditing] = useState(material === null);
  const [way, setWay] = useState<Way>("paste");
  /** 換一份教材: the ways are shown although there is a material. */
  const [switching, setSwitching] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const textRef = useRef<HTMLTextAreaElement>(null);
  const openedEditor = useRef(false);

  // After 修改教材 the cursor goes to the text.
  useEffect(() => {
    if (editing && openedEditor.current) textRef.current?.focus();
  }, [editing]);

  const paragraphCount = material?.paragraphs.length ?? 0;
  const pasted = material === null || (material.origin ?? "paste") === "paste";
  const choosing = material === null || switching;

  async function removeFromLesson() {
    setBusy(true);
    setActionError(null);
    const failed = await sop.pickMaterial(null);
    setBusy(false);
    if (failed) setActionError(failed);
    else {
      setEditing(true);
      setSwitching(false);
    }
  }

  const pasteFields = (
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
  );

  return (
    <>
      <p className="lead">把這節課要讀的文章放進來。之後的每一步都會一段一段對照它。</p>

      {material === null && sop.materialDeleted && (
        <Notice tone="warn">
          <strong>教材已刪除。</strong>
          這個課例原本的教材已經從教材庫永久刪除。你寫的分析、目標與任務都還在；放進新的教材後可以重新對照。
        </Notice>
      )}

      {choosing && (
        <div className="material-ways" role="group" aria-label="教材從哪裡來">
          {WAYS.map((w) => (
            <button
              key={w.key}
              type="button"
              className="way-button"
              aria-pressed={way === w.key}
              onClick={() => setWay(w.key)}
            >
              {w.label}
            </button>
          ))}
          {switching && (
            <button type="button" className="text-link is-plain" onClick={() => setSwitching(false)}>
              不換了
            </button>
          )}
        </div>
      )}

      {choosing && way !== "paste" && (
        <section className="stage-block" aria-label={way === "library" ? "從教材庫選" : "上傳檔案"}>
          <LibraryWays
            way={way}
            sop={sop}
            lessonId={lessonId}
            onDone={() => {
              setSwitching(false);
              setEditing(false);
              setWay("paste");
            }}
          />
        </section>
      )}

      {choosing && way === "paste" && switching && material !== null && !pasted && (
        <p className="quiet-note">
          貼上的文字會成為這個課例的新教材；「{material.title || "原本的教材"}」仍留在教材庫。
        </p>
      )}

      {way === "paste" && (material === null || switching || (editing && pasted)) ? (
        pasteFields
      ) : (
        material !== null &&
        !switching && (
          <section className="stage-block" aria-label="教材">
            <div className="material-head">
              <p className="quiet-note">
                {paragraphCount > 0
                  ? `共 ${paragraphCount} 段、${material.word_count} 字。段落編號在左邊。`
                  : `${KIND_LABEL[material.kind ?? "text"]}教材，沒有可以逐段對照的文字。`}
              </p>
              <div className="material-head-actions">
                {pasted && (
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
                )}
                <button
                  type="button"
                  className="text-link is-plain"
                  onClick={() => {
                    setWay("library");
                    setSwitching(true);
                  }}
                >
                  換一份教材
                </button>
                <button type="button" className="text-link is-plain" disabled={busy} onClick={() => void removeFromLesson()}>
                  從這個課例移除
                </button>
              </div>
            </div>
            <MaterialContent materialId={material.id} material={material} />
            <p className="field-hint">
              「從這個課例移除」只是不在這節課使用，教材仍留在教材庫；要永久刪除請到備課庫的「教材」。
            </p>
          </section>
        )
      )}
      <ErrorText message={actionError} />
      {material !== null && editing && pasted && !switching && paragraphCount > 0 && (
        <p className="quiet-note" role="status">
          目前分成 {paragraphCount} 段。
        </p>
      )}
    </>
  );
}
