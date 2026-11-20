import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { api, errorMessage } from "../../api/client";
import type { SaveSopMaterialRequest, Unit } from "../../api/types";
import { ErrorText } from "../common";
import { Dialog, useDialogGuard } from "../Dialog";
import { materialFields, type MaterialChoice } from "../material/MaterialChooser";

export const STEPWISE_DIALOG_TITLE = "逐步設計：放進哪個單元？";

/** The article's title as the teacher sees it (a pasted text may have none). */
function articleTitleOf(choice: MaterialChoice | null): string {
  if (!choice) return "";
  return (choice.way === "paste" ? choice.title : (choice.picked?.title ?? "")).trim();
}

/** The body 閱讀教材 takes (`PUT /lessons/:id/sop/material`): a library material, or pasted text. */
function sopMaterialOf(choice: MaterialChoice | null): SaveSopMaterialRequest | null {
  const fields = choice ? materialFields(choice) : null;
  if (!fields) return null;
  return "material_id" in fields ? { material_id: fields.material_id } : { title: fields.material.title, text: fields.material.text };
}

type Mode = "existing" | "new";

function StepwiseForm({
  choice,
  typed,
  onCancel,
}: {
  choice: MaterialChoice | null;
  /** What the teacher typed in the home box: it only names the lesson. */
  typed: string;
  onCancel: () => void;
}) {
  const ids = useId();
  const navigate = useNavigate();
  const [units, setUnits] = useState<Unit[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [mode, setMode] = useState<Mode | null>(null);
  const [unitId, setUnitId] = useState<number | null>(null);
  /** null: the teacher has not typed here, so the default shows. */
  const [unitTitle, setUnitTitle] = useState<string | null>(null);
  const [lessonTitle, setLessonTitle] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** What is already made when a later step fails, so 再試一次 does not make a second one. */
  const made = useRef<{ unitId?: number; lessonId?: number }>({});
  const dirty = mode !== null || unitId !== null || unitTitle !== null || lessonTitle !== null;
  const requestClose = useDialogGuard({ busy, dirty });

  useEffect(() => {
    let alive = true;
    api
      .listUnits()
      .then((list) => alive && setUnits(list))
      // Without the list the teacher can still make a new unit.
      .catch((err: unknown) => {
        if (!alive) return;
        setUnits([]);
        setLoadError(errorMessage(err));
      });
    return () => {
      alive = false;
    };
  }, []);

  const article = articleTitleOf(choice);
  const typedName = typed.split("\n")[0]?.trim().slice(0, 60) ?? "";
  const effectiveMode: Mode = mode ?? (units && units.length > 0 ? "existing" : "new");
  // The newest unit first (the list is newest first).
  const chosenUnit = units?.find((u) => u.id === unitId) ?? units?.[0] ?? null;
  const defaultUnitTitle = article || typedName;
  const defaultLessonTitle = typedName || article || `第 ${effectiveMode === "existing" ? (chosenUnit?.lesson_count ?? 0) + 1 : 1} 課`;
  const shownUnitTitle = unitTitle ?? defaultUnitTitle;
  const shownLessonTitle = lessonTitle ?? defaultLessonTitle;

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    if (busy || units === null) return;
    if (effectiveMode === "new" && !shownUnitTitle.trim()) {
      setError("請輸入單元名稱。");
      return;
    }
    if (effectiveMode === "existing" && !chosenUnit) {
      setError("請選一個單元。");
      return;
    }
    if (!shownLessonTitle.trim()) {
      setError("請輸入課例名稱。");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (made.current.lessonId === undefined) {
        if (made.current.unitId === undefined) {
          made.current.unitId =
            effectiveMode === "new" ? (await api.createUnit({ title: shownUnitTitle.trim() })).id : chosenUnit!.id;
        }
        made.current.lessonId = (
          await api.createLesson(made.current.unitId, { title: shownLessonTitle.trim(), entry: "home" })
        ).id;
      }
      // The article goes in as step 1's material, the same way 閱讀教材 takes it.
      const material = sopMaterialOf(choice);
      if (material) await api.saveSopMaterial(made.current.lessonId, material);
      navigate(`/lessons/${made.current.lessonId}`);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <form className="create-lesson-form" onSubmit={onSubmit} aria-label="逐步設計">
      <p className="quiet-note">
        {choice ? "附上的文章會放進第一步「閱讀教材」。" : "進去後，第一步「閱讀教材」可以貼上、從教材庫選或上傳文章。"}
      </p>
      {units === null && <p className="quiet-note">載入單元中…</p>}
      {units !== null && (
        <>
          {loadError && <ErrorText message={loadError} />}
          <div className="radio-rows" role="radiogroup" aria-label="單元">
            {units.length > 0 && (
              <label>
                <input
                  type="radio"
                  name={`${ids}-mode`}
                  checked={effectiveMode === "existing"}
                  disabled={busy}
                  onChange={() => setMode("existing")}
                />
                <span>放進現有的單元</span>
              </label>
            )}
            <label>
              <input
                type="radio"
                name={`${ids}-mode`}
                checked={effectiveMode === "new"}
                disabled={busy}
                onChange={() => setMode("new")}
              />
              <span>建立新單元</span>
            </label>
          </div>
          {effectiveMode === "existing" && chosenUnit && (
            <div>
              <label className="field-label" htmlFor={`${ids}-unit`}>
                單元
              </label>
              <select
                id={`${ids}-unit`}
                value={chosenUnit.id}
                disabled={busy}
                onChange={(e) => setUnitId(Number(e.target.value))}
              >
                {units.map((u) => (
                  <option key={u.id} value={u.id}>
                    {u.title}
                  </option>
                ))}
              </select>
            </div>
          )}
          {effectiveMode === "new" && (
            <div>
              <label className="field-label" htmlFor={`${ids}-unit-title`}>
                新單元的名稱
              </label>
              <input
                id={`${ids}-unit-title`}
                value={shownUnitTitle}
                placeholder="例如：L3 The Umbrella Stand"
                disabled={busy}
                onChange={(e) => setUnitTitle(e.target.value)}
              />
            </div>
          )}
          <div>
            <label className="field-label" htmlFor={`${ids}-lesson`}>
              課例名稱
            </label>
            <input
              id={`${ids}-lesson`}
              value={shownLessonTitle}
              disabled={busy}
              onChange={(e) => setLessonTitle(e.target.value)}
            />
          </div>
        </>
      )}
      <ErrorText message={error} />
      <div className="button-row">
        <button className="primary-button" type="submit" disabled={busy || units === null}>
          {busy ? "建立中…" : "開始逐步設計"}
        </button>
        <button type="button" className="text-link is-plain" disabled={busy} onClick={requestClose ?? onCancel}>
          取消
        </button>
      </div>
    </form>
  );
}

/**
 * 逐步設計 from the home page: pick (or make) the unit, name the lesson, and open the seven-step
 * workspace. The article attached on the home page becomes step 1's material.
 */
export function StepwiseDialog({
  choice,
  typed,
  returnFocus,
  onClose,
}: {
  choice: MaterialChoice | null;
  typed: string;
  returnFocus?: () => HTMLElement | null;
  onClose: () => void;
}) {
  return (
    <Dialog title={STEPWISE_DIALOG_TITLE} returnFocus={returnFocus} onClose={onClose}>
      <StepwiseForm choice={choice} typed={typed} onCancel={onClose} />
    </Dialog>
  );
}
