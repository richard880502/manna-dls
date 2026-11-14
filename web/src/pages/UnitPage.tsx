import { Fragment, useEffect, useState, type FormEvent, type ReactNode } from "react";
import { Link, useParams } from "react-router";
import { ApiError, api, errorMessage } from "../api/client";
import type { Lesson, LessonRun, PrepRoute, Unit } from "../api/types";
import { ErrorText, Loading, Notice, PrivacyHint } from "../components/common";
import { MaterialsPanel } from "../components/unit/MaterialsPanel";
import { RouteMenu } from "../components/unit/RouteMenu";
import { RouteOptions } from "../components/unit/RouteOptions";
import { UnitIdeas } from "../components/unit/UnitIdeas";
import { useApp } from "../context/AppContext";
import { usedByText, useActiveClass } from "../lib/classes";
import { formatDate } from "../lib/labels";
import { CLASS_PROFILE_MAX_CHARS, PREP_ROUTE_LABELS, effectiveRoute } from "../lib/preferences";

const ROUTE_LOCKED_MESSAGE =
  "主持人目前把備課順序固定為「目標優先」，暫時不能切換備課菜單。每個畫面仍然都可以打開。";

const IDEAS_ROUTE_HINTS: Record<PrepRoute, string> = {
  goal: "依你的備課順序，可以先寫 Q1–Q3：要學會什麼、為什麼重要、這次先不教什麼。",
  material: "從教材候選重點放進來的學習重點會出現在單元想法，你可以修改或移除。",
  student: "依你的備課順序，可以先寫 Q5、Q6：學生可能怎麼想、還有什麼會影響教學。",
  activity: "可以先寫活動；寫完課例後再回來補單元想法。",
};

/** 「高一 3 班、高一 7 班上過」 for one lesson, from the teacher's finished runs. */
function taughtBy(runs: LessonRun[], lessonId: number): string {
  const mine = runs.filter((r) => r.lesson_id === lessonId && r.ended_at !== null);
  if (mine.length === 0) return "還沒上";
  const names = [...new Set(mine.map((r) => r.class_name).filter((n): n is string => !!n))];
  return names.length > 0 ? `${names.join("、")}上過` : "上過";
}

/**
 * 單元頁 (docs/DESIGN_OVERHAUL.md): the unit's lessons as ruled rows with one filled action
 * (carry on with the next lesson), and 單元想法 (CoRe) beside them as a companion that never
 * blocks. Materials, the class profile and the prep order follow as plain sections.
 */
