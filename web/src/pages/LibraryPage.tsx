import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from "react";
import { Link, useLocation, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { CreateUnitRequest, SchoolClass, Unit } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { Dialog, useDialogGuard } from "../components/Dialog";
import { MoreMenu } from "../components/library/Cards";
import { DeleteUnitDialog } from "../components/unit/DeleteDialogs";
import { RenameDialog } from "../components/unit/RenameDialog";
import { useApp } from "../context/AppContext";
import { usedByText } from "../lib/classes";
import { UNIT_IDEAS_STATUS } from "../lib/labels";
import { useDocumentTitle } from "../lib/useDocumentTitle";
import { MetaList } from "../components/MetaList";

/** 加到班級: which classes use one unit. Each tick is saved at once. */
function AssignClasses({
  unit,
  classes,
  onChanged,
}: {
  unit: Unit;
  classes: SchoolClass[];
  onChanged: (unitId: number, classId: number, assigned: boolean) => void;
}) {
  const [busy, setBusy] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const used = new Set(unit.classes.map((c) => c.id));

  async function toggle(c: SchoolClass, assign: boolean) {
    setBusy(c.id);
    setError(null);
    try {
      if (assign) await api.assignUnit(c.id, unit.id);
      else await api.unassignUnit(c.id, unit.id);
      onChanged(unit.id, c.id, assign);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  }

  return (
    <fieldset className="row-more assign-classes">
      <legend>「{unit.title}」給哪些班級用？</legend>
      {classes.length === 0 ? (
        <p className="field-hint">
          還沒有班級。<Link to="/classes">到班級頁新增班級</Link>
        </p>
      ) : (
        <div className="assign-classes-list">
          {classes.map((c) => (
            <label key={c.id} className="check-row">
              <input
                type="checkbox"
                checked={used.has(c.id)}
                disabled={busy !== null}
                onChange={(e) => void toggle(c, e.target.checked)}
              />
              {c.name}
              {busy === c.id ? "（儲存中…）" : ""}
            </label>
          ))}
        </div>
      )}
      <p className="field-hint">取消勾選只是讓那個班級不再取用；單元與那班上過的紀錄都保留。</p>
      <ErrorText message={error} />
    </fieldset>
  );
}

/** 新增單元: the name and, when there are classes, which one uses it. Lives in a dialog. */
function NewUnitForm({
  classes,
  onCreated,
  onCancel,
}: {
  classes: SchoolClass[];
  onCreated: (unit: Unit) => void;
  onCancel: () => void;
}) {
  const ids = useId();
  const [title, setTitle] = useState("");
  /** "" = 先放在備課庫，不加入班級. */
  const [classId, setClassId] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const requestClose = useDialogGuard({ busy: creating, dirty: title.trim() !== "" || classId !== "" });

  async function onCreate(event: FormEvent) {
    event.preventDefault();
    const trimmed = title.trim();
    if (!trimmed) {
      setCreateError("請輸入單元名稱。");
      return;
    }
    const body: CreateUnitRequest = { title: trimmed };
    if (classId) body.class_id = Number(classId);
    setCreating(true);
    setCreateError(null);
    try {
      onCreated(await api.createUnit(body));
    } catch (err) {
      setCreateError(errorMessage(err));
      setCreating(false);
    }
  }

  return (
    <form className="create-unit-form" onSubmit={onCreate} aria-label="新增單元">
      <label className="field-label" htmlFor={`${ids}-title`}>
        單元名稱
      </label>
      <input
        id={`${ids}-title`}
        placeholder="例如：龍騰 B1 L3 Not only… but also 倒裝句"
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        disabled={creating}
      />
      {classes.length > 0 && (
        <div className="create-unit-class">
          <label className="field-label" htmlFor={`${ids}-class`}>
            給哪個班級用？
          </label>
          <select id={`${ids}-class`} value={classId} onChange={(e) => setClassId(e.target.value)} disabled={creating}>
            <option value="">先放在備課庫</option>
            {classes.map((c) => (
              <option key={c.id} value={String(c.id)}>
                {c.name}
              </option>
            ))}
          </select>
        </div>
      )}
      <ErrorText message={createError} />
      <div className="button-row">
        <button className="primary-button" type="submit" disabled={creating}>
          {creating ? "建立中…" : "建立單元"}
        </button>
        <button type="button" className="text-link is-plain" disabled={creating} onClick={requestClose ?? onCancel}>
          取消
        </button>
      </div>
    </form>
  );
}

/** Where the unit page sends the teacher after deleting its unit: said once here. */
export interface LibraryLocationState {
  deletedUnit?: string;
}

/**
 * 備課庫 (`/units`): every unit of the teacher as ruled rows (lessons, 單元想法, which classes use
 * it), with 加到班級 to reuse a unit in another class, and 改名 / 刪除 behind each row's ⋯
 * (刪除 says what goes with the unit first). It is not in the top bar: it is reached from 班級
 * (the home page and the class page's 「從備課庫加入單元」). Materials have their own page (教材).
 * Coming here leaves the class context: a unit opened from the library has no class in play
 * unless only one class uses it.
 */
export function LibraryPage() {
  useDocumentTitle("備課庫");
  const { setCurrentClassId } = useApp();
  const navigate = useNavigate();
  const location = useLocation();
  const [units, setUnits] = useState<Unit[] | null>(null);
  const [classes, setClasses] = useState<SchoolClass[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  /** 新增單元 opens a dialog; focus goes back to the button when it closes. */
  const opener = useRef<HTMLElement | null>(null);
  const backToOpener = useCallback(() => opener.current, []);
  /** The unit whose 加到班級 choices are open. */
  const [assigning, setAssigning] = useState<number | null>(null);
  const [renaming, setRenaming] = useState<Unit | null>(null);
  const [deleting, setDeleting] = useState<Unit | null>(null);
  const [notice, setNotice] = useState<string | null>(() => {
    const deleted = (location.state as LibraryLocationState | null)?.deletedUnit;
    return deleted ? `已刪除單元「${deleted}」。` : null;
  });
  /** A row's ⋯: focus goes back there, or to the heading once the row is gone. */
  const rowOpener = useRef<HTMLElement | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const backToRow = useCallback(
    () => (rowOpener.current?.isConnected ? rowOpener.current : heading.current),
    [],
  );

  useEffect(() => {
    setCurrentClassId(null);
  }, [setCurrentClassId]);

  // The 已刪除 line is said once: a reload or Back does not bring it again.
  useEffect(() => {
    if ((location.state as LibraryLocationState | null)?.deletedUnit) {
      navigate(location.pathname, { replace: true, state: null });
    }
  }, [location.state, location.pathname, navigate]);

  useEffect(() => {
    let alive = true;
    api
      .listUnits()
      .then((list) => {
        if (alive) setUnits(list);
      })
      .catch((err: unknown) => {
        if (alive) setLoadError(errorMessage(err));
      });
    api
      .listClasses()
      .then((list) => alive && setClasses(list))
      // Without the classes the library still lists and creates units.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  function onAssignChanged(unitId: number, changedClassId: number, assigned: boolean) {
    const c = classes.find((x) => x.id === changedClassId);
    setUnits((prev) =>
      (prev ?? []).map((u) => {
        if (u.id !== unitId) return u;
        const others = (u.classes ?? []).filter((x) => x.id !== changedClassId);
        if (!assigned || !c) return { ...u, classes: others };
        const added = { id: c.id, name: c.name, course_label: c.course_label, class_profile: c.class_profile, note: "" };
        // Keep the teacher's class order.
        const order = classes.map((x) => x.id);
        return { ...u, classes: [...others, added].sort((a, b) => order.indexOf(a.id) - order.indexOf(b.id)) };
      }),
    );
  }

  return (
    <section aria-labelledby="library-heading">
      <p className="crumbs">
        <Link to="/classes">班級</Link> / 備課庫
      </p>
      <div className="head-row page-head">
        <div>
          <h1 id="library-heading" ref={heading} tabIndex={-1}>
            備課庫
          </h1>
          <p className="lead">你準備好的單元。一份設計可以給好幾個班用，在哪一班改都是同一份。</p>
        </div>
        <button
          type="button"
          className="primary-button"
          aria-haspopup="dialog"
          onClick={(event) => {
            opener.current = event.currentTarget;
            setAdding(true);
          }}
        >
          新增單元
        </button>
      </div>

      {adding && (
        <Dialog title="新增單元" returnFocus={backToOpener} onClose={() => setAdding(false)}>
          <NewUnitForm
            classes={classes}
            onCancel={() => setAdding(false)}
            onCreated={(unit) => {
              setUnits((prev) => [unit, ...(prev ?? [])]);
              setAdding(false);
              navigate(`/units/${unit.id}`);
            }}
          />
        </Dialog>
      )}

      {notice && (
        <p className="quiet-note" role="status">
          {notice}
        </p>
      )}
      {renaming && (
        <RenameDialog
          title="改單元名稱"
          label="單元名稱"
          initial={renaming.title}
          emptyMessage="請輸入單元名稱。"
          returnFocus={backToRow}
          onClose={() => setRenaming(null)}
          onSave={async (title) => {
            const saved = await api.updateUnit(renaming.id, { title });
            setUnits((prev) => (prev ?? []).map((u) => (u.id === saved.id ? { ...u, title: saved.title } : u)));
            setRenaming(null);
            setNotice(null);
          }}
        />
      )}
      {deleting && (
        <DeleteUnitDialog
          unit={deleting}
          returnFocus={backToRow}
          onClose={() => setDeleting(null)}
          onDeleted={() => {
            setUnits((prev) => (prev ?? []).filter((u) => u.id !== deleting.id));
            if (assigning === deleting.id) setAssigning(null);
            setDeleting(null);
            setNotice(`已刪除單元「${deleting.title}」。`);
          }}
        />
      )}

      {loadError && <ErrorText message={loadError} />}
      {!units && !loadError && <Loading />}
      {units && units.length === 0 && <p className="empty-line">備課庫還沒有單元。建立第一個單元就能開始備課。</p>}
      {units && units.length > 0 && <h2 className="sr-only">單元</h2>}
      {units && units.length > 0 && (
        <ul className="rows is-first" aria-label="單元">
          {units.map((unit) => {
            const used = unit.classes ?? [];
            return (
              <li key={unit.id} className="row">
                <div>
                  <h3>
                    <Link to={`/units/${unit.id}`}>{unit.title}</Link>
                  </h3>
                  <MetaList
                    className="meta"
                    items={[
                      unit.lesson_count === 0 ? "還沒有課例" : `${unit.lesson_count} 節課例`,
                      UNIT_IDEAS_STATUS[unit.core_status],
                    ]}
                  />
                </div>
                <div className="side">
                  <span>{usedByText(used)}</span>
                  <button
                    type="button"
                    className="text-link"
                    aria-expanded={assigning === unit.id}
                    aria-label={`把「${unit.title}」加到班級`}
                    onClick={() => setAssigning((cur) => (cur === unit.id ? null : unit.id))}
                  >
                    加到班級
                  </button>
                  <MoreMenu
                    label={`更多動作「${unit.title}」`}
                    items={[
                      {
                        label: "改名",
                        onSelect: (from) => {
                          rowOpener.current = from;
                          setRenaming(unit);
                        },
                      },
                      {
                        label: "刪除",
                        onSelect: (from) => {
                          rowOpener.current = from;
                          setDeleting({ ...unit, classes: used });
                        },
                      },
                    ]}
                  />
                </div>
                {assigning === unit.id && (
                  <AssignClasses unit={{ ...unit, classes: used }} classes={classes} onChanged={onAssignChanged} />
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
