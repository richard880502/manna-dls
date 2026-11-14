import { useEffect, useId, useState, type FormEvent } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { ClassDetail, ClassUnit, CreateClassRequest, SchoolClass, Unit } from "../api/types";
import { ClassForm } from "../components/classes/ClassForm";
import { ErrorText, Loading, Notice } from "../components/common";
import { LeaveConfirm, useLeaveGuard } from "../components/LeaveGuard";
import { useApp } from "../context/AppContext";
import { CLASS_NOTE_MAX_CHARS, classSubtitle, nextStep } from "../lib/classes";
import { CORE_STATUS_LABELS, formatDate } from "../lib/labels";
import { CLASS_PROFILE_MAX_CHARS } from "../lib/preferences";

/** One unit design as this class uses it: progress in this class, the per-class note, order. */
function ClassUnitRow({
  item,
  index,
  total,
  busy,
  onMove,
  onRemove,
  onNote,
}: {
  item: ClassUnit;
  index: number;
  total: number;
  busy: boolean;
  onMove: (from: number, to: number) => void;
  onRemove: (unit: Unit) => void;
  onNote: (unitId: number, note: string) => Promise<string | null>;
}) {
  const id = useId();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(item.note);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { unit } = item;

  async function saveNote(event: FormEvent) {
    event.preventDefault();
    setSaving(true);
    setError(null);
    const failed = await onNote(unit.id, draft.trim());
    setSaving(false);
    if (failed) setError(failed);
    else setEditing(false);
  }

  return (
    <li className="class-unit">
      <div className="class-unit-main">
        <strong>
          <Link to={`/units/${unit.id}`}>{unit.title}</Link>
        </strong>
        <span className="unit-meta">
          {item.lessons_total === 0
            ? "還沒有課例"
            : `這班已上 ${item.lessons_taught} / ${item.lessons_total} 個課例`}
          {item.last_run_at ? ` · 最近一次上課 ${formatDate(item.last_run_at)}` : ""}
        </span>
        <div className="unit-card-tags">
          <span className={`core-status core-status-${unit.core_status}`}>
            CoRe：{CORE_STATUS_LABELS[unit.core_status]}
          </span>
          {unit.classes.length > 1 && <span className="soft-tag">{unit.classes.length} 個班級共用</span>}
        </div>

        {editing ? (
          <form className="class-unit-note-form" onSubmit={saveNote}>
            <label className="field-label" htmlFor={`${id}-note`}>
              這班的備註
            </label>
            <div className="inline-form">
              <input
                id={`${id}-note`}
                value={draft}
                maxLength={CLASS_NOTE_MAX_CHARS}
                placeholder="例如：這班用基礎版"
                disabled={saving}
                onChange={(e) => setDraft(e.target.value)}
              />
              <button type="submit" className="secondary-button" disabled={saving}>
                {saving ? "儲存中…" : "儲存備註"}
              </button>
            </div>
            <ErrorText message={error} />
          </form>
        ) : (
          <p className="class-unit-note">
            {item.note ? (
              <>
                <span className="class-unit-note-label">這班的備註</span>
                {item.note}
              </>
            ) : null}
            <button
              type="button"
              className="ghost-button small-button"
              onClick={() => {
                setDraft(item.note);
                setEditing(true);
              }}
            >
              {item.note ? "修改備註" : "加上這班的備註"}
            </button>
          </p>
        )}
      </div>

      <div className="class-unit-actions">
        {item.next_lesson ? (
          <Link className="primary-button link-button" to={`/lessons/${item.next_lesson.id}`}>
            下一節：{item.next_lesson.title}
          </Link>
        ) : null}
        <Link className="secondary-button link-button" to={`/units/${unit.id}`}>
          打開單元
        </Link>
        <div className="class-unit-order">
          <button
            type="button"
            className="ghost-button small-button"
            aria-label={`把「${unit.title}」往前移`}
            disabled={busy || index === 0}
            onClick={() => onMove(index, index - 1)}
          >
            上移
          </button>
          <button
            type="button"
            className="ghost-button small-button"
            aria-label={`把「${unit.title}」往後移`}
            disabled={busy || index === total - 1}
            onClick={() => onMove(index, index + 1)}
          >
            下移
          </button>
          <button
            type="button"
            className="ghost-button small-button"
            aria-label={`把「${unit.title}」移出這個班級`}
            disabled={busy}
            onClick={() => onRemove(unit)}
          >
            移出班級
          </button>
        </div>
      </div>
    </li>
  );
}

