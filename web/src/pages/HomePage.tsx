import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { CreateClassRequest, SchoolClass } from "../api/types";
import { ClassForm, emptyClassValues } from "../components/classes/ClassForm";
import { ErrorText, Loading, Notice } from "../components/common";
import { useApp } from "../context/AppContext";
import { classSubtitle, nextStep } from "../lib/classes";
import { formatDate } from "../lib/labels";

const FIRST_CLASS_SKIPPED_KEY = "manna.firstClassSkipped";

function loadSkipped(): boolean {
  try {
    return window.sessionStorage.getItem(FIRST_CLASS_SKIPPED_KEY) === "1";
  } catch {
    return false;
  }
}

function ClassCard({ item }: { item: SchoolClass }) {
  const { setCurrentClassId } = useApp();
  const step = nextStep(item.id, item.next_action);
  const subtitle = classSubtitle(item);
  // Following a class's next step means working in that class.
  const enter = () => setCurrentClassId(item.id);
  return (
    <li className="class-card panel-card">
      <div className="class-card-head">
        <h4>
          <Link to={`/classes/${item.id}`} onClick={enter}>
            {item.name}
          </Link>
        </h4>
        <span className="soft-tag">{item.unit_count} 個單元</span>
      </div>
      {subtitle && <p className="unit-meta">{subtitle}</p>}
      <p className="class-card-next">
        <span className="class-card-next-label">下一步</span>
        {step.text}
      </p>
      <div className="class-card-actions">
        <Link className="primary-button link-button" to={step.to} onClick={enter}>
          {step.action}
        </Link>
        <Link className="secondary-button link-button" to={`/classes/${item.id}`} onClick={enter}>
          班級頁
        </Link>
      </div>
    </li>
  );
}

/**
 * 工作檯 (Phase 1.4): class-centred. Each class is a card with its next step; the design library
 * (備課庫) is one clear entry below. A teacher with no class yet is invited to create the first
 * one, prefilled from the preference survey (P6); the invitation can be skipped.
 */
export function HomePage() {
  const { teacher, preferences } = useApp();
  const navigate = useNavigate();
  const [classes, setClasses] = useState<SchoolClass[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [unitCount, setUnitCount] = useState<number | null>(null);
  const [adding, setAdding] = useState(false);
  const [skipped, setSkipped] = useState(loadSkipped);
  const [archiveError, setArchiveError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .listClasses(true)
      .then((list) => alive && setClasses(list))
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    api
      .listUnits()
      .then((units) => alive && setUnitCount(units.length))
      // Only the count on the library entry; the entry works without it.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  const active = (classes ?? []).filter((c) => c.archived_at === null);
  const archived = (classes ?? []).filter((c) => c.archived_at !== null);
  const isFirst = classes !== null && classes.length === 0;
  const p6 = (preferences?.answers?.class_profile ?? "").trim();
  const track = teacher?.school_track ?? "general";

  function skip() {
    setSkipped(true);
    try {
      window.sessionStorage.setItem(FIRST_CLASS_SKIPPED_KEY, "1");
    } catch {
      // only remembered for this page view
    }
  }

  async function create(body: CreateClassRequest): Promise<string | null> {
    // First class: an untouched prefill is left to the server, which copies P6 and records that.
    if (isFirst && body.class_profile === p6) delete body.class_profile;
    try {
      const created = await api.createClass(body);
      navigate(`/classes/${created.id}`);
      return null;
    } catch (err) {
      return errorMessage(err);
    }
  }

  async function unarchive(id: number) {
    setArchiveError(null);
    try {
      const restored = await api.unarchiveClass(id);
      setClasses((prev) => (prev ?? []).map((c) => (c.id === id ? restored : c)));
    } catch (err) {
      setArchiveError(errorMessage(err));
    }
  }

  const libraryEntry = (
    <section className="panel-card library-entry" aria-labelledby="library-entry-heading">
      <div>
        <h3 id="library-entry-heading">備課庫</h3>
        <p>
          你已經準備好的範本都在這裡：每個單元設計包含單元前 CoRe、課例與教案。一份設計可以給好幾個班級用。
          {unitCount !== null && `目前有 ${unitCount} 個單元設計。`}
        </p>
      </div>
      <Link className="secondary-button link-button" to="/library">
        進入備課庫
      </Link>
    </section>
  );

  return (
    <section aria-labelledby="home-heading">
      <div className="welcome-banner">
        <p className="eyebrow warm">歡迎回來{teacher ? `，${teacher.research_id}` : ""}</p>
        <h2 id="home-heading">今天要上哪一班？</h2>
        <p>選一個班級，接著上次的進度；要準備新的單元，就到備課庫。每一步都可以略過，留白也不會被擋下。</p>
      </div>

      <section className="classes-panel" aria-labelledby="classes-heading">
        <div className="section-heading">
          <h3 id="classes-heading">我的班級</h3>
          <div className="section-heading-actions">
            {classes && <span className="status-pill">{active.length} 個班級</span>}
            {classes && active.length > 0 && !adding && (
              <button type="button" className="secondary-button" onClick={() => setAdding(true)}>
                新增班級
              </button>
            )}
          </div>
        </div>

        {loadError && <ErrorText message={loadError} />}
        {!classes && !loadError && <Loading />}

        {classes && active.length === 0 && !skipped && (
          <section className="panel-card first-class" aria-labelledby="first-class-heading">
            <div className="card-heading">
              <h4 id="first-class-heading">
                {isFirst ? "先建立你的第一個班級" : "建立一個班級"}
              </h4>
              <span className="optional-label">可略過</span>
            </div>
            <p className="first-class-lead">
              班級只需要一個名稱。之後每個班級會有自己的進度與上課紀錄，同一份單元設計可以給不同班級用。平台不存任何學生資料。
            </p>
            <ClassForm
              initial={emptyClassValues(track, isFirst ? p6 : "")}
              submitLabel="建立班級"
              onSubmit={create}
              onCancel={skip}
              cancelLabel="先略過"
              profileHint={isFirst && p6 ? "已帶入你在備課偏好寫的班級特性，可修改。" : undefined}
            />
          </section>
        )}
        {classes && active.length === 0 && skipped && (
          <div className="empty-preview first-class-skipped">
            <p>還沒有班級。可以直接到備課庫準備單元，之後再建立班級。</p>
            <button type="button" className="secondary-button" onClick={() => setSkipped(false)}>
              新增班級
            </button>
          </div>
        )}

        {adding && (
          <section className="panel-card first-class" aria-labelledby="new-class-heading">
            <h4 id="new-class-heading">新增班級</h4>
            <ClassForm
              initial={emptyClassValues(track)}
              submitLabel="建立班級"
              onSubmit={create}
              onCancel={() => setAdding(false)}
            />
          </section>
        )}

        {active.length > 0 && (
          <ul className="class-grid">
            {active.map((c) => (
              <ClassCard key={c.id} item={c} />
            ))}
          </ul>
        )}
      </section>

      {libraryEntry}

      {archived.length > 0 && (
        <details className="archived-classes">
          <summary>已封存的班級（{archived.length}）</summary>
          <ul className="archived-list">
            {archived.map((c) => (
              <li key={c.id}>
                <Link to={`/classes/${c.id}`}>{c.name}</Link>
                <span className="unit-meta">封存於 {formatDate(c.archived_at)}</span>
                <button type="button" className="ghost-button small-button" onClick={() => void unarchive(c.id)}>
                  取消封存
                </button>
              </li>
            ))}
          </ul>
          {archiveError && <Notice tone="warn">{archiveError}</Notice>}
        </details>
      )}
    </section>
  );
}
