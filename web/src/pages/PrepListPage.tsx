import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Link, useSearchParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { PrepKind, PrepSessionSummary, StepwiseLesson, WorksheetSummary } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { WorkCard, WorkCardSkeleton, type WorkCardData } from "../components/mywork/WorkCard";
import { STEPWISE_TAG, TOOL_NAME } from "../lib/create";
import { stepwiseProgress } from "../lib/sop";
import { DELETE_CONFIRM, NEXT_LABEL, PREP_SESSIONS_CHANGED, sessionTitle } from "../lib/prep";
import { LEVEL_LABEL, formatDateTime } from "../lib/worksheet";
import { useDocumentTitle } from "../lib/useDocumentTitle";
import { MetaList } from "../components/MetaList";

/** The filter chips. 學習單 covers every worksheet-making kind and the standalone worksheets. */
type Filter = "all" | "worksheet" | "activity" | "ask" | "stepwise";
const FILTERS: { key: Filter; label: string }[] = [
  { key: "all", label: "全部" },
  { key: "worksheet", label: "學習單" },
  { key: "activity", label: "課堂活動" },
  { key: "ask", label: "問 Manna" },
  { key: "stepwise", label: STEPWISE_TAG },
];
const isFilter = (v: string | null): v is Filter => FILTERS.some((f) => f.key === v);

const WORKSHEET_KINDS: readonly PrepKind[] = ["worksheet", "comprehension", "tiered", "kit", "assessment", "revise"];

/** Which filter a session's last step falls under (null: only under 全部). */
const filterOfKind = (kind: PrepKind | null): Filter | null => {
  if (kind === null) return null;
  if (WORKSHEET_KINDS.includes(kind)) return "worksheet";
  if (kind === "activity") return "activity";
  if (kind === "ask") return "ask";
  return null;
};

/**
 * The tag on a row, in the tools' visible names (TOOL_NAME; TOOL_CHIP is wire-only). A revise turn
 * does not say what it revised: 學習單.
 */
const KIND_TAG: Record<PrepKind, string> = {
  comprehension: TOOL_NAME.comprehension,
  tiered: TOOL_NAME.tiered,
  activity: TOOL_NAME.activity,
  kit: TOOL_NAME.kit,
  worksheet: TOOL_NAME.worksheet,
  assessment: NEXT_LABEL.assessment,
  resources: NEXT_LABEL.resources,
  ask: "問 Manna",
  revise: "學習單",
  chat: "問 Manna",
};

/** 卡片 (the default) or 清單; the choice is kept on this browser. */
type View = "cards" | "list";
const VIEW_KEY = "manna.mywork.view";
const VIEWS: { key: View; label: string }[] = [
  { key: "cards", label: "卡片" },
  { key: "list", label: "清單" },
];

function loadView(): View {
  try {
    return localStorage.getItem(VIEW_KEY) === "list" ? "list" : "cards";
  } catch {
    return "cards";
  }
}

function saveView(view: View) {
  try {
    localStorage.setItem(VIEW_KEY, view);
  } catch {
    // Private windows and blocked storage: the choice just is not kept.
  }
}

type Item =
  | { type: "session"; key: string; at: string; filter: Filter | null; session: PrepSessionSummary }
  | { type: "worksheet"; key: string; at: string; filter: Filter | null; worksheet: WorksheetSummary }
  | { type: "lesson"; key: string; at: string; filter: Filter | null; lesson: StepwiseLesson };

/**
 * What tells a session from another of the same name, besides its type and date: the material
 * (only when it is not just the title again), or the topic when there is no material, and the
 * class.
 */
function sessionDetail(s: PrepSessionSummary): string[] {
  const title = sessionTitle(s);
  const material = s.material ? s.material.title.trim() || "未命名教材" : null;
  let about: string | null = null;
  if (material !== null) {
    if (material !== title) about = `教材：${material}`;
  } else if (s.topic && s.topic.trim() !== title) {
    about = `主題：${s.topic}`;
  }
  return [about, s.class ? s.class.name : null].filter((p): p is string => Boolean(p));
}

