import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { PrepSessionSummary } from "../api/types";
import { ErrorText } from "../components/common";
import { Icon } from "../components/Icon";
import {
  ATTACH_LABEL,
  AttachDialog,
  MaterialChip,
  PrivacyReminder,
  choiceTitle,
  type AttachWay,
} from "../components/material/AttachDialog";
import { materialFields, type MaterialChoice } from "../components/material/MaterialChooser";
import { WorksheetNotice, type MimicState } from "../components/material/WorksheetNotice";
import { OncePerPage } from "../components/OncePerPage";
import { usePopover } from "../components/Popover";
import {
  KIND_TOOLS,
  MATERIAL_REQUIRED,
  REQUEST_MAX,
  TOOLS,
  TOOL_BLURB,
  TOOL_CHIP,
  TOOL_NAME,
  asCreateRequest,
  mimicPath,
  toolFromWords,
  type KindTool,
  type StartKind,
} from "../lib/create";
import { KIND_LABEL, sessionTitle } from "../lib/prep";
import { useAutoGrow } from "../lib/useAutoGrow";
import { formatDateTime } from "../lib/worksheet";

const RECENT_SHOWN = 5;

export const HOME_HEADING = "今天想準備什麼？";
export const QUICK_PLACEHOLDER = "例如：B1 程度、8 題閱讀理解，記憶到評鑑都要有";

/** The latest few conversations, quietly, under the tools. */
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
    <section className="home-recent" aria-labelledby="home-recent-heading">
      <div className="head-row">
        <h2 id="home-recent-heading">最近的備課</h2>
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
                {[s.last_kind ? (KIND_LABEL[s.last_kind] ?? null) : null, formatDateTime(s.updated_at)]
                  .filter(Boolean)
                  .join(" · ")}
              </p>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

/**
 * The quick box (like Diffit's): one text box, 附上文章 and send. With a material it makes 閱讀理解題
 * unless a chip says otherwise; without one, a question to Manna. Typed words pick a chip until the
 * teacher picks one herself.
 */
