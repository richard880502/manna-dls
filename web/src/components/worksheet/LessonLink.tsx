import { useEffect, useId, useState } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../../api/client";
import type { Lesson, Unit } from "../../api/types";
import { ErrorText } from "../common";
import type { WorksheetController } from "./useWorksheet";

type Mode = "link" | "new" | null;

/**
 * A worksheet made directly stays on its own (spec §4.2). When it is done the teacher may link
 * it to a lesson they already have, or save it as a new lesson. Neither changes an existing lesson.
 */
export function LessonLinkControls({ ws }: { ws: WorksheetController }) {
  const ids = useId();
  const state = ws.state;
  const [mode, setMode] = useState<Mode>(null);
  const [units, setUnits] = useState<Unit[] | null>(null);
  const [unitId, setUnitId] = useState<number | null>(null);
  const [lessons, setLessons] = useState<Lesson[]>([]);
  const [lessonId, setLessonId] = useState<number | null>(null);
  const [title, setTitle] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (mode === null || units !== null) return;
    let alive = true;
    api
      .listUnits()
      .then((list) => {
        if (!alive) return;
        setUnits(list);
        setUnitId(list[0]?.id ?? null);
      })
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [mode, units]);

  useEffect(() => {
    if (mode !== "link" || unitId === null) return;
    let alive = true;
    api
      .listLessons(unitId)
      .then((list) => {
        if (!alive) return;
        setLessons(list);
        setLessonId(list[0]?.id ?? null);
      })
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [mode, unitId]);

  if (!state) return null;

  async function run(call: () => ReturnType<typeof api.linkWorksheet>) {
    setSending(true);
    setError(null);
    try {
      const failed = await ws.flush();
      if (failed) throw new Error(failed);
      ws.adopt(await call());
      setMode(null);
    } catch (err) {
      setError(err instanceof Error && !("status" in err) ? `沒有儲存成功：${err.message}` : errorMessage(err));
    } finally {
      setSending(false);
    }
  }

  if (state.lesson) {
    return (
      <section className="ws-link" aria-label="課例">
        <p className="quiet-note">
          這份學習單連結到課例「<Link to={`/lessons/${state.lesson.id}`}>{state.lesson.title}</Link>」（
          {state.lesson.unit_title}）。在這裡改不會動到課例。
          <button
            type="button"
            className="text-link is-plain"
            disabled={sending}
            onClick={() => void run(() => api.linkWorksheet(state.id, null))}
          >
            取消連結
          </button>
        </p>
        <ErrorText message={error} />
      </section>
    );
  }

  const unitSelect = (
    <label className="field-label" htmlFor={`${ids}-unit`}>
      單元
      <select
        id={`${ids}-unit`}
        value={unitId ?? ""}
        onChange={(e) => setUnitId(e.target.value === "" ? null : Number(e.target.value))}
      >
        {(units ?? []).map((u) => (
          <option key={u.id} value={u.id}>
            {u.title}
          </option>
        ))}
      </select>
    </label>
  );

  return (
    <section className="ws-link" aria-label="課例">
      <p className="quiet-note">
        這是一份獨立的學習單，沒有連結課例。
        <button type="button" className="text-link is-plain" onClick={() => setMode(mode === "link" ? null : "link")}>
          連結既有課例
        </button>
        <button type="button" className="text-link is-plain" onClick={() => setMode(mode === "new" ? null : "new")}>
          另存為新課例
        </button>
      </p>
      {mode !== null && units !== null && units.length === 0 && (
        <p className="quiet-note">
          備課庫裡還沒有單元。先到<Link to="/library">備課庫</Link>建立一個單元。
        </p>
      )}
      {mode === "link" && units !== null && units.length > 0 && (
        <form
          className="ws-link-form"
          onSubmit={(e) => {
            e.preventDefault();
            if (lessonId !== null) void run(() => api.linkWorksheet(state.id, lessonId));
          }}
        >
          {unitSelect}
          <label className="field-label" htmlFor={`${ids}-lesson`}>
            課例
            <select
              id={`${ids}-lesson`}
              value={lessonId ?? ""}
              onChange={(e) => setLessonId(e.target.value === "" ? null : Number(e.target.value))}
            >
              {lessons.map((l) => (
                <option key={l.id} value={l.id}>
                  {l.title}
                </option>
              ))}
            </select>
          </label>
          {lessons.length === 0 && <p className="quiet-note">這個單元還沒有課例。</p>}
          <button type="submit" className="secondary-button" disabled={sending || lessonId === null}>
            連結這個課例
          </button>
        </form>
      )}
      {mode === "new" && units !== null && units.length > 0 && (
        <form
          className="ws-link-form"
          onSubmit={(e) => {
            e.preventDefault();
            if (unitId !== null && title.trim()) {
              void run(() => api.saveWorksheetAsLesson(state.id, unitId, title.trim()));
            }
          }}
        >
          {unitSelect}
          <label className="field-label" htmlFor={`${ids}-title`}>
            新課例的名稱
            <input id={`${ids}-title`} type="text" value={title} onChange={(e) => setTitle(e.target.value)} />
          </label>
          <button type="submit" className="secondary-button" disabled={sending || unitId === null || !title.trim()}>
            建立課例並連結
          </button>
        </form>
      )}
      <ErrorText message={error} />
    </section>
  );
}
