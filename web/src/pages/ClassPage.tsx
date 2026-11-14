import { useEffect, useId, useState, type FormEvent } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { ClassDetail, ClassUnit, CreateClassRequest, LessonRun, SchoolClass, Unit } from "../api/types";
import { ClassForm } from "../components/classes/ClassForm";
import { NextLesson } from "../components/classes/NextLesson";
import { ErrorText, Loading, Notice } from "../components/common";
import { LeaveConfirm, useLeaveGuard } from "../components/LeaveGuard";
import { useApp } from "../context/AppContext";
import { CLASS_NOTE_MAX_CHARS, classSubtitle, usedByText } from "../lib/classes";
import { formatDate, formatShortDate } from "../lib/labels";
import { CLASS_PROFILE_MAX_CHARS } from "../lib/preferences";

/** One unit as this class uses it: a ruled row with this class's progress and note. */
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
  const [more, setMore] = useState(false);
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

  const progress =
    item.lessons_total === 0
      ? "還沒有課例"
      : item.lessons_taught === 0
        ? `還沒上 · ${item.lessons_total} 節課例`
        : `已上 ${item.lessons_taught} 節，共 ${item.lessons_total} 節`;

  return (
    <li className="row">
      <div>
        <h3>
          <span className="num">{index + 1}</span>
          <Link to={`/units/${unit.id}`}>{unit.title}</Link>
        </h3>
        <p className="meta">
          {progress}
          {item.last_run_at ? ` · 最近 ${formatShortDate(item.last_run_at)}` : ""}
          {item.note ? `　·　這班的備註：${item.note}` : ""}
        </p>
      </div>
      <p className="side">
        <button
          type="button"
          className="text-link is-plain"
          aria-label={`${item.note ? "改" : "加"}「${unit.title}」的備註`}
          onClick={() => {
            setDraft(item.note);
            setEditing((v) => !v);
          }}
        >
          {item.note ? "改備註" : "加備註"}
        </button>
        <button
          type="button"
          className="text-link is-plain"
          aria-expanded={more}
          aria-label={`「${unit.title}」的順序與移出`}
          onClick={() => setMore((v) => !v)}
        >
          更多
        </button>
      </p>

      {editing && (
        <form className="row-more" onSubmit={saveNote}>
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
      )}

      {more && (
        <div className="row-more row-tools">
          <button
            type="button"
            className="text-link is-plain"
            aria-label={`把「${unit.title}」往前移`}
            disabled={busy || index === 0}
            onClick={() => onMove(index, index - 1)}
          >
            上移
          </button>
          <button
            type="button"
            className="text-link is-plain"
            aria-label={`把「${unit.title}」往後移`}
            disabled={busy || index === total - 1}
            onClick={() => onMove(index, index + 1)}
          >
            下移
          </button>
          <button
            type="button"
            className="text-link is-plain"
            aria-label={`把「${unit.title}」移出這個班級`}
            disabled={busy}
            onClick={() => onRemove(unit)}
          >
            移出班級
          </button>
        </div>
      )}
    </li>
  );
}

/**
 * 班級頁 (docs/DESIGN_OVERHAUL.md): the class's next lesson with 「開始上課」, then its units
 * (order, this class's progress and note), its lesson records (write-up and review), and its
 * profile. Opening it makes this class the class in play for the unit and lesson pages.
 */