function worksheetDetail(w: WorksheetSummary): string[] {
  return [
    w.lesson ? `${w.lesson.unit_title} / ${w.lesson.title}` : null,
    w.generated ? (w.levels.length > 0 ? w.levels.map((l) => LEVEL_LABEL[l]).join("、") : "已產生") : "還沒產生",
  ].filter((p): p is string => Boolean(p));
}

const lessonDetail = (l: StepwiseLesson): string[] => [l.unit_title, stepwiseProgress(l.next_step)];

const worksheetName = (w: WorksheetSummary) => w.title || "未命名的學習單";

/** What a card shows of an item. */
function cardOf(item: Item): WorkCardData {
  if (item.type === "session") {
    const s = item.session;
    return {
      title: sessionTitle(s),
      href: `/prep/${s.id}`,
      tag: s.last_kind ? KIND_TAG[s.last_kind] : null,
      detail: sessionDetail(s).join("，"),
      when: formatDateTime(s.updated_at),
      worksheet: s.preview_worksheet ?? null,
      text: s.preview_text ?? null,
    };
  }
  if (item.type === "lesson") {
    const l = item.lesson;
    return {
      title: l.title,
      href: `/lessons/${l.id}`,
      tag: STEPWISE_TAG,
      detail: lessonDetail(l).join("，"),
      when: formatDateTime(l.updated_at),
      worksheet: null,
      // No page to show: the preview says where the lesson stands.
      text: stepwiseProgress(l.next_step),
    };
  }
  const w = item.worksheet;
  return {
    title: worksheetName(w),
    href: `/worksheets/${w.id}`,
    tag: TOOL_NAME.worksheet,
    detail: worksheetDetail(w).join("，"),
    when: formatDateTime(w.updated_at),
    // Only a generated worksheet has a first page.
    worksheet: w.generated ? { id: w.id, version_no: w.version_no } : null,
    text: null,
  };
}

const time = (iso: string) => {
  const t = Date.parse(iso);
  return Number.isNaN(t) ? 0 : t;
};

/**
 * 我的作品: every prep session and every worksheet made outside one, newest first, as cards or a
 * list. Every item has 刪除; a session's takes its worksheets with it.
 */
