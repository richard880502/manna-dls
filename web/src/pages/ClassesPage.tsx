import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { CreateClassRequest, LessonRun, SchoolClass } from "../api/types";
import { ClassForm, emptyClassValues } from "../components/classes/ClassForm";
import { NextLesson, useStartInClass } from "../components/classes/NextLesson";
import { ErrorText, Loading, Notice } from "../components/common";
import { Dialog } from "../components/Dialog";
import { useApp } from "../context/AppContext";
import { classSubtitle, focalClass, nextLine } from "../lib/classes";
import { formatDate, formatShortDate } from "../lib/labels";

const FIRST_CLASS_SKIPPED_KEY = "manna.firstClassSkipped";
/** 「上完還沒補寫」 shows the latest few; the rest are on the records page. */
const UNREVIEWED_SHOWN = 5;

function loadSkipped(): boolean {
  try {
    return window.sessionStorage.getItem(FIRST_CLASS_SKIPPED_KEY) === "1";
  } catch {
    return false;
  }
}

/** One class as a ruled row: its name, a quiet meta line, and what is next for it. */
function ClassRow({ item, isFocal }: { item: SchoolClass; isFocal: boolean }) {
  const { setCurrentClassId } = useApp();
  const { start, starting, error } = useStartInClass(item.id);
  const next = item.next_action;
  // Following a class's next step means working in that class.
  const enter = () => setCurrentClassId(item.id);
  const meta = [
    classSubtitle(item, true),
    item.unit_count === 0 ? "還沒有單元" : `${item.unit_count} 個單元`,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <li className="row">
      <div>
        <h3>
          <Link to={`/classes/${item.id}`} onClick={enter}>
            {item.name}
          </Link>
        </h3>
        <p className="meta">{meta}</p>
      </div>
      <p className="side">
        {next?.kind === "add_unit" ? (
          <Link className="text-link" to={`/classes/${item.id}`} onClick={enter}>
            從備課庫加入單元
          </Link>
        ) : (
          <>
            <span>{nextLine(next)}</span>
            {/* The class the page leads with already has its action in the block above. */}
            {next?.kind === "start_lesson" && !isFocal && (
              <button
                type="button"
                className="text-link"
                disabled={starting}
                aria-label={`${item.name}：開始上課`}
                onClick={() => void start(next.lesson_id)}
              >
                {starting ? "開始中…" : "開始上課"}
              </button>
            )}
            {next?.kind === "resume_run" && !isFocal && (
              <Link className="text-link" to={`/runs/${next.run_id}`} onClick={enter}>
                回到課堂
              </Link>
            )}
            {next?.kind === "plan_unit" && !isFocal && (
              <Link className="text-link" to={`/units/${next.unit_id}`} onClick={enter}>
                去備課
              </Link>
            )}
          </>
        )}
      </p>
      {error && (
        <div className="row-more">
          <ErrorText message={error} />
        </div>
      )}
    </li>
  );
}

/**
 * 班級 (`/classes`; the home page until 備課 took `/`, docs/DESIGN_OVERHAUL.md): one focal block, the next lesson of the class
 * taught most recently, with 「開始上課」; then the classes as ruled rows; then the lessons that
 * were taught but not written up yet. A teacher with no class is invited to create the first one,
 * prefilled from the preference survey (P6); the invitation can be skipped.
 */