export function ClassPage() {
  const classId = Number(useParams().classId);
  const navigate = useNavigate();
  const ids = useId();
  const { setCurrentClassId } = useApp();
  const [detail, setDetail] = useState<ClassDetail | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [runs, setRuns] = useState<LessonRun[]>([]);

  const [profile, setProfile] = useState("");
  const [profileEditing, setProfileEditing] = useState(false);
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
    api
      .listRuns(classId)
      .then((list) => alive && setRuns(list))
      // The records are one section; the rest of the page works without them.
      .catch(() => undefined);
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
      setProfileEditing(false);
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
  const next = detail.next_action;
  const showNext = !archived && next !== null && next.kind !== "add_unit";
  const assigned = new Set(detail.units.map((u) => u.unit.id));
  const available = (library ?? []).filter((u) => !assigned.has(u.id));

  return (
    <>
      <p className="crumbs">
        <Link to="/">班級</Link> / {detail.name}
      </p>

      {showNext ? (
        <NextLesson
          classId={detail.id}
          kicker={`${detail.name} · ${next.kind === "resume_run" ? "上課中" : next.kind === "plan_unit" ? "下一個單元" : "下一節"}`}
          next={next}
          headingId="class-heading"
        />
      ) : (
        <div className="page-head">
          <h1 id="class-heading">{detail.name}</h1>
          {subtitle && <p className="lead">{subtitle}</p>}
        </div>
      )}

      {archived && (
        <Notice tone="warn">
          這個班級已在 {formatDate(detail.archived_at)} 封存：資料與上課紀錄都還在，但不會出現在班級清單。{" "}
          <button type="button" className="text-link" onClick={() => void setArchived(false)}>
            取消封存
          </button>
        </Notice>
      )}

      <section className="section" aria-labelledby="class-units-heading">
        <div className="head-row">
          <h2 id="class-units-heading">這班的單元</h2>
          {!archived && (
            <span className="actions">
              <button
                type="button"
                className="text-link is-plain"
                aria-expanded={creating}
                onClick={() => {
                  setCreating((v) => !v);
                  setPicking(false);
                }}
              >
                新增單元
              </button>
              <button
                type="button"
                className="text-link"
                aria-expanded={picking}
                onClick={() => (picking ? setPicking(false) : openPicker())}
              >
                從備課庫加入單元
              </button>
            </span>
          )}
        </div>

        {picking && (
          <section className="inline-form-block" aria-labelledby={`${ids}-picker`}>
            <h3 id={`${ids}-picker`}>從備課庫加入單元</h3>
            {!library ? (
              <Loading />
            ) : available.length === 0 ? (
              <p className="empty-line">
                {library.length === 0 ? "備課庫還沒有單元。" : "備課庫的單元都已經加入這個班級。"}
              </p>
            ) : (
              <ul className="rows is-compact">
                {available.map((u) => (
                  <li key={u.id} className="row">
                    <div>
                      <strong>{u.title}</strong>
                      <p className="meta">
                        {u.lesson_count} 節課例 · {usedByText(u.classes)}
                      </p>
                    </div>
                    <p className="side">
                      <button
                        type="button"
                        className="secondary-button small-button"
                        aria-label={`把「${u.title}」加入這個班級`}
                        disabled={busy}
                        onClick={() => void changeUnits(() => api.assignUnit(classId, u.id), `已加入「${u.title}」。`)}
                      >
                        加入
                      </button>
                    </p>
                  </li>
                ))}
              </ul>
            )}
          </section>
        )}

        {creating && (
          <form className="inline-form-block" onSubmit={createUnit}>
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
              <button type="submit" className="secondary-button" disabled={busy}>
                建立並加入這個班級
              </button>
            </div>
            <p className="field-hint">新單元會放進備課庫，並由這個班級取用；之後也可以給其他班級用。</p>
            <ErrorText message={createError} />
          </form>
        )}

        {detail.units.length === 0 ? (
          <p className="empty-line">這個班級還沒有單元。從備課庫加入，或新增一個單元。</p>
        ) : (
          <ol className="rows">
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
                    `已把「${unit.title}」移出這個班級。單元還在備課庫，這班上過的紀錄也都保留。`,
                  )
                }
                onNote={(unitId, note) => changeUnits(() => api.updateClassUnit(classId, unitId, note))}
              />
            ))}
          </ol>
        )}
        {unitsNotice && <Notice tone="success">{unitsNotice}</Notice>}
        <ErrorText message={unitsError} />
      </section>

      {runs.length > 0 && (
        <section className="section" aria-labelledby="class-runs-heading">
          <h2 id="class-runs-heading">上課紀錄</h2>
          <ul className="rows">
            {runs.map((r) => {
              const open = r.ended_at === null;
              const to = open ? `/runs/${r.id}` : `/runs/${r.id}/review`;
              return (
                <li key={r.id} className="row">
                  <div>
                    <h3>
                      <Link to={to}>
                        {r.unit_title}　{r.lesson_title}
                      </Link>
                    </h3>
                    <p className="meta">
                      {formatShortDate(r.started_at)} · 順利 {r.good_count} 次 · 卡住 {r.stuck_count} 次 ·{" "}
                      {open ? "上課中" : r.review_completed_at ? "已補寫" : "還沒補寫"}
                    </p>
                  </div>
                  <p className="side">
                    <Link
                      className={`text-link${!open && r.review_completed_at ? " is-plain" : ""}`}
                      to={to}
                      aria-label={`${open ? "回到課堂" : r.review_completed_at ? "回顧" : "補寫"}：${r.lesson_title} ${formatShortDate(r.started_at)}`}
                    >
                      {open ? "回到課堂" : r.review_completed_at ? "回顧" : "補寫"}
                    </Link>
                  </p>
                </li>
              );
            })}
          </ul>
        </section>
      )}

      <section className="section" aria-labelledby="class-profile-heading">
        <div className="head-row">
          <h2 id="class-profile-heading">班級特性</h2>
          {!profileEditing && (
            <button type="button" className="text-link is-plain" onClick={() => setProfileEditing(true)}>
              {detail.class_profile ? "修改" : "寫一點"}
            </button>
          )}
        </div>
        {profileEditing ? (
          <>
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
              Manna 提建議時會參考這段（課例可以另外覆寫）。不要寫學生姓名或個人資料。（
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
              <button
                type="button"
                className="text-link is-plain"
                disabled={profileSaving}
                onClick={() => {
                  setProfile(detail.class_profile);
                  setProfileDirty(false);
                  setProfileEditing(false);
                }}
              >
                取消
              </button>
            </div>
          </>
        ) : (
          <>
            <p className={detail.class_profile ? "lead" : "empty-line"}>
              {detail.class_profile || "還沒有寫。幾句話就夠，例如人數、程度落差、討論時的樣子。"}
            </p>
            <p className="quiet-note">Manna 提建議時會參考這段。不要寫學生姓名或個人資料。</p>
          </>
        )}
        {profileSaved && <Notice tone="success">已儲存班級特性。</Notice>}
        <ErrorText message={profileError} />
      </section>

      <section className="section" aria-labelledby="class-settings-heading">
        <div className="head-row">
          <h2 id="class-settings-heading">班級資料</h2>
          {!editing && (
            <span className="actions">
              {!archived && !confirmArchive && (
                <button type="button" className="text-link is-plain" onClick={() => setConfirmArchive(true)}>
                  封存這個班級
                </button>
              )}
              <button type="button" className="text-link is-plain" onClick={() => setEditing(true)}>
                修改班級資料
              </button>
            </span>
          )}
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
            <p className="lead">
              {detail.name}
              {subtitle ? ` · ${subtitle}` : ""}
            </p>
            {confirmArchive && (
              <div className="notice notice-warn" role="group" aria-label="封存這個班級">
                <p>
                  封存後，這個班級不會出現在班級清單；單元留在備課庫，這班的上課紀錄與補寫都保留，之後可以取消封存。
                </p>
                <div className="button-row">
                  <button type="button" className="secondary-button" onClick={() => void setArchived(true)}>
                    確定封存
                  </button>
                  <button type="button" className="text-link is-plain" onClick={() => setConfirmArchive(false)}>
                    取消
                  </button>
                </div>
              </div>
            )}
            <ErrorText message={settingsError} />
          </>
        )}
      </section>

      <LeaveConfirm guard={guard} message="離開這頁，尚未儲存的班級特性就會遺失。" onSave={saveProfile} />
    </>
  );
}
