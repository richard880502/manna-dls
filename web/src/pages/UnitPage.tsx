import { Fragment, useEffect, useState, type FormEvent, type ReactNode } from "react";
import { Link, useParams } from "react-router";
import { ApiError, api, errorMessage } from "../api/client";
import type { Lesson, PrepRoute, Unit } from "../api/types";
import { ClassContextBar } from "../components/classes/ClassContextBar";
import { ErrorText, Loading, Notice, PrivacyHint } from "../components/common";
import { MaterialsPanel } from "../components/unit/MaterialsPanel";
import { RouteMenu } from "../components/unit/RouteMenu";
import { RouteOptions } from "../components/unit/RouteOptions";
import { useApp } from "../context/AppContext";
import { useActiveClass } from "../lib/classes";
import { CORE_STATUS_LABELS, formatDate } from "../lib/labels";
import { CLASS_PROFILE_MAX_CHARS, PREP_ROUTE_LABELS, effectiveRoute } from "../lib/preferences";

const ROUTE_LOCKED_MESSAGE =
  "主持人目前把備課順序固定為「目標優先」，暫時不能切換備課菜單。每個畫面仍然都可以打開。";

const CORE_ROUTE_HINTS: Record<PrepRoute, string> = {
  goal: "建議先寫 Q1–Q3：學生要學會什麼、為什麼要學、這次先不教什麼。",
  material: "從教材候選重點放進來的學習重點會出現在 CoRe，你可以修改或移除。",
  student: "建議先寫 Q5、Q6：學生可能怎麼想或誤解、還有哪些因素會影響教學。",
  activity: "可以先寫活動；寫完課例後再回來連結並補寫 CoRe。",
};

