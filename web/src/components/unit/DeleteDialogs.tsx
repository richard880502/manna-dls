import { useEffect, useState, type ReactNode } from "react";
import { api, errorMessage } from "../../api/client";
import type { Lesson, LessonDeletePreview, Unit, UnitDeletePreview } from "../../api/types";
import { ErrorText, Loading } from "../common";
import { Dialog } from "../Dialog";

/** Said under what goes: there is no undo for the teacher (the research record keeps the rows). */
export const DELETE_FINAL = "刪除後無法復原。";

/**
 * What deleting a unit takes with it, one item a line: its lessons, the worksheets linked to them,
 * 單元想法 when it was started, and the 上課紀錄 that stop showing. Empty: nothing under it.
 */
export function unitDeleteItems(unit: Pick<Unit, "core_status">, preview: UnitDeletePreview): string[] {
  const items: string[] = [];
  if (preview.lessons > 0) items.push(`${preview.lessons} 節課例`);
  if (preview.worksheets > 0) items.push(`${preview.worksheets} 份學習單`);
  if (unit.core_status !== "none") items.push("單元想法");
  if (preview.runs > 0) items.push(`${preview.runs} 次上課紀錄`);
  return items;
}

/** What deleting a lesson takes with it: its worksheets and the 上課紀錄 that stop showing. */
export function lessonDeleteItems(preview: LessonDeletePreview): string[] {
  const items: string[] = [];
  if (preview.worksheets > 0) items.push(`${preview.worksheets} 份學習單`);
  if (preview.runs > 0) items.push(`${preview.runs} 次上課紀錄`);
  return items;
}

/** 「高一 3 班、高一 7 班」 */
const classNames = (unit: Pick<Unit, "classes">) => unit.classes.map((c) => c.name).join("、");

/**
 * The one confirmation both deletes use (`role="alertdialog"`): what goes is read first and said
 * in full; the delete button waits for it (a failed read still allows it, with the general line).
 */
function ConfirmDelete<P>({
  title,
  confirmLabel,
  loadPreview,
  describe,
  fallback,
  remove,
  returnFocus,
  onClose,
}: {
  title: string;
  confirmLabel: string;
  loadPreview: () => Promise<P>;
  describe: (preview: P) => ReactNode;
  /** Said when the preview could not be read. */
  fallback: string;
  remove: () => Promise<void>;
  returnFocus: () => HTMLElement | null;
  onClose: () => void;
}) {
  const [preview, setPreview] = useState<P | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    loadPreview()
      .then((p) => alive && setPreview(p))
      .catch(() => alive && setLoadFailed(true));
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once per opened dialog (the loader is a new closure each render)
  }, []);

  async function onConfirm() {
    setBusy(true);
    setError(null);
    try {
      await remove();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  const ready = preview !== null || loadFailed;
  return (
    <Dialog title={title} role="alertdialog" returnFocus={returnFocus} onClose={onClose} busy={busy}>
      <div className="material-delete library-delete">
        {preview !== null ? describe(preview) : loadFailed ? <p>{fallback}</p> : <Loading />}
        <p>{DELETE_FINAL}</p>
        <div className="button-row">
          <button type="button" className="danger-button" disabled={!ready || busy} onClick={() => void onConfirm()}>
            {busy ? "刪除中…" : confirmLabel}
          </button>
          <button type="button" className="text-link is-plain" disabled={busy} onClick={onClose}>
            取消
          </button>
        </div>
        <ErrorText message={error} />
      </div>
    </Dialog>
  );
}

function GoesWith({ items, none }: { items: string[]; none: string }) {
  if (items.length === 0) return <p>{none}</p>;
  return (
    <>
      <p>會一起刪除：</p>
      <ul className="library-delete-list">
        {items.map((item) => (
          <li key={item}>{item}</li>
        ))}
      </ul>
    </>
  );
}

/**
 * 刪除單元 (備課庫, 單元頁): says how many lessons, worksheets and 上課紀錄 go with it, and which
 * classes lose it, before anything is sent. `onDeleted` runs once the server has hidden it.
 */
export function DeleteUnitDialog({
  unit,
  returnFocus,
  onDeleted,
  onClose,
}: {
  unit: Unit;
  returnFocus: () => HTMLElement | null;
  onDeleted: () => void;
  onClose: () => void;
}) {
  const classes = classNames(unit);
  return (
    <ConfirmDelete<UnitDeletePreview>
      title={`刪除單元「${unit.title}」？`}
      confirmLabel="刪除單元"
      loadPreview={() => api.unitDeletePreview(unit.id)}
      describe={(preview) => (
        <>
          <GoesWith items={unitDeleteItems(unit, preview)} none="這個單元還沒有課例、學習單或單元想法。" />
          {classes && <p>也會從{classes}移除。</p>}
        </>
      )}
      fallback={`單元底下的課例、連到這些課例的學習單和單元想法都會一起刪除${classes ? `，也會從${classes}移除` : ""}。`}
      remove={async () => {
        await api.deleteUnit(unit.id);
        onDeleted();
      }}
      returnFocus={returnFocus}
      onClose={onClose}
    />
  );
}

/** 刪除課例 (單元頁): says how many worksheets and 上課紀錄 go with it first. */
export function DeleteLessonDialog({
  lesson,
  returnFocus,
  onDeleted,
  onClose,
}: {
  lesson: Pick<Lesson, "id" | "title">;
  returnFocus: () => HTMLElement | null;
  onDeleted: () => void;
  onClose: () => void;
}) {
  return (
    <ConfirmDelete<LessonDeletePreview>
      title={`刪除課例「${lesson.title}」？`}
      confirmLabel="刪除課例"
      loadPreview={() => api.lessonDeletePreview(lesson.id)}
      describe={(preview) => (
        <GoesWith items={lessonDeleteItems(preview)} none="這節課例還沒有學習單或上課紀錄。" />
      )}
      fallback="連到這節課例的學習單會一起刪除。"
      remove={async () => {
        await api.deleteLesson(lesson.id);
        onDeleted();
      }}
      returnFocus={returnFocus}
      onClose={onClose}
    />
  );
}