export function UnitPage() {
  const unitId = Number(useParams().unitId);
  const { routeChoiceEnabled } = useApp();
  const [unit, setUnit] = useState<Unit | null>(null);
  const [lessons, setLessons] = useState<Lesson[] | null>(null);
  const [runs, setRuns] = useState<LessonRun[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [materialsCount, setMaterialsCount] = useState<number | null>(null);

  const [switching, setSwitching] = useState(false);
  const [routeLocked, setRouteLocked] = useState(false);
  const [routeError, setRouteError] = useState<string | null>(null);
  const [routeNotice, setRouteNotice] = useState<string | null>(null);

  const [profile, setProfile] = useState("");
  const [profileDirty, setProfileDirty] = useState(false);
  const [profileSaving, setProfileSaving] = useState(false);
  const [profileError, setProfileError] = useState<string | null>(null);
  const [profileSaved, setProfileSaved] = useState(false);

  const [addingLesson, setAddingLesson] = useState(false);
  const [title, setTitle] = useState("");
  const [lessonDate, setLessonDate] = useState("");
  const [classDescription, setClassDescription] = useState("");
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    Promise.all([api.getUnit(unitId), api.listLessons(unitId)])
      .then(([u, l]) => {
        if (!alive) return;
        setUnit(u);
        setProfile(u.class_profile ?? "");
        setLessons(l);
      })
      .catch((err: unknown) => {
        if (alive) setLoadError(errorMessage(err));
      });
    api
      .listRuns()
      .then((list) => alive && setRuns(list))
      // Only the quiet 「上過」 facts; the page works without them.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [unitId]);

  const { active: activeClass } = useActiveClass(unit);
  const choiceEnabled = routeChoiceEnabled && !routeLocked;
  const route = effectiveRoute(choiceEnabled, unit?.prep_route);

  async function switchRoute(next: PrepRoute) {
    if (!unit || next === route) return;
    const previous = unit;
    setUnit({ ...unit, prep_route: next });
    setSwitching(true);
    setRouteError(null);
    setRouteNotice(null);
    try {
      const updated = await api.updateUnit(unit.id, { prep_route: next });
      setUnit((u) => ({ ...(u ?? previous), ...updated }));
      setRouteNotice(`已切換為「${PREP_ROUTE_LABELS[next]}」。`);
    } catch (err) {
      setUnit(previous);
      if (err instanceof ApiError && err.status === 409) {
        setRouteLocked(true);
        setRouteError(ROUTE_LOCKED_MESSAGE);
      } else {
        setRouteError(errorMessage(err));
      }
    } finally {
      setSwitching(false);
    }
  }

  async function saveProfile() {
    if (!unit) return;
    setProfileSaving(true);
    setProfileError(null);
    setProfileSaved(false);
    try {
      const updated = await api.updateUnit(unit.id, { class_profile: profile });
      setUnit((u) => ({ ...(u ?? unit), ...updated }));
      setProfile(updated.class_profile ?? profile);
      setProfileDirty(false);
      setProfileSaved(true);
    } catch (err) {
      setProfileError(errorMessage(err));
    } finally {
      setProfileSaving(false);
    }
  }

  async function onCreate(event: FormEvent) {
    event.preventDefault();
    const trimmed = title.trim();
    if (!trimmed) {
      setCreateError("請輸入課例名稱。");
      return;
    }
    setCreating(true);
    setCreateError(null);
    try {
      const lesson = await api.createLesson(unitId, {
        title: trimmed,
        ...(lessonDate ? { lesson_date: lessonDate } : {}),
        ...(classDescription.trim() ? { class_description: classDescription.trim() } : {}),
      });
      setLessons((prev) => [...(prev ?? []), lesson]);
      setTitle("");
      setLessonDate("");
      setClassDescription("");
      setAddingLesson(false);
    } catch (err) {
      setCreateError(errorMessage(err));
    } finally {
      setCreating(false);
    }
  }

  if (loadError) return <ErrorText message={loadError} />;
  if (!unit || !lessons) return <Loading />;

  // The lesson to carry on with: the first one not taught yet (in this class, when there is one).
  const taught = new Set(
    (runs ?? [])
      .filter((r) => r.ended_at !== null && (!activeClass || r.class_id === activeClass.id))
      .map((r) => r.lesson_id),
  );
  const continueLesson = lessons.find((l) => !taught.has(l.id)) ?? lessons[lessons.length - 1] ?? null;
  // With no lesson yet, the form is the page's one action; it is open without being asked for.
  const lessonFormOpen = addingLesson || lessons.length === 0;

  const lessonsSection = (
    <section aria-labelledby="lessons-heading">
      <h2 id="lessons-heading">課例</h2>
      {route === "activity" && (
        <p className="quiet-note">活動優先：先在課例寫活動點子與教案步驟，再回頭補單元想法。</p>
      )}
      {lessons.length === 0 ? (
        <p className="empty-line">還沒有課例。建立第一節課例開始備課。</p>
      ) : (
        <ol className="rows">
          {lessons.map((lesson, i) => {
            const meta = [lesson.lesson_date ? formatDate(lesson.lesson_date) : "", lesson.class_description]
              .filter(Boolean)
              .join(" · ");
            return (
              <li key={lesson.id} className="row">
                <div>
                  <h3>
                    <span className="num">{i + 1}</span>
                    <Link to={`/lessons/${lesson.id}`}>{lesson.title}</Link>
                  </h3>
                  {meta && <p className="meta">{meta}</p>}
                </div>
                {runs && <p className="side">{taughtBy(runs, lesson.id)}</p>}
              </li>
            );
          })}
        </ol>
      )}

      {lessonFormOpen ? (
        <form className="inline-form-block create-lesson-form" onSubmit={onCreate} aria-labelledby="new-lesson-heading">
          <h3 id="new-lesson-heading">新增一節課例</h3>
          <label className="field-label" htmlFor="lesson-title">
            課例名稱
          </label>
          <input
            id="lesson-title"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="例如：第 2 節・句型練習"
            disabled={creating}
          />
          <div className="two-fields">
            <div>
              <label className="field-label" htmlFor="lesson-date">
                上課日期（可以不填）
              </label>
              <input
                id="lesson-date"
                type="date"
                value={lessonDate}
                onChange={(e) => setLessonDate(e.target.value)}
                disabled={creating}
              />
            </div>
            <div>
              <label className="field-label" htmlFor="lesson-class">
                這節課的班級描述（可以不填）
              </label>
              <input
                id="lesson-class"
                value={classDescription}
                onChange={(e) => setClassDescription(e.target.value)}
                placeholder={activeClass ? "留白則沿用班級的班級特性" : "留白則沿用單元的班級特性"}
                disabled={creating}
              />
            </div>
          </div>
          <PrivacyHint />
          <ErrorText message={createError} />
          <div className="button-row">
            <button className={lessons.length === 0 ? "primary-button" : "secondary-button"} type="submit" disabled={creating}>
              {creating ? "建立中…" : "建立課例"}
            </button>
            {addingLesson && lessons.length > 0 && (
              <button type="button" className="text-link is-plain" disabled={creating} onClick={() => setAddingLesson(false)}>
                取消
              </button>
            )}
          </div>
        </form>
      ) : (
        <p className="after-rows">
          <button type="button" className="text-link" onClick={() => setAddingLesson(true)}>
            新增一節課例
          </button>
        </p>
      )}
    </section>
  );

  const unitProfileFields = (
    <>
      <label className="field-label" htmlFor="unit-class-profile">
        {route === "student" ? "班級特性、學生最近卡住的地方" : "這個單元的班級特性"}
      </label>
      <textarea
        id="unit-class-profile"
        rows={3}
        value={profile}
        maxLength={CLASS_PROFILE_MAX_CHARS}
        disabled={profileSaving}
        placeholder="例如：高一普通班，程度落差大；多數學生不太敢開口，倒裝句常把助動詞放錯位置。"
        onChange={(e) => {
          setProfile(e.target.value);
          setProfileDirty(true);
          setProfileSaved(false);
        }}
      />
      <p className="field-hint">
        不要寫學生姓名或個人資料。（{profile.length.toLocaleString()} / {CLASS_PROFILE_MAX_CHARS.toLocaleString()} 字）
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
      </div>
      {profileSaved && <Notice tone="success">已儲存班級特性。</Notice>}
      <ErrorText message={profileError} />
    </>
  );

  // In a class the profile is the class's (edited on the class page); the unit's own profile is
  // legacy text, used only when the class has none.
  const profileSection = (
    <section className="section" aria-labelledby="class-profile-heading">
      <div className="head-row">
        <h2 id="class-profile-heading">{route === "student" ? "班級特性與學生卡點" : "班級特性"}</h2>
        {activeClass && (
          <Link className="text-link is-plain" to={`/classes/${activeClass.id}`}>
            到班級頁修改
          </Link>
        )}
      </div>
      {activeClass ? (
        <>
          <p className="quiet-note">{activeClass.name}的班級特性。在這個班級備課時，Manna 提建議都會參考（課例可以另外覆寫）。</p>
          <p className={activeClass.class_profile ? "lead" : "empty-line"}>
            {activeClass.class_profile ||
              (unit.class_profile
                ? "這個班級還沒有寫班級特性，目前沿用下面「這個單元的班級特性」。"
                : "這個班級還沒有寫班級特性。")}
          </p>
          {(unit.class_profile || profileDirty) && (
            <details className="unit-profile-legacy" open={!activeClass.class_profile}>
              <summary>這個單元另外寫的班級特性（班級沒有寫時才使用）</summary>
              {unitProfileFields}
            </details>
          )}
        </>
      ) : unit.classes.length > 1 ? (
        // Opened from the library: several classes use it, so there is no one profile to show.
        <>
          <p className="quiet-note">
            {unit.classes.length} 個班級在用這個單元，班級特性寫在各班的班級頁；從班級進來備課時，Manna 會參考那一班的。
          </p>
          {(unit.class_profile || profileDirty) && (
            <details className="unit-profile-legacy">
              <summary>這個單元另外寫的班級特性（班級沒有寫時才使用）</summary>
              {unitProfileFields}
            </details>
          )}
        </>
      ) : (
        <>
          <p className="quiet-note">單元內的課例會沿用這段描述（課例可以另外覆寫），Manna 提建議都會參考。</p>
          {unitProfileFields}
        </>
      )}
    </section>
  );

  const materialsSection = (
    <MaterialsPanel
      unitId={unit.id}
      classId={activeClass?.id ?? null}
      emphasized={route === "material"}
      onCountChange={setMaterialsCount}
      onCoreChanged={() => setUnit((u) => (u && u.core_status === "none" ? { ...u, core_status: "draft" } : u))}
    />
  );

  const routeSection = (
    <section className="section route-card" aria-labelledby="route-heading">
      <div className="head-row">
        <h2 id="route-heading">{choiceEnabled ? `備課菜單：${PREP_ROUTE_LABELS[route]}` : "備課順序"}</h2>
        {switching && <span className="quiet-note">切換中…</span>}
      </div>
      <RouteMenu
        route={route}
        unit={unit}
        lessons={lessons}
        materialsCount={materialsCount}
      />
      {choiceEnabled && (
        <details className="route-switch">
          <summary>換一種備課菜單</summary>
          <RouteOptions
            name={`unit-${unit.id}-route`}
            legend="切換備課菜單"
            value={route}
            onChange={(r) => void switchRoute(r)}
            disabled={switching}
            hint="切換只改變畫面順序與強調，已寫的內容都會保留。"
          />
        </details>
      )}
      {routeNotice && <Notice tone="success">{routeNotice}</Notice>}
      <ErrorText message={routeError} />
    </section>
  );

  // The prep route only changes the order below the lessons: what it puts first comes first.
  const ORDER: Record<PrepRoute, string[]> = {
    goal: ["profile", "materials"],
    material: ["materials", "profile"],
    student: ["profile", "materials"],
    activity: ["profile", "materials"],
  };
  const blocks: Record<string, ReactNode> = { profile: profileSection, materials: materialsSection };

  return (
    <>
      <p className="crumbs">
        {activeClass ? (
          <>
            <Link to="/">班級</Link> / <Link to={`/classes/${activeClass.id}`}>{activeClass.name}</Link>
          </>
        ) : (
          <Link to="/library">備課庫</Link>
        )}{" "}
        / {unit.title}
      </p>
      <div className="head-row page-head">
        <div>
          <h1 id="unit-heading">{unit.title}</h1>
          <p className="lead">
            {usedByText(unit.classes)}
            {unit.classes.length === 0 && (
              <>
                。<Link to="/library">到備課庫加到班級</Link>
              </>
            )}
          </p>
        </div>
        {continueLesson && (
          <Link className="primary-button link-button" to={`/lessons/${continueLesson.id}`}>
            繼續備課：{continueLesson.title}
          </Link>
        )}
      </div>

      <div className="with-side">
        {lessonsSection}
        <UnitIdeas unit={unit} hint={IDEAS_ROUTE_HINTS[route]} />
      </div>

      {ORDER[route].map((key) => (
        <Fragment key={key}>{blocks[key]}</Fragment>
      ))}
      {routeSection}
    </>
  );
}