function QuickBox() {
  const ids = useId();
  const navigate = useNavigate();
  const [text, setText] = useState("");
  const [choice, setChoice] = useState<MaterialChoice | null>(null);
  /** null: the teacher has not clicked a chip, so typed words choose. */
  const [clicked, setClicked] = useState<{ tool: KindTool | null } | null>(null);
  /** Materials the teacher said to keep as the article (no worksheet notice for them). */
  const [keptAsText, setKeptAsText] = useState<ReadonlySet<number>>(() => new Set());
  const [dialog, setDialog] = useState<AttachWay | null>(null);
  const [needMaterial, setNeedMaterial] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { open: attachOpen, toggle: toggleAttach, close: closeAttach, rootRef: attachRoot, triggerRef: attachTrigger, onBlur: attachBlur } =
    usePopover();
  const boxRef = useRef<HTMLTextAreaElement>(null);
  useAutoGrow(boxRef, text);
  const backToAttach = useCallback(() => attachTrigger.current, [attachTrigger]);

  const tool = clicked ? clicked.tool : toolFromWords(text);
  const material = choice ? materialFields(choice) : null;
  const typed = text.trim();
  const kind: StartKind | null = tool ?? (material ? "comprehension" : typed ? "ask" : null);
  const missing = needMaterial && material === null && kind !== null && kind !== "ask";
  const picked = choice?.picked ?? null;
  const worksheetLike = picked?.looks_like_worksheet === true && !keptAsText.has(picked.id) ? picked : null;

  async function send() {
    if (busy || kind === null) return;
    if (kind !== "ask" && material === null) {
      setNeedMaterial(true);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const request = typed || (kind === "ask" ? "" : TOOL_CHIP[kind]);
      const session = await api.createPrepSession(asCreateRequest({ kind, request, ...(material ?? {}) }));
      navigate(`/prep/${session.id}`);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    void send();
  }

  function open(way: AttachWay) {
    closeAttach();
    setDialog(way);
  }

  return (
    <>
      <section className="compose-card is-quick" aria-label="快速開始">
        <label className="sr-only" htmlFor={`${ids}-box`}>
          給 Manna 的要求
        </label>
        <textarea
          id={`${ids}-box`}
          ref={boxRef}
          className="prompt-input"
          rows={2}
          maxLength={REQUEST_MAX}
          value={text}
          placeholder={QUICK_PLACEHOLDER}
          disabled={busy}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
        />
        {choice && (
          <div className="prompt-chips">
            <MaterialChip title={choiceTitle(choice)} disabled={busy} onRemove={() => setChoice(null)} />
          </div>
        )}
        <div className="prompt-toolbar">
          <div className="popover-root" ref={attachRoot} onBlur={attachBlur}>
            <button
              ref={attachTrigger}
              type="button"
              className="tool-button"
              aria-expanded={attachOpen}
              aria-controls={`${ids}-attach`}
              disabled={busy}
              onClick={toggleAttach}
            >
              <Icon name="paperclip" />
              附上文章
            </button>
            {attachOpen && (
              <div className="popover" id={`${ids}-attach`} role="group" aria-label="附上文章">
                {(["upload", "library", "paste"] as const).map((way) => (
                  <button key={way} type="button" className="popover-item" onClick={() => open(way)}>
                    <Icon name={way === "upload" ? "upload" : way === "library" ? "library" : "paste"} />
                    {ATTACH_LABEL[way]}
                  </button>
                ))}
              </div>
            )}
          </div>
          {missing && (
            <p className="prompt-need" role="alert">
              {MATERIAL_REQUIRED}
            </p>
          )}
          <button
            type="button"
            className="send-button"
            aria-label="送出"
            disabled={busy || kind === null}
            onClick={() => void send()}
          >
            <Icon name="send" />
          </button>
        </div>
      </section>

      {worksheetLike && (
        <WorksheetNotice
          disabled={busy}
          onUse={() => {
            const state: MimicState = { sample: { id: worksheetLike.id, title: worksheetLike.title } };
            navigate(mimicPath(worksheetLike.id), { state });
          }}
          onKeep={() => setKeptAsText((kept) => new Set(kept).add(worksheetLike.id))}
        />
      )}

      <div className="output-chips" role="group" aria-label="要做什麼">
        {KIND_TOOLS.map((t) => (
          <button
            key={t}
            type="button"
            className="output-chip"
            aria-pressed={tool === t}
            disabled={busy}
            onClick={() => {
              setClicked({ tool: tool === t ? null : t });
              setNeedMaterial(false);
            }}
          >
            {TOOL_CHIP[t]}
          </button>
        ))}
      </div>
      <ErrorText message={error} />
      {dialog === null && <PrivacyReminder />}

      {dialog && (
        <AttachDialog
          way={dialog}
          pasted={choice?.way === "paste" ? { title: choice.title, text: choice.text } : undefined}
          returnFocus={backToAttach}
          onClose={() => setDialog(null)}
          onChoice={(next) => {
            setChoice(next);
            setNeedMaterial(false);
            setDialog(null);
          }}
        />
      )}
    </>
  );
}

/**
 * 首頁: a greeting with the quick box, the tools (each opens its generator; 照我的學習單做 leads,
 * across the whole row), and the latest prep sessions.
 */
export function PrepHomePage() {
  return (
    <OncePerPage>
      <div className="home-head">
        <h1>{HOME_HEADING}</h1>
        <p className="lead">附上一篇文章或教材，選要做什麼，Manna 依 Bloom 認知層次出題。</p>
      </div>

      <QuickBox />

      <section className="home-tools" aria-labelledby="home-tools-heading">
        <h2 id="home-tools-heading">Manna 可以幫你做</h2>
        <ul className="tool-cards">
          {TOOLS.map((t) => (
            <li key={t} className={t === "mimic" ? "is-featured" : undefined}>
              <Link className={`tool-card${t === "mimic" ? " is-featured" : ""}`} to={`/create/${t}`}>
                <span className={`tool-card-icon is-${t}`} aria-hidden="true">
                  <Icon name={t} />
                </span>
                <span className="tool-card-title">{TOOL_NAME[t]}</span>
                <span className="tool-card-blurb">{TOOL_BLURB[t]}</span>
              </Link>
            </li>
          ))}
        </ul>
      </section>

      <RecentSessions />
    </OncePerPage>
  );
}
