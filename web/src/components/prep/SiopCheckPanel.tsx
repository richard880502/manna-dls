import { useEffect, useId, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { PrepTurn, SiopCheck, SiopCheckItem } from "../../api/types";
import { RATING_LABEL, SIOP_WORDS, checkGroups, checkIsStale, prepConfig } from "../../lib/prep";
import { ErrorText } from "../common";
import { QueueLine } from "../RunProgress";

/** The newer of two looks at a turn's check: a later run, or the same run once it finished. */
function newer(a: SiopCheck | null, b: SiopCheck | null): SiopCheck | null {
  if (!a || !b) return a ?? b;
  if (a.id !== b.id) return a.id > b.id ? a : b;
  return a.status !== "running" ? a : b;
}

function ItemRow({ item }: { item: SiopCheckItem }) {
  return (
    <li className="siop-item">
      <p className="siop-item-head">
        <span className="siop-item-id">{item.id.replace("-", "–")}</span>
        <span className="siop-item-name">{item.name}</span>
        {item.rating ? (
          <span className={`siop-rating is-${item.rating}`}>{RATING_LABEL[item.rating]}</span>
        ) : (
          <span className="siop-rating is-none">{SIOP_WORDS.notJudged}</span>
        )}
      </p>
      {item.scope === "partial" && (
        <p className="siop-item-note">
          {SIOP_WORDS.partial}：{item.note}
        </p>
      )}
      {item.reason.trim() && <p className="siop-item-reason">{item.reason}</p>}
    </li>
  );
}

/**
 * 「SIOP 自我檢核」 under a /create result (docs/SIOP_DESIGN_RULES.md section 八): what Manna saw on
 * the page, item by item, as 符合／部分符合／未見 with one sentence each, collapsed until the teacher
 * opens it; the items that need a classroom are named as 需課堂觀察, the ones judged on their
 * visible part say so. No total score: it is advice for the teacher's judgment.
 *
 * The check runs in the background after the output is made; while it runs the panel asks for it
 * again every couple of seconds. A check that failed, or one made before the worksheet was
 * revised, can be run again. Nothing is shown for a turn kind without a check, a turn that is not
 * done, or a server that does not have the check (`siop_check` absent).
 */
export function SiopCheckPanel({
  sessionId,
  turn,
  quiet = false,
}: {
  sessionId: number;
  turn: PrepTurn;
  /** The turn's 「進度」 panel lists the 自我檢核 step: no working line of our own while it runs. */
  quiet?: boolean;
}) {
  const ids = useId();
  const [local, setLocal] = useState<SiopCheck | null>(null);
  /** Whether the teacher opened the results: a new run (重新檢核) shows open again when it is done. */
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const check = newer(turn.siop_check ?? null, local);
  const running = check?.status === "running";

  useEffect(() => {
    if (!running) return undefined;
    let alive = true;
    const timer = setTimeout(() => {
      api
        .getSiopCheck(sessionId, turn.id)
        .then((c) => alive && setLocal(c))
        .catch((err: unknown) => alive && setError(errorMessage(err)));
    }, prepConfig.pollMs);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [running, check, sessionId, turn.id]);

  if (turn.siop_check === undefined || turn.status !== "done") return null;

  /** After a queued check was cancelled: ask for it at once (it is closed as failed). */
  async function refresh() {
    try {
      setLocal(await api.getSiopCheck(sessionId, turn.id));
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  async function rerun() {
    setBusy(true);
    setError(null);
    try {
      setLocal(await api.rerunSiopCheck(sessionId, turn.id));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  const button = (label: string) => (
    <button type="button" className="secondary-button small-button" disabled={busy || running} onClick={() => void rerun()}>
      {label}
    </button>
  );

  if (quiet && (!check || check.status === "running")) return null;

  let body;
  if (!check) {
    body = (
      <div className="siop-check-state">
        <p className="quiet-note">{SIOP_WORDS.none}</p>
        {button(SIOP_WORDS.start)}
      </div>
    );
  } else if (check.status === "running") {
    body = check.queue ? (
      <QueueLine queue={check.queue} onCancelled={() => void refresh()} />
    ) : (
      <p className="manna-label prep-working" role="status">
        <span className="star is-working" aria-hidden="true" />
        {SIOP_WORDS.running}
      </p>
    );
  } else if (check.status === "failed") {
    body = (
      <div className="siop-check-state" role="alert">
        <p>{SIOP_WORDS.failed}</p>
        {check.error && <p className="quiet-note">{check.error}</p>}
        {button(SIOP_WORDS.retry)}
      </div>
    );
  } else {
    const { judged, classroom } = checkGroups(check);
    const stale = checkIsStale(check, turn.worksheet?.version_no);
    body = (
      <details className="siop-details" open={open} onToggle={(e) => setOpen(e.currentTarget.open)}>
        <summary>{SIOP_WORDS.title}</summary>
        <p className="siop-advice">{SIOP_WORDS.advice}</p>
        {stale && check.worksheet_version_no !== null && (
          <div className="siop-check-state">
            <p className="quiet-note">{SIOP_WORDS.stale(check.worksheet_version_no)}</p>
            {button(SIOP_WORDS.recheck)}
          </div>
        )}
        <ul className="siop-items" aria-labelledby={`${ids}-title`}>
          {judged.map((item) => (
            <ItemRow key={item.id} item={item} />
          ))}
        </ul>
        {classroom.length > 0 && (
          <div className="siop-classroom">
            <p className="quiet-note">{SIOP_WORDS.classroomLine}</p>
            <ul>
              {classroom.map((item) => (
                <li key={item.id} title={item.note}>
                  <span className="siop-item-id">{item.id}</span>
                  {item.name}
                  <span className="siop-rating is-classroom">{SIOP_WORDS.classroom}</span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </details>
    );
  }

  return (
    <section className="siop-check" aria-labelledby={`${ids}-title`}>
      <h4 className="sr-only" id={`${ids}-title`}>
        {SIOP_WORDS.title}
      </h4>
      {check?.status !== "done" && (
        <p className="siop-check-title" aria-hidden="true">
          {SIOP_WORDS.title}
        </p>
      )}
      {body}
      <ErrorText message={error} />
    </section>
  );
}
