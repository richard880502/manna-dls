import { useEffect, useState } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../api/client";
import type { PrepSessionSummary } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { KIND_LABEL, sessionTitle } from "../lib/prep";
import { formatDateTime } from "../lib/worksheet";

/** 我的備課: every prep conversation, newest first; and the way to all worksheets. */
export function PrepListPage() {
  const [sessions, setSessions] = useState<PrepSessionSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .listPrepSessions()
      .then((res) => alive && setSessions(res.sessions))
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, []);

  return (
    <>
      <div className="head-row page-head">
        <div>
          <h1>我的備課</h1>
          <p className="lead">每一次備課對話都留在這裡；做出來的學習單也在「所有學習單」。</p>
        </div>
        <span className="actions">
          <Link className="text-link is-plain" to="/worksheets">
            所有學習單
          </Link>
          <Link className="text-link" to="/">
            開始新的備課
          </Link>
        </span>
      </div>

      <ErrorText message={error} />
      {sessions === null && !error && <Loading />}
      {sessions !== null && sessions.length === 0 && (
        <p className="empty-line">
          還沒有備課對話。到<Link to="/">備課</Link>放進一篇課文，就會出現在這裡。
        </p>
      )}
      {sessions !== null && sessions.length > 0 && (
        <ul className="rows" aria-label="備課對話">
          {sessions.map((s) => (
            <li key={s.id} className="row">
              <div>
                <h3>
                  <Link to={`/prep/${s.id}`}>{sessionTitle(s)}</Link>
                </h3>
                <p className="meta">
                  {[
                    s.material ? `教材：${s.material.title.trim() || "未命名教材"}` : s.topic ? `主題：${s.topic}` : null,
                    s.class ? s.class.name : null,
                    s.last_kind ? KIND_LABEL[s.last_kind] : null,
                    formatDateTime(s.updated_at),
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </p>
              </div>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