export function UnitPage() {
  const unitId = Number(useParams().unitId);
  const { routeChoiceEnabled } = useApp();
  const [unit, setUnit] = useState<Unit | null>(null);
  const [lessons, setLessons] = useState<Lesson[] | null>(null);
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
    } catch (err) {
      setCreateError(errorMessage(err));
    } finally {
      setCreating(false);
    }
  }

  if (loadError) return <ErrorText message={loadError} />;
  if (!unit || !lessons) return <Loading />;

  const routeCard = (
    <section className="panel-card route-card" aria-labelledby="route-heading">
      <div className="card-heading">
        <h3 id="route-heading">{choiceEnabled ? `備課菜單：${PREP_ROUTE_LABELS[route]}` : "備課順序"}</h3>
        {switching && <span className="status-pill">切換中…</span>}
      </div>
      <RouteMenu
        route={route}
        routeChoiceEnabled={choiceEnabled}
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

  const coreCard = (
    <section className="panel-card" aria-labelledby="unit-core-heading">
      <div className="card-heading">
        <h3 id="unit-core-heading">單元前 CoRe</h3>
        <span className={`core-status core-status-${unit.core_status}`}>{CORE_STATUS_LABELS[unit.core_status]}</span>
      </div>
      <p>寫下這個單元 2–3 個最重要的學習重點，以及每個重點的八題思考。可以留白，也可以選「目前沒有想法」。</p>
      <p className="field-hint route-hint">{CORE_ROUTE_HINTS[route]}</p>
      <Link className="primary-button link-button" to={`/units/${unit.id}/core`}>
        {unit.core_status === "none" ? "開始寫 CoRe" : "打開 CoRe"}
      </Link>
    </section>
  );

  const lessonsCard = (
    <section className="panel-card" aria-labelledby="lessons-heading">
      <div className="card-heading">
        <h3 id="lessons-heading">課例</h3>
        <span className="soft-tag">{lessons.length} 個課例</span>
      </div>
      {route === "activity" && (
        <p className="field-hint route-hint">活動優先：先在課例寫活動點子與教案步驟，再回頭連結 CoRe 重點。</p>
      )}
      {lessons.length === 0 ? (
        <p className="empty-preview">還沒有課例。建立第一個課例開始備課。</p>
      ) : (
        <ul className="lesson-list">
          {lessons.map((lesson) => (
            <li key={lesson.id}>
              <Link to={`/lessons/${lesson.id}`}>
                <strong>{lesson.title}</strong>
                <span>
                  {lesson.lesson_date ? formatDate(lesson.lesson_date) : "未設定日期"}
                  {lesson.class_description ? ` · ${lesson.class_description}` : ""}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      )}

      <form className="create-lesson-form" onSubmit={onCreate}>
        <h4>建立新課例</h4>
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
              上課日期（選填）
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
              班級描述（選填）
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
        <button className="primary-button" type="submit" disabled={creating}>
          {creating ? "建立中…" : "建立課例"}
        </button>
      </form>
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

  // In a class context the profile is the class's (edited on the class page); the unit's own
  // profile is legacy text, used only when the class has none.
  const classProfileCard = activeClass && (
    <section
      className={`panel-card class-profile-card${route === "student" ? " is-route-first" : ""}`}
      aria-labelledby="class-profile-heading"
    >
      <div className="card-heading">
        <h3 id="class-profile-heading">{route === "student" ? "班級特性與學生卡點" : "班級特性"}</h3>
      </div>
      <div className="profile-prefill">
        <div className="profile-prefill-head">
          <div>
            <span className="field-label">{activeClass.name}的班級特性</span>
            <p className="field-hint">在這個班級備課時，Manna 的每次 AI 協助都會帶入（課例可以另外覆寫）。</p>
          </div>
          <Link className="ghost-button small-button link-button" to={`/classes/${activeClass.id}`}>
            到班級頁修改
          </Link>
        </div>
        <p className="profile-prefill-text">
          {activeClass.class_profile ||
            (unit.class_profile
              ? "這個班級還沒有寫班級特性，目前沿用下面「這個單元的班級特性」。"
              : "這個班級還沒有寫班級特性。")}
        </p>
      </div>
      {(unit.class_profile || profileDirty) && (
        <details className="unit-profile-legacy" open={!activeClass.class_profile}>
          <summary>這個單元另外寫的班級特性（班級沒有寫時才使用）</summary>
          {unitProfileFields}
        </details>
      )}
    </section>
  );

  const profileCard = classProfileCard || (
    <section
      className={`panel-card class-profile-card${route === "student" ? " is-route-first" : ""}`}
      aria-labelledby="class-profile-heading"
    >
      <div className="card-heading">
        <h3 id="class-profile-heading">{route === "student" ? "班級特性與學生卡點" : "班級特性"}</h3>
        <span className="optional-label">選填</span>
      </div>
      <p className="field-hint">
        單元內的課例會沿用這段描述（課例可以另外覆寫），Manna 的每次 AI 協助都會自動帶入。
      </p>
      {unitProfileFields}
    </section>
  );

  const materialsCard = (
    <MaterialsPanel
      unitId={unit.id}
      classId={activeClass?.id ?? null}
      emphasized={route === "material"}
      onCountChange={setMaterialsCount}
      onCoreChanged={() => setUnit((u) => (u && u.core_status === "none" ? { ...u, core_status: "draft" } : u))}
    />
  );

  const grid = (
    <div className={`unit-grid${route === "activity" ? " is-lessons-first" : ""}`}>
      {route === "activity" ? (
        <>
          {lessonsCard}
          {coreCard}
        </>
      ) : (
        <>
          {coreCard}
          {lessonsCard}
        </>
      )}
    </div>
  );

  const ORDER: Record<PrepRoute, string[]> = {
    goal: ["grid", "profile", "materials"],
    material: ["materials", "grid", "profile"],
    student: ["profile", "grid", "materials"],
    activity: ["grid", "profile", "materials"],
  };
  const blocks: Record<string, ReactNode> = { grid, profile: profileCard, materials: materialsCard };

  return (
    <section aria-labelledby="unit-heading">
      <p className="breadcrumb">
        {activeClass ? (
          <Link to={`/classes/${activeClass.id}`}>{activeClass.name}</Link>
        ) : (
          <Link to="/library">備課庫</Link>
        )}{" "}
        / {unit.title}
      </p>
      <div className="page-intro">
        <p className="eyebrow warm">Unit{choiceEnabled ? ` · ${PREP_ROUTE_LABELS[route]}` : ""}</p>
        <h2 id="unit-heading">{unit.title}</h2>
      </div>
      <ClassContextBar unit={unit} />

      <div className="unit-stack">
        {routeCard}
        {ORDER[route].map((key) => (
          <Fragment key={key}>{blocks[key]}</Fragment>
        ))}
      </div>
    </section>
  );
}