/**
 * 班級頁: the class profile, the unit designs this class takes from the library (in order, with
 * this class's progress and note), adding from the library or creating a new unit, and archiving.
 * Opening it makes this class the current class (目前班級) of the unit and lesson pages.
 */
export function ClassPage() {
  const classId = Number(useParams().classId);
  const navigate = useNavigate();
  const ids = useId();
  const { setCurrentClassId } = useApp();
  const [detail, setDetail] = useState<ClassDetail | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [profile, setProfile] = useState("");
  const [profileDirty, setProfileDirty] = useState(false);
  const [profileSaving, setProfileSaving] = useState(false);
  const [profileSaved, setProfileSaved] = useState(false);
  const [profileError, setProfileError] = useState<string | null>(null);

  const [busy, setBusy] = useState(false);
  const [unitsError, setUnitsError] = useState<string | null>(null);
  const [unitsNotice, setUnitsNotice] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  const [library, setLibrary] = useState<Unit[] | null>(null);
  const [creating, setCreating] = useState(false);
  const [newTitle, setNewTitle] = useState("");
  const [createError, setCreateError] = useState<string | null>(null);

  const [editing, setEditing] = useState(false);
  const [confirmArchive, setConfirmArchive] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);

  const guard = useLeaveGuard(profileDirty);

  useEffect(() => {
    let alive = true;
    api
      .getClass(classId)
      .then((d) => {
        if (!alive) return;
        setDetail(d);
        setProfile(d.class_profile);
        if (d.archived_at === null) setCurrentClassId(d.id);
      })
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [classId, setCurrentClassId]);

  const patchClass = (c: SchoolClass) => setDetail((prev) => (prev ? { ...prev, ...c } : prev));

  /** Resolves to an error message, or null when it saved. */
  async function saveProfile(): Promise<string | null> {
    setProfileSaving(true);
    setProfileError(null);
    setProfileSaved(false);
    try {
      const updated = await api.updateClass(classId, { class_profile: profile });
      patchClass(updated);
      setProfile(updated.class_profile);
      setProfileDirty(false);
      setProfileSaved(true);
      return null;
    } catch (err) {
      const message = errorMessage(err);
      setProfileError(message);
      return message;
    } finally {
      setProfileSaving(false);
    }
  }

  /** Runs a change to the class's units and shows the returned detail. */
  async function changeUnits(change: () => Promise<ClassDetail>, notice?: string): Promise<string | null> {
    setBusy(true);
    setUnitsError(null);
    setUnitsNotice(null);
    try {
      setDetail(await change());
      if (notice) setUnitsNotice(notice);
      return null;
    } catch (err) {
      const message = errorMessage(err);
      setUnitsError(message);
      return message;
    } finally {
      setBusy(false);
    }
  }

  function move(from: number, to: number) {
    if (!detail) return;
    const order = detail.units.map((u) => u.unit.id);
    const [moved] = order.splice(from, 1);
    if (moved === undefined) return;
    order.splice(to, 0, moved);
    void changeUnits(() => api.reorderClassUnits(classId, order));
  }

  function openPicker() {
    setPicking(true);
    setCreating(false);
    if (library) return;
    api
      .listUnits()
      .then(setLibrary)
      .catch((err: unknown) => setUnitsError(errorMessage(err)));
  }

  async function createUnit(event: FormEvent) {
    event.preventDefault();
    const title = newTitle.trim();
    if (!title) {
      setCreateError("請輸入單元名稱。");
      return;
    }
    setBusy(true);
    setCreateError(null);
    try {
      // In the library and assigned to this class in one step.
      const unit = await api.createUnit({ title, class_id: classId });
      navigate(`/units/${unit.id}`);
    } catch (err) {
      setCreateError(errorMessage(err));
      setBusy(false);
    }
  }

  async function saveSettings(body: CreateClassRequest): Promise<string | null> {
    try {
      patchClass(await api.updateClass(classId, body));
      setEditing(false);
      return null;
    } catch (err) {
      return errorMessage(err);
    }
  }

  async function setArchived(archived: boolean) {
    setSettingsError(null);
    try {
      const updated = archived ? await api.archiveClass(classId) : await api.unarchiveClass(classId);
      if (archived) {
        setCurrentClassId(null);
        navigate("/");
        return;
      }
      patchClass(updated);
      setCurrentClassId(updated.id);
    } catch (err) {
      setSettingsError(errorMessage(err));
    }
  }

  if (loadError) return <ErrorText message={loadError} />;
  if (!detail) return <Loading />;

  const archived = detail.archived_at !== null;
  const subtitle = classSubtitle(detail, true);
  const step = nextStep(detail.id, detail.next_action);
  const assigned = new Set(detail.units.map((u) => u.unit.id));
  const available = (library ?? []).filter((u) => !assigned.has(u.id));

  return (
    <section aria-labelledby="class-heading">
      <p className="breadcrumb">
        <Link to="/">工作檯</Link> / {detail.name}
      </p>
      <div className="page-intro">
        <p className="eyebrow warm">班級{subtitle ? ` · ${subtitle}` : ""}</p>
        <h2 id="class-heading">{detail.name}</h2>
      </div>

      {archived && (
        <Notice tone="warn">
          這個班級已在 {formatDate(detail.archived_at)} 封存：資料與上課紀錄都還在，但不會出現在工作檯。{" "}
          <button type="button" className="ghost-button small-button" onClick={() => void setArchived(false)}>
            取消封存
          </button>
        </Notice>
      )}

      <div className="unit-stack">
        {!archived && detail.next_action && detail.next_action.kind !== "add_unit" && (
          <section className="panel-card class-next" aria-label="這個班級的下一步">
            <p className="class-card-next">
              <span className="class-card-next-label">下一步</span>
              {step.text}
            </p>
            <Link className="primary-button link-button" to={step.to}>
              {step.action}
            </Link>
          </section>
        )}

        <section className="panel-card" aria-labelledby="class-units-heading">
          <div className="card-heading">
            <h3 id="class-units-heading">這個班級的單元</h3>
            <span className="soft-tag">{detail.units.length} 個單元</span>
          </div>
          <p className="field-hint">
            單元設計放在備課庫，這裡是這個班級取用的單元與進度。同一份設計給其他班級用時，內容是同一份；這班的差異先記在備註。
          </p>

          {detail.units.length === 0 ? (
            <p className="empty-preview">這個班級還沒有單元。從備課庫加入，或新增一個單元。</p>
          ) : (
            <ol className="class-unit-list">
              {detail.units.map((item, i) => (
                <ClassUnitRow
                  key={item.unit.id}
                  item={item}
                  index={i}
                  total={detail.units.length}
                  busy={busy}
                  onMove={move}
                  onRemove={(unit) =>
                    void changeUnits(
                      () => api.unassignUnit(classId, unit.id),
                      `已把「${unit.title}」移出這個班級。單元設計還在備課庫，這班上過的紀錄也都保留。`,
                    )
                  }
                  onNote={(unitId, note) => changeUnits(() => api.updateClassUnit(classId, unitId, note))}
                />
              ))}
            </ol>
          )}
          {unitsNotice && <Notice tone="success">{unitsNotice}</Notice>}
          <ErrorText message={unitsError} />

          {!archived && (
            <div className="button-row class-units-add">
              <button
                type="button"
                className="secondary-button"
                aria-expanded={picking}
                onClick={() => (picking ? setPicking(false) : openPicker())}
              >
                從備課庫加入單元
              </button>
              <button
                type="button"
                className="secondary-button"
                aria-expanded={creating}
                onClick={() => {
                  setCreating((v) => !v);
                  setPicking(false);
                }}
              >
                新增單元
              </button>
            </div>
          )}

          {picking && (
            <section className="class-picker" aria-labelledby={`${ids}-picker`}>
              <h4 id={`${ids}-picker`}>從備課庫加入單元</h4>
              {!library ? (
                <Loading />
              ) : available.length === 0 ? (
                <p className="empty-preview">
                  {library.length === 0 ? "備課庫還沒有單元。" : "備課庫的單元都已經加入這個班級。"}
                </p>
              ) : (
                <ul className="class-picker-list">
                  {available.map((u) => (
                    <li key={u.id}>
                      <div>
                        <strong>{u.title}</strong>
                        <span className="unit-meta">
                          {u.lesson_count} 個課例
                          {u.classes.length > 0 ? ` · ${u.classes.map((c) => c.name).join("、")}也在用` : " · 還沒有班級使用"}
                        </span>
                      </div>
                      <button
                        type="button"
                        className="secondary-button"
                        aria-label={`把「${u.title}」加入這個班級`}
                        disabled={busy}
                        onClick={() => void changeUnits(() => api.assignUnit(classId, u.id), `已加入「${u.title}」。`)}
                      >
                        加入
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          )}

          {creating && (
            <form className="class-new-unit" onSubmit={createUnit}>
              <label className="field-label" htmlFor={`${ids}-new-unit`}>
                新單元名稱
              </label>
              <div className="inline-form">
                <input
                  id={`${ids}-new-unit`}
                  value={newTitle}
                  placeholder="例如：龍騰 B1 L3 Not only… but also 倒裝句"
                  disabled={busy}
                  onChange={(e) => setNewTitle(e.target.value)}
                />
                <button type="submit" className="primary-button" disabled={busy}>
                  建立並加入這個班級
                </button>
              </div>
              <p className="field-hint">新單元會放進備課庫，並由這個班級取用；之後也可以給其他班級用。</p>
              <ErrorText message={createError} />
            </form>
          )}
        </section>

        <section className="panel-card class-profile-card" aria-labelledby="class-profile-heading">
          <div className="card-heading">
            <h3 id="class-profile-heading">班級特性</h3>
            <span className="optional-label">選填</span>
          </div>
          <label className="field-label" htmlFor={`${ids}-profile`}>
            這個班級的特性
          </label>
          <textarea
            id={`${ids}-profile`}
            rows={3}
            value={profile}
            maxLength={CLASS_PROFILE_MAX_CHARS}
            disabled={profileSaving}
            placeholder="例如：普通班 34 人，程度落差大；多數學生不太敢開口，倒裝句常把助動詞放錯位置。"
            onChange={(e) => {
              setProfile(e.target.value);
              setProfileDirty(true);
              setProfileSaved(false);
            }}
          />
          <p className="field-hint">
            在這個班級備課時，Manna 的每次協助都會帶入這段描述（課例可以另外覆寫）。不要寫學生姓名或個人資料。（
            {profile.length.toLocaleString()} / {CLASS_PROFILE_MAX_CHARS.toLocaleString()} 字）
          </p>
          <div className="button-row">
            <button
              type="button"
              className="secondary-button"
              onClick={() => void saveProfile()}
              disabled={profileSaving || !profileDirty}
            >
              {profileSaving ? "儲存中…" : "儲存班級特性"}
            </button>
            {profileDirty && <span className="status-pill">尚未儲存</span>}
          </div>
          {profileSaved && <Notice tone="success">已儲存班級特性。</Notice>}
          <ErrorText message={profileError} />
        </section>

        <section className="panel-card" aria-labelledby="class-settings-heading">
          <div className="card-heading">
            <h3 id="class-settings-heading">班級資料</h3>
          </div>
          {editing ? (
            <ClassForm
              initial={{
                name: detail.name,
                grade: detail.grade,
                school_track: detail.school_track,
                course_label: detail.course_label,
                class_profile: detail.class_profile,
                school_year: detail.school_year,
                term: detail.term,
              }}
              submitLabel="儲存班級資料"
              onSubmit={saveSettings}
              onCancel={() => setEditing(false)}
              hideProfile
            />
          ) : (
            <>
              <p className="class-settings-summary">
                {detail.name}
                {subtitle ? ` · ${subtitle}` : ""}
              </p>
              <div className="button-row">
                <button type="button" className="secondary-button" onClick={() => setEditing(true)}>
                  修改班級資料
                </button>
                {!archived && !confirmArchive && (
                  <button type="button" className="ghost-button" onClick={() => setConfirmArchive(true)}>
                    封存這個班級
                  </button>
                )}
              </div>
              {confirmArchive && (
                <div className="notice notice-warn class-archive-confirm" role="group" aria-label="封存這個班級">
                  <p>
                    封存後，這個班級不會出現在工作檯；單元設計留在備課庫，這班的上課紀錄與補寫都保留，之後可以取消封存。
                  </p>
                  <div className="button-row">
                    <button type="button" className="primary-button" onClick={() => void setArchived(true)}>
                      確定封存
                    </button>
                    <button type="button" className="ghost-button" onClick={() => setConfirmArchive(false)}>
                      取消
                    </button>
                  </div>
                </div>
              )}
              <ErrorText message={settingsError} />
            </>
          )}
        </section>
      </div>

      <LeaveConfirm guard={guard} message="離開這頁，尚未儲存的班級特性就會遺失。" onSave={saveProfile} />
    </section>
  );
}
