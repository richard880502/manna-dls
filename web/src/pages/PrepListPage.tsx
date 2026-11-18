import { useEffect, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { PrepKind, PrepSessionSummary, WorksheetSummary } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { TOOL_CHIP } from "../lib/create";
import { NEXT_LABEL, sessionTitle } from "../lib/prep";
import { LEVEL_LABEL, formatDateTime } from "../lib/worksheet";

/** The filter chips. 學習單 covers every worksheet-making kind and the standalone worksheets. */
type Filter = "all" | "worksheet" | "activity" | "ask";
const FILTERS: { key: Filter; label: string }[] = [
  { key: "all", label: "全部" },
  { key: "worksheet", label: "學習單" },
  { key: "activity", label: "課堂活動" },
  { key: "ask", label: "問 Manna" },
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

/** The tag on a session row, in the tools' own names. A revise turn does not say what it revised: 學習單. */
const KIND_TAG: Record<PrepKind, string> = {
  ...TOOL_CHIP,
  assessment: NEXT_LABEL.assessment,
  resources: NEXT_LABEL.resources,
  ask: "問 Manna",
  revise: "學習單",
};

type Item =
  | { type: "session"; key: string; at: string; filter: Filter | null; session: PrepSessionSummary }
  | { type: "worksheet"; key: string; at: string; filter: Filter | null; worksheet: WorksheetSummary };

const time = (iso: string) => {
  const t = Date.parse(iso);
  return Number.isNaN(t) ? 0 : t;
};

/** 我的作品: every prep session and every worksheet made outside one, newest first, in one list. */
export function PrepListPage() {
  const [params, setParams] = useSearchParams();
  const [sessions, setSessions] = useState<PrepSessionSummary[] | null>(null);
  const [worksheets, setWorksheets] = useState<WorksheetSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<number | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const typeParam = params.get("type");
  const filter: Filter = isFilter(typeParam) ? typeParam : "all";

  useEffect(() => {
    let alive = true;
    Promise.all([api.listPrepSessions(), api.listWorksheets()])
      .then(([s, w]) => {
        if (!alive) return;
        setSessions(s.sessions);
        setWorksheets(w);
      })
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, []);

  const items = useMemo<Item[] | null>(() => {
    if (sessions === null || worksheets === null) return null;
    const all: Item[] = [
      ...sessions.map(
        (s): Item => ({ type: "session", key: `s${s.id}`, at: s.updated_at, filter: filterOfKind(s.last_kind), session: s }),
      ),
      // A worksheet made in a prep session is that session's row, not a row of its own.
      ...worksheets
        .filter((w) => w.prep_session_id == null)
        .map((w): Item => ({ type: "worksheet", key: `w${w.id}`, at: w.updated_at, filter: "worksheet", worksheet: w })),
    ];
    return all.sort((a, b) => time(b.at) - time(a.at));
  }, [sessions, worksheets]);

  const count = (f: Filter) => (items ? items.filter((i) => f === "all" || i.filter === f).length : 0);
  const shown = items?.filter((i) => filter === "all" || i.filter === filter) ?? [];

  function choose(f: Filter) {
    setParams(f === "all" ? {} : { type: f });
  }

  async function remove(id: number) {
    setDeleteError(null);
    try {
      await api.deleteWorksheet(id);
      setWorksheets((rows) => rows?.filter((r) => r.id !== id) ?? null);
      setConfirming(null);
    } catch (err) {
      setDeleteError(errorMessage(err));
    }
  }

  return (
    <>
      <div className="head-row page-head">
        <div>
          <h1>我的作品</h1>
          <p className="lead">做過的備課、學習單和課堂活動都在這裡，最近用過的在最上面。</p>
        </div>
        <Link className="primary-button link-button" to="/">
          新的備課
        </Link>
      </div>

      <ErrorText message={error} />
      {items === null && !error && <Loading />}
      {items !== null && items.length === 0 && (
        <p className="empty-line">
          還沒有作品。到<Link to="/">首頁</Link>放進一篇課文，就會出現在這裡。
        </p>
      )}
      {items !== null && items.length > 0 && (
        <>
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
          {shown.length === 0 && <p className="empty-line">這個類型還沒有作品。</p>}
          {shown.length > 0 && (
            <ul className="rows" aria-label="我的作品">
              {shown.map((item) =>
                item.type === "session" ? (
                  <SessionRow key={item.key} s={item.session} />
                ) : (
                  <WorksheetRow
                    key={item.key}
                    w={item.worksheet}
                    confirming={confirming === item.worksheet.id}
                    onAsk={() => setConfirming(item.worksheet.id)}
                    onCancel={() => setConfirming(null)}
                    onConfirm={() => void remove(item.worksheet.id)}
                  />
                ),
              )}
            </ul>
          )}
        </>
      )}
      <ErrorText message={deleteError} />
    </>
  );
}

function SessionRow({ s }: { s: PrepSessionSummary }) {
  return (
    <li className="row">
      <div>
        <h3>
          <Link to={`/prep/${s.id}`}>{sessionTitle(s)}</Link>
        </h3>
        <p className="meta">
          {s.last_kind && <span className="mywork-tag">{KIND_TAG[s.last_kind]}</span>}
          {[
            s.material ? `教材：${s.material.title.trim() || "未命名教材"}` : s.topic ? `主題：${s.topic}` : null,
            s.class ? s.class.name : null,
            formatDateTime(s.updated_at),
          ]
            .filter(Boolean)
            .join(" · ")}
        </p>
      </div>
    </li>
  );
}

function WorksheetRow({
  w,
  confirming,
  onAsk,
  onCancel,
  onConfirm,
}: {
  w: WorksheetSummary;
  confirming: boolean;
  onAsk: () => void;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const name = w.title || "未命名的學習單";
  return (
    <li className="row">
      <div>
        <h3>
          <Link to={`/worksheets/${w.id}`}>{name}</Link>
        </h3>
        <p className="meta">
          <span className="mywork-tag">學習單</span>
          {[
            w.lesson ? `${w.lesson.unit_title} / ${w.lesson.title}` : null,
            w.generated ? (w.levels.length > 0 ? w.levels.map((l) => LEVEL_LABEL[l]).join("、") : "已產生") : "還沒產生",
            formatDateTime(w.updated_at),
          ]
            .filter(Boolean)
            .join(" · ")}
        </p>
      </div>
      <div className="row-tools">
        {confirming ? (
          <span role="group" aria-label="確認刪除">
            確定刪除？
            <button type="button" className="text-link is-danger" onClick={onConfirm}>
              確定刪除
            </button>
            <button type="button" className="text-link is-plain" onClick={onCancel}>
              不刪
            </button>
          </span>
        ) : (
          <button type="button" className="text-link is-plain" aria-label={`刪除 ${name}`} onClick={onAsk}>
            刪除
          </button>
        )}
      </div>
    </li>
  );
}
