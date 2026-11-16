import { useEffect, useId, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { CreatePrepSessionRequest, PrepSessionSummary } from "../api/types";
import { ErrorText } from "../components/common";
import {
  MaterialChooser,
  emptyMaterialChoice,
  materialFields,
  type MaterialChoice,
} from "../components/material/MaterialChooser";
import { OncePerPage, useFirstOnPage } from "../components/OncePerPage";
import { ClassChip, TemplateChip, useActiveClasses } from "../components/prep/ContextChips";
import { useReadyTemplates } from "../components/template/TemplateSelect";
import { useApp } from "../context/AppContext";
import { UPLOAD_WARNING } from "../lib/materials";
import { ACTION_KINDS, ACTION_LABEL, ASK_PLACEHOLDER, KIND_LABEL, OTHER_LABEL, sessionTitle, type ActionKind } from "../lib/prep";
import { formatDateTime } from "../lib/worksheet";

type Tab = "material" | "ask";

const RECENT_SHOWN = 5;

/** The privacy reminder, said once on the page (the upload form would otherwise say it too). */
function PrivacyReminder() {
  const show = useFirstOnPage();
  if (!show) return null;
  return (
    <p className="material-warning prep-privacy" role="note">
      {UPLOAD_WARNING}
    </p>
  );
}

/** The latest few conversations, quietly, under the bar. */
function RecentSessions() {
  const [sessions, setSessions] = useState<PrepSessionSummary[] | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .listPrepSessions()
      .then((res) => alive && setSessions(res.sessions))
      // The list is a convenience: without it the page still works.
      .catch(() => alive && setSessions([]));
    return () => {
      alive = false;
    };
  }, []);
  if (!sessions || sessions.length === 0) return null;
  return (
    <section className="prep-recent" aria-labelledby="prep-recent-heading">
      <div className="head-row">
        <h2 id="prep-recent-heading">最近的備課</h2>
        <Link className="text-link is-plain" to="/prep">
          全部
        </Link>
      </div>
      <ul className="rows is-compact">
        {sessions.slice(0, RECENT_SHOWN).map((s) => (
          <li key={s.id} className="row">
            <div>
              <h3>
                <Link to={`/prep/${s.id}`}>{sessionTitle(s)}</Link>
              </h3>
              <p className="meta">
                {[s.last_kind ? KIND_LABEL[s.last_kind] : null, formatDateTime(s.updated_at)].filter(Boolean).join(" · ")}
              </p>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

/**
 * 備課 (the home page): one input bar, as on 築學網. 「從課文開始」 takes the lesson text (upload,
 * library or paste), optional class and template, then an action chip; 「直接問 AI」 takes a
 * question. Either creates a prep session and opens it.
 */
export function PrepHomePage() {
  const ids = useId();
  const navigate = useNavigate();
  const { currentClassId } = useApp();
  const classes = useActiveClasses();
  const templates = useReadyTemplates();
  const [tab, setTab] = useState<Tab>("material");
  const [choice, setChoice] = useState<MaterialChoice>(emptyMaterialChoice);
  const [topic, setTopic] = useState("");
  /** undefined: follow the current class (if it is one of the teacher's). */
  const [classPick, setClassPick] = useState<number | null | undefined>(undefined);
  const [templateId, setTemplateId] = useState<number | null>(null);
  const [otherOpen, setOtherOpen] = useState(false);
  const [otherText, setOtherText] = useState("");
  const [question, setQuestion] = useState("");
  const [askMaterial, setAskMaterial] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const defaultClass = classes?.some((c) => c.id === currentClassId) ? currentClassId : null;
  const classId = classPick === undefined ? defaultClass : classPick;
  const material = materialFields(choice);
  const hasMaterial = material !== null;
  const hasTopic = topic.trim() !== "";

  async function start(body: CreatePrepSessionRequest) {
    if (classId !== null) body.class_id = classId;
    if (templateId !== null && body.kind !== "ask") body.template_id = templateId;
    setBusy(true);
    setError(null);
    try {
      const session = await api.createPrepSession(body);
      navigate(`/prep/${session.id}`);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  function action(kind: ActionKind) {
    if (material) {
      void start({ kind, request: ACTION_LABEL[kind], ...material });
    } else if (kind === "resources" && hasTopic) {
      void start({ kind, request: ACTION_LABEL[kind], topic: topic.trim() });
    }
  }

  function sendOther() {
    const text = otherText.trim();
    if (!text || !material) return;
    void start({ kind: "ask", request: text, ...material });
  }

  function sendQuestion() {
    const text = question.trim();
    if (!text) return;
    void start({ kind: "ask", request: text, ...(askMaterial && material ? material : {}) });
  }

  const contextChips = (withTemplate: boolean) =>
    (classes?.length || (withTemplate && templates?.length)) ? (
      <div className="prep-context" role="group" aria-label="備課情境（可以不選）">
        {classes && classes.length > 0 && (
          <ClassChip classes={classes} value={classId} disabled={busy} onChange={setClassPick} />
        )}
        {withTemplate && templates && templates.length > 0 && (
          <TemplateChip id={`${ids}-template`} templates={templates} value={templateId} disabled={busy} onChange={setTemplateId} />
        )}
      </div>
    ) : null;

  return (
    <OncePerPage>
      <div className="page-head prep-home-head">
        <h1>老師，今天要教哪一課？</h1>
      </div>

      <section className="prep-bar-card" aria-label="開始備課">
        <div className="ws-tabs prep-tabs" role="tablist" aria-label="怎麼開始">
          {(
            [
              ["material", "從課文開始"],
              ["ask", "直接問 AI"],
            ] as const
          ).map(([key, label]) => (
            <button
              key={key}
              type="button"
              role="tab"
              id={`${ids}-tab-${key}`}
              aria-selected={tab === key}
              aria-controls={`${ids}-panel`}
              className={tab === key ? "is-current" : ""}
              onClick={() => {
                setTab(key);
                setError(null);
              }}
            >
              {label}
            </button>
          ))}
        </div>

        <div id={`${ids}-panel`} role="tabpanel" aria-labelledby={`${ids}-tab-${tab}`} className="prep-panel">
          <PrivacyReminder />

          {tab === "material" && (
            <>
              <MaterialChooser
                choice={choice}
                onChange={(next) => {
                  setChoice(next);
                  setError(null);
                }}
                pasteRows={6}
              />
              {!hasMaterial && (
                <div className="prep-topic">
                  <label className="field-label" htmlFor={`${ids}-topic`}>
                    或只輸入主題 <span className="optional-label">只能找教學資源</span>
                  </label>
                  <input
                    id={`${ids}-topic`}
                    type="text"
                    maxLength={200}
                    value={topic}
                    onChange={(e) => setTopic(e.target.value)}
                  />
                </div>
              )}

              {contextChips(true)}

              <div className="prep-chips" role="group" aria-label="要 Manna 做什麼">
                {ACTION_KINDS.map((kind) => (
                  <button
                    key={kind}
                    type="button"
                    className="prep-chip"
                    disabled={busy || !(hasMaterial || (kind === "resources" && hasTopic))}
                    onClick={() => action(kind)}
                  >
                    {ACTION_LABEL[kind]}
                  </button>
                ))}
                <button
                  type="button"
                  className="prep-chip"
                  aria-expanded={otherOpen}
                  disabled={busy || !hasMaterial}
                  onClick={() => setOtherOpen((o) => !o)}
                >
                  {OTHER_LABEL}
                </button>
              </div>
              {!hasMaterial && <p className="quiet-note">放進教材之後就可以選；只有主題時可以找教學資源。</p>}

              {otherOpen && hasMaterial && (
                <form
                  className="prep-other"
                  onSubmit={(e) => {
                    e.preventDefault();
                    sendOther();
                  }}
                >
                  <label className="field-label" htmlFor={`${ids}-other`}>
                    你想做什麼？
                  </label>
                  <textarea
                    id={`${ids}-other`}
                    rows={3}
                    maxLength={2000}
                    value={otherText}
                    placeholder={ASK_PLACEHOLDER}
                    onChange={(e) => setOtherText(e.target.value)}
                  />
                  <div className="actions">
                    <button type="submit" className="primary-button" disabled={busy || otherText.trim() === ""}>
                      {busy ? "送出中…" : "送出"}
                    </button>
                  </div>
                </form>
              )}
            </>
          )}

          {tab === "ask" && (
            <form
              className="prep-ask"
              onSubmit={(e) => {
                e.preventDefault();
                sendQuestion();
              }}
            >
              <label className="sr-only" htmlFor={`${ids}-question`}>
                直接問 AI
              </label>
              <textarea
                id={`${ids}-question`}
                rows={4}
                maxLength={2000}
                value={question}
                placeholder={ASK_PLACEHOLDER}
                onChange={(e) => setQuestion(e.target.value)}
              />
              {askMaterial ? (
                <div className="prep-ask-material">
                  <MaterialChooser choice={choice} onChange={setChoice} pasteRows={5} />
                  <button
                    type="button"
                    className="text-link is-plain"
                    onClick={() => {
                      setAskMaterial(false);
                    }}
                  >
                    不附教材
                  </button>
                </div>
              ) : (
                <p className="quiet-note">
                  <button type="button" className="text-link is-plain" onClick={() => setAskMaterial(true)}>
                    附上教材（可以不附）
                  </button>
                </p>
              )}
              {contextChips(false)}
              <div className="actions">
                <button type="submit" className="primary-button" disabled={busy || question.trim() === ""}>
                  {busy ? "送出中…" : "送出"}
                </button>
              </div>
            </form>
          )}
          <ErrorText message={error} />
        </div>
      </section>

      <RecentSessions />

      <p className="page-foot">
        <Link className="text-link is-plain" to="/classes">
          完整備課流程（班級、單元、課例 SOP）
        </Link>
      </p>
    </OncePerPage>
  );
}