export function ClassesPage() {
  const { teacher, preferences } = useApp();
  const navigate = useNavigate();
  const [classes, setClasses] = useState<SchoolClass[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [runs, setRuns] = useState<LessonRun[]>([]);
  const [adding, setAdding] = useState(false);
  const [skipped, setSkipped] = useState(loadSkipped);
  const [archiveError, setArchiveError] = useState<string | null>(null);
  /** 新增班級 opens a dialog; focus goes back to the button when it closes. */
  const opener = useRef<HTMLElement | null>(null);
  const backToOpener = useCallback(() => opener.current, []);

  useEffect(() => {
    let alive = true;
    api
      .listClasses(true)
      .then((list) => alive && setClasses(list))
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    api
      .listRuns()
      .then((list) => alive && setRuns(list))
      // Only which class leads the page and the 「還沒補寫」 list; the page works without them.
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

  const focal = focalClass(active, runs);
  const focalNext = focal?.next_action ?? null;
  const lastTaught = focal ? runs.find((r) => r.class_id === focal.id && r.ended_at !== null) : undefined;
  const unreviewed = runs.filter((r) => r.ended_at !== null && r.review_completed_at === null);

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

  return (
    <>
      {focal && focalNext && focalNext.kind !== "add_unit" ? (
        <NextLesson
          classId={focal.id}
          kicker={`${focalNext.kind === "resume_run" ? "上課中" : "接下來"} · ${focal.name}`}
          next={focalNext}
          detail={
            focalNext.kind === "resume_run"
              ? "這節課還沒結束。"
              : lastTaught
                ? `上一節 ${formatShortDate(lastTaught.started_at)} 上完。`
                : "這班還沒上過課。"
          }
          headingId="home-heading"
        />
      ) : (
        <div className="page-head">
          <h1 id="home-heading">班級</h1>
          <p className="lead">每個班級有自己的進度與上課紀錄；一份單元可以給好幾個班用。</p>
        </div>
      )}

      <section className="section" aria-labelledby="classes-heading">
        <div className="head-row">
          <h2 id="classes-heading">我的班級</h2>
          <span className="actions">
            {/* Every unit, to reuse one in another class: reached from here, not from the top bar. */}
            <Link className="text-link is-plain" to="/units">
              所有單元
            </Link>
            {classes && active.length > 0 && (
              <button
                type="button"
                className="text-link"
                aria-haspopup="dialog"
                onClick={(event) => {
                  opener.current = event.currentTarget;
                  setAdding(true);
                }}
              >
                新增班級
              </button>
            )}
          </span>
        </div>

        {loadError && <ErrorText message={loadError} />}
        {!classes && !loadError && <Loading />}

        {classes && active.length === 0 && !skipped && (
          <section className="inline-form-block" aria-labelledby="first-class-heading">
            <h3 id="first-class-heading">{isFirst ? "先建立你的第一個班級" : "建立一個班級"}</h3>
            <p className="lead">
              班級只需要一個名稱。之後每個班級會有自己的進度與上課紀錄，同一份單元可以給不同班級用。平台不存任何學生資料。
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
          <p className="empty-line">
            還沒有班級。可以先到<Link to="/units">備課庫</Link>準備單元，之後再
            <button type="button" className="text-link" onClick={() => setSkipped(false)}>
              新增班級
            </button>
            。
          </p>
        )}

        {adding && (
          <Dialog title="新增班級" returnFocus={backToOpener} onClose={() => setAdding(false)}>
            <ClassForm
              initial={emptyClassValues(track)}
              submitLabel="建立班級"
              onSubmit={create}
              onCancel={() => setAdding(false)}
            />
          </Dialog>
        )}

        {active.length > 0 && (
          <ul className="rows">
            {active.map((c) => (
              <ClassRow key={c.id} item={c} isFocal={c.id === focal?.id} />
            ))}
          </ul>
        )}
      </section>

      {unreviewed.length > 0 && (
        <section className="section" aria-labelledby="unreviewed-heading">
          <div className="head-row">
            <h2 id="unreviewed-heading">上完還沒補寫</h2>
            <Link className="text-link is-plain" to="/record">
              全部上課紀錄
            </Link>
          </div>
          <ul className="rows">
            {unreviewed.slice(0, UNREVIEWED_SHOWN).map((r) => (
              <li key={r.id} className="row">
                <div>
                  <h3>
                    <Link to={`/runs/${r.id}/review`}>
                      {r.class_name ? `${r.class_name}　` : ""}
                      {r.unit_title}　{r.lesson_title}
                    </Link>
                  </h3>
                  <p className="meta">
                    {formatShortDate(r.started_at)} 上課 · 順利 {r.good_count} 次 · 卡住 {r.stuck_count} 次
                  </p>
                </div>
                <p className="side">
                  <Link
                    className="text-link"
                    to={`/runs/${r.id}/review`}
                    aria-label={`補寫 ${r.class_name ?? ""} ${r.lesson_title}`}
                  >
                    補寫
                  </Link>
                </p>
              </li>
            ))}
          </ul>
        </section>
      )}

      {classes && unreviewed.length === 0 && runs.length > 0 && (
        <p className="page-foot">
          <Link className="text-link is-plain" to="/record">
            全部上課紀錄
          </Link>
        </p>
      )}

      {archived.length > 0 && (
        <details className="archived-classes">
          <summary>已封存的班級（{archived.length}）</summary>
          <ul className="rows">
            {archived.map((c) => (
              <li key={c.id} className="row">
                <div>
                  <h3>
                    <Link to={`/classes/${c.id}`}>{c.name}</Link>
                  </h3>
                  <p className="meta">封存於 {formatDate(c.archived_at)}</p>
                </div>
                <p className="side">
                  <button
                    type="button"
                    className="text-link is-plain"
                    aria-label={`取消封存 ${c.name}`}
                    onClick={() => void unarchive(c.id)}
                  >
                    取消封存
                  </button>
                </p>
              </li>
            ))}
          </ul>
          {archiveError && <Notice tone="warn">{archiveError}</Notice>}
        </details>
      )}
    </>
  );
}