export function PrepListPage() {
  useDocumentTitle("我的作品");
  const [params, setParams] = useSearchParams();
  const [sessions, setSessions] = useState<PrepSessionSummary[] | null>(null);
  const [worksheets, setWorksheets] = useState<WorksheetSummary[] | null>(null);
  const [lessons, setLessons] = useState<StepwiseLesson[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Bumped by 再試一次: the lists are asked for again. */
  const [attempt, setAttempt] = useState(0);
  /** The key of the item whose 刪除 waits for 確定刪除. */
  const [confirming, setConfirming] = useState<string | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [view, setView] = useState<View>(loadView);
  /** After a delete the card (and the focused button) is gone: focus goes to the list. */
  const listRef = useRef<HTMLUListElement>(null);

  const typeParam = params.get("type");
  const filter: Filter = isFilter(typeParam) ? typeParam : "all";

  useEffect(() => {
    let alive = true;
    Promise.all([api.listPrepSessions(), api.listWorksheets(), api.listStepwiseLessons()])
      .then(([s, w, l]) => {
        if (!alive) return;
        setSessions(s.sessions);
        setWorksheets(w);
        setLessons(l);
      })
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [attempt]);

  const items = useMemo<Item[] | null>(() => {
    if (sessions === null || worksheets === null || lessons === null) return null;
    const all: Item[] = [
      ...sessions.map(
        (s): Item => ({ type: "session", key: `s${s.id}`, at: s.updated_at, filter: filterOfKind(s.last_kind), session: s }),
      ),
      // A worksheet made in a prep session is that session's row, not a row of its own.
      ...worksheets
        .filter((w) => w.prep_session_id == null)
        .map((w): Item => ({ type: "worksheet", key: `w${w.id}`, at: w.updated_at, filter: "worksheet", worksheet: w })),
      ...lessons.map(
        (l): Item => ({ type: "lesson", key: `l${l.id}`, at: l.updated_at, filter: "stepwise", lesson: l }),
      ),
    ];
    return all.sort((a, b) => time(b.at) - time(a.at));
  }, [sessions, worksheets, lessons]);

  const count = (f: Filter) => (items ? items.filter((i) => f === "all" || i.filter === f).length : 0);
  const shown = items?.filter((i) => filter === "all" || i.filter === filter) ?? [];

  function retry() {
    setError(null);
    setAttempt((n) => n + 1);
  }

  function choose(f: Filter) {
    setParams(f === "all" ? {} : { type: f });
  }

  function chooseView(v: View) {
    setView(v);
    saveView(v);
  }

  /** A session goes with every worksheet made in it; a worksheet of its own goes alone. */
  async function remove(item: Exclude<Item, { type: "lesson" }>) {
    setDeleteError(null);
    try {
      if (item.type === "session") {
        const id = item.session.id;
        await api.deletePrepSession(id);
        setSessions((rows) => rows?.filter((r) => r.id !== id) ?? null);
        setWorksheets((rows) => rows?.filter((r) => r.prep_session_id !== id) ?? null);
        window.dispatchEvent(new Event(PREP_SESSIONS_CHANGED));
      } else {
        const id = item.worksheet.id;
        await api.deleteWorksheet(id);
        setWorksheets((rows) => rows?.filter((r) => r.id !== id) ?? null);
      }
      setConfirming(null);
      listRef.current?.focus();
    } catch (err) {
      setDeleteError(errorMessage(err));
    }
  }

  /** The delete controls of an item: the same in both views (none for a 逐步設計 lesson). */
  function deleteControl(item: Item) {
    // Lessons have no delete in the API: their cards carry no 刪除.
    if (item.type === "lesson") return undefined;
    return (
      <DeleteControl
        name={item.type === "session" ? sessionTitle(item.session) : worksheetName(item.worksheet)}
        question={item.type === "session" ? DELETE_CONFIRM : "確定刪除？"}
        confirming={confirming === item.key}
        error={confirming === item.key ? deleteError : null}
        onAsk={() => {
          setDeleteError(null);
          setConfirming(item.key);
        }}
        onCancel={() => {
          setDeleteError(null);
          setConfirming(null);
        }}
        onConfirm={() => void remove(item)}
      />
    );
  }

  return (
    <>
      <div className="head-row page-head">
        <div>
          <h1>我的作品</h1>
          <p className="lead">做過的備課、學習單、課堂活動和逐步設計的課例都在這裡，最近用過的在最上面。</p>
        </div>
        <Link className="primary-button link-button" to="/">
          新的備課
        </Link>
      </div>

      <ErrorText message={error} />
      {error && (
        <p>
          <button type="button" className="text-link" onClick={retry}>
            再試一次
          </button>
        </p>
      )}
      {items === null && !error && view === "list" && <Loading />}
      {items === null && !error && view === "cards" && (
        <>
          <p className="sr-only" role="status">
            載入中…
          </p>
          <ul className="work-grid" aria-hidden="true">
            {Array.from({ length: 8 }, (_, i) => (
              <WorkCardSkeleton key={i} />
            ))}
          </ul>
        </>
      )}
      {items !== null && items.length === 0 && (
        <p className="empty-line">
          還沒有作品。到<Link to="/">首頁</Link>放進一篇課文，就會出現在這裡。
        </p>
      )}
      {items !== null && items.length > 0 && (
        <>
          <div className="mywork-bar">
            <div className="lib-chips" role="group" aria-label="作品類型">
              {FILTERS.map((f) => (
                <button
                  key={f.key}
                  type="button"
                  className="lib-chip"
                  aria-pressed={filter === f.key}
                  onClick={() => choose(f.key)}
                >
                  {f.label} <span className="lib-chip-count">{count(f.key)}</span>
                </button>
              ))}
            </div>
            <div className="view-switch" role="group" aria-label="檢視方式">
              {VIEWS.map((v) => (
                <button key={v.key} type="button" aria-pressed={view === v.key} onClick={() => chooseView(v.key)}>
                  {v.label}
                </button>
              ))}
            </div>
          </div>
          {shown.length === 0 && (
            <p className="empty-line mywork-empty">
              這個類型還沒有作品。
              <button type="button" className="text-link" onClick={() => choose("all")}>
                看全部
              </button>
            </p>
          )}
          {shown.length > 0 && <h2 className="sr-only">作品清單</h2>}
          {shown.length > 0 && view === "cards" && (
            <ul className="work-grid" aria-label="我的作品" ref={listRef} tabIndex={-1}>
              {shown.map((item) => (
                <WorkCard key={item.key} data={cardOf(item)} tools={deleteControl(item)} />
              ))}
            </ul>
          )}
          {shown.length > 0 && view === "list" && (
            <ul className="rows mywork-rows" aria-label="我的作品" ref={listRef} tabIndex={-1}>
              {shown.map((item) =>
                item.type === "session" ? (
                  <SessionRow key={item.key} s={item.session} tools={deleteControl(item)} />
                ) : item.type === "lesson" ? (
                  <LessonRow key={item.key} l={item.lesson} />
                ) : (
                  <WorksheetRow key={item.key} w={item.worksheet} tools={deleteControl(item)} />
                ),
              )}
            </ul>
          )}
        </>
      )}
    </>
  );
}

function SessionRow({ s, tools }: { s: PrepSessionSummary; tools: ReactNode }) {
  return (
    <li className="row">
      <div>
        <h3>
          <Link to={`/prep/${s.id}`}>{sessionTitle(s)}</Link>
        </h3>
        <MetaList
          className="meta"
          items={[
            s.last_kind && <span key="tag" className="mywork-tag">{KIND_TAG[s.last_kind]}</span>,
            ...sessionDetail(s),
            formatDateTime(s.updated_at),
          ]}
        />
      </div>
      <div className="row-tools">{tools}</div>
    </li>
  );
}

function LessonRow({ l }: { l: StepwiseLesson }) {
  return (
    <li className="row">
      <div>
        <h3>
          <Link to={`/lessons/${l.id}`}>{l.title}</Link>
        </h3>
        <MetaList
          className="meta"
          items={[<span key="tag" className="mywork-tag">{STEPWISE_TAG}</span>, ...lessonDetail(l), formatDateTime(l.updated_at)]}
        />
      </div>
    </li>
  );
}

function WorksheetRow({ w, tools }: { w: WorksheetSummary; tools: ReactNode }) {
  return (
    <li className="row">
      <div>
        <h3>
          <Link to={`/worksheets/${w.id}`}>{worksheetName(w)}</Link>
        </h3>
        <MetaList
          className="meta"
          items={[<span key="tag" className="mywork-tag">{TOOL_NAME.worksheet}</span>, ...worksheetDetail(w), formatDateTime(w.updated_at)]}
        />
      </div>
      <div className="row-tools">{tools}</div>
    </li>
  );
}

/** 刪除 with its confirmation, in place; a failed delete is said right beside it. */
function DeleteControl({
  name,
  question,
  confirming,
  error,
  onAsk,
  onCancel,
  onConfirm,
}: {
  name: string;
  /** Said before 確定刪除: what goes with it. */
  question: string;
  confirming: boolean;
  /** A failed delete. */
  error: string | null;
  onAsk: () => void;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  // The confirmation replaces 刪除 in place: focus goes to the safe 不刪, and back to 刪除 after.
  const keepRef = useRef<HTMLButtonElement>(null);
  const askRef = useRef<HTMLButtonElement>(null);
  /** Set by 不刪: only then does focus go back to 刪除 (another card's 刪除 keeps its own focus). */
  const backToAsk = useRef(false);
  useEffect(() => {
    if (confirming) keepRef.current?.focus();
    else if (backToAsk.current) {
      backToAsk.current = false;
      askRef.current?.focus();
    }
  }, [confirming]);
  return (
    <>
      {confirming ? (
        <span role="group" aria-label="確認刪除">
          {question}
          <button type="button" className="text-link is-danger" onClick={onConfirm}>
            確定刪除
          </button>
          <button
            ref={keepRef}
            type="button"
            className="text-link is-plain"
            onClick={() => {
              backToAsk.current = true;
              onCancel();
            }}
          >
            不刪
          </button>
        </span>
      ) : (
        <button ref={askRef} type="button" className="text-link is-plain" aria-label={`刪除 ${name}`} onClick={onAsk}>
          刪除
        </button>
      )}
      <ErrorText message={error} />
    </>
  );
}
