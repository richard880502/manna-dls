import { useState } from "react";
import { api, errorMessage } from "../../api/client";
import type {
  AiResult,
  DecisionKind,
  DecisionReason,
  DecisionRequest,
  PlanSuggestOutput,
  PlanSuggestion,
  Source,
} from "../../api/types";
import { useApp } from "../../context/AppContext";
import { DECISION_REASONS, DECISION_REASON_LABELS, ICAP_LABELS } from "../../lib/labels";
import { logScaffold } from "../../lib/scaffold";
import { AiContextPanel } from "../AiContextPanel";
import { AiStatusMessage, AiUnavailableNote, ErrorText, PrivacyHint } from "../common";
import type { EditableStep } from "./StepsEditor";

type PlanDecision = Extract<DecisionKind, "adopt" | "revise" | "reject">;

const DECIDED_LABELS: Record<PlanDecision, string> = {
  adopt: "已採用",
  revise: "已修訂",
  reject: "已拒絕",
};

interface SuggestionsPanelProps {
  lessonId: number;
  /** 目前班級: its profile goes into Manna's context. */
  classId?: number | null;
  onAppendStep: (step: Omit<EditableStep, "key">) => void;
  /** Moved to the top of the lesson page because of the teacher's preference (P3 找合適素材). */
  pinned?: boolean;
}

export function SuggestionsPanel({ lessonId, classId, onAppendStep, pinned = false }: SuggestionsPanelProps) {
  const { aiEnabled } = useApp();
  const [note, setNote] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<AiResult<PlanSuggestOutput> | null>(null);
  const [requestId, setRequestId] = useState(0);

  async function requestSuggestions() {
    setLoading(true);
    setError(null);
    logScaffold("SC-RAG", "expand", { trigger: "request" });
    try {
      const trimmed = note.trim();
      const res = await api.requestSuggestions(lessonId, {
        ...(trimmed ? { request_note: trimmed } : {}),
        ...(classId ? { class_id: classId } : {}),
      });
      setResult(res);
      setRequestId((n) => n + 1);
      if (res.status === "shown") logScaffold("SC-RAG", "show", { trigger: "suggestions" });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }

  const output = result?.status === "shown" ? result.output : null;

  return (
    <section className={`suggestion-card${pinned ? " is-pinned" : ""}`} aria-labelledby="rag-heading">
      <div className="head-row">
        <div>
          <p className="manna-label"><span className="star" aria-hidden="true" />Manna</p>
          <h2 id="rag-heading">備課建議</h2>
        </div>
        <div className="card-heading-tags">
          {pinned && <span className="optional-label">依你的備課偏好置頂</span>}
          {result && result.sources.length > 0 && <span className="source-count">{result.sources.length} 個來源</span>}
        </div>
      </div>
      <p className="field-hint">每項建議都附知識庫引用；採用、修訂或拒絕由你決定，也可以完全不使用。</p>

      <label className="field-label" htmlFor="rag-note">
        想請 Manna 特別注意什麼？
      </label>
      <textarea
        id="rag-note"
        rows={2}
        value={note}
        onChange={(e) => setNote(e.target.value)}
        placeholder="例如：想多一點口說練習，班上程度落差大。"
      />
      <PrivacyHint />
      {!aiEnabled && <AiUnavailableNote />}
      <div className="button-row">
        <button
          type="button"
          className="manna-button"
          onClick={() => void requestSuggestions()}
          disabled={loading || !aiEnabled}
        >
          {loading ? "Manna 查找中…" : aiEnabled ? "請 Manna 提供建議" : "AI 目前未開放"}
        </button>
      </div>
      <ErrorText message={error} />

      {result && result.status !== "shown" && <AiStatusMessage status={result.status} message={result.message} />}
      {result && <AiContextPanel key={requestId} items={result.context} />}

      {output?.kb_empty && (
        <div className="ai-status ai-status-kb-empty" role="status">
          知識庫查無相關條目。你可以繼續手動完成教案。
        </div>
      )}

      {output && !output.kb_empty && output.suggestions.length > 0 && (
        <div className="suggestion-list">
          {output.suggestions.map((suggestion, index) => (
            <SuggestionCard
              key={`${requestId}-${index}`}
              index={index}
              suggestion={suggestion}
              sources={result?.sources ?? []}
              interactionId={result?.interaction_id ?? null}
              onAppendStep={onAppendStep}
            />
          ))}
        </div>
      )}
    </section>
  );
}

interface SuggestionCardProps {
  index: number;
  suggestion: PlanSuggestion;
  sources: Source[];
  interactionId: number | null;
  onAppendStep: (step: Omit<EditableStep, "key">) => void;
}

export function SuggestionCard({ index, suggestion, sources, interactionId, onAppendStep }: SuggestionCardProps) {
  const [revising, setRevising] = useState(false);
  const [revisedText, setRevisedText] = useState(suggestion.body);
  const [reason, setReason] = useState<DecisionReason | "">("");
  const [reasonNote, setReasonNote] = useState("");
  const [decided, setDecided] = useState<PlanDecision | null>(null);
  const [appended, setAppended] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const titleId = `suggestion-${index}-title`;
  const hasDiff =
    typeof suggestion.duration_min === "number" || !!suggestion.support_note || !!suggestion.extension_note;

  async function decide(decision: PlanDecision) {
    if (interactionId === null) return;
    const body: DecisionRequest = { item_index: index, decision };
    if (decision === "revise") body.revised_content = revisedText.trim();
    if (reason) body.reason = reason;
    if (reason && reasonNote.trim()) body.reason_note = reasonNote.trim();

    setSending(true);
    setError(null);
    try {
      await api.sendDecision(interactionId, body);
      setDecided(decision);
      setRevising(false);
      logScaffold("SC-RAG", decision === "reject" ? "skip" : "use", { trigger: `${decision}-${index}` });
      if (decision === "adopt" || decision === "revise") {
        onAppendStep({
          activity_type: suggestion.activity_type ?? "",
          icap_level: suggestion.icap_level,
          content: decision === "revise" ? revisedText.trim() : `${suggestion.title}：${suggestion.body}`,
          duration_min: typeof suggestion.duration_min === "number" ? suggestion.duration_min : null,
          support_note: suggestion.support_note ?? "",
          extension_note: suggestion.extension_note ?? "",
        });
        setAppended(true);
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSending(false);
    }
  }

  return (
    <article className={`ai-suggestion suggestion-item${decided ? ` is-${decided}` : ""}`} aria-labelledby={titleId}>
      <div className="suggestion-head">
        <strong id={titleId}>{suggestion.title}</strong>
        {decided && <span className="status-pill">{DECIDED_LABELS[decided]}</span>}
      </div>
      <div className="suggestion-tags">
        {suggestion.activity_type && <span className="soft-tag">{suggestion.activity_type}</span>}
        {suggestion.icap_level && <span className="soft-tag">{ICAP_LABELS[suggestion.icap_level]}</span>}
      </div>
      <p>{suggestion.body}</p>

      {hasDiff && (
        <dl className="suggestion-diff" aria-label="時間與差異化">
          {typeof suggestion.duration_min === "number" && (
            <div>
              <dt>預估時間</dt>
              <dd>
                {suggestion.duration_min} 分鐘 <span className="ai-estimate">AI 估計，請依班級調整</span>
              </dd>
            </div>
          )}
          {suggestion.support_note && (
            <div>
              <dt>程度較弱的支援</dt>
              <dd>{suggestion.support_note}</dd>
            </div>
          )}
          {suggestion.extension_note && (
            <div>
              <dt>程度較好的延伸</dt>
              <dd>{suggestion.extension_note}</dd>
            </div>
          )}
        </dl>
      )}

      <div className="citations">
        <span className="citations-label">引用條目</span>
        <ul className="citation-list">
          {suggestion.citations.map((code) => {
            const src = sources.find((s) => s.item_code === code);
            return (
              <li key={code} className="source-detail">
                <code>{code}</code>
                {src ? (
                  <>
                    {" "}
                    <strong>{src.title}</strong>
                    <span className="citation-meta">
                      {src.source_doc}
                      {src.source_page ? `，p. ${src.source_page}` : ""}
                    </span>
                  </>
                ) : (
                  <span className="citation-meta">（找不到這個條目的來源資料）</span>
                )}
              </li>
            );
          })}
        </ul>
      </div>

      {!decided && interactionId !== null && (
        <>
          <div className="reason-row">
            <label className="field-label" htmlFor={`suggestion-${index}-reason`}>
              理由（選填）
            </label>
            <select
              id={`suggestion-${index}-reason`}
              value={reason}
              onChange={(e) => setReason(e.target.value as DecisionReason | "")}
            >
              <option value="">不填理由</option>
              {DECISION_REASONS.map((r) => (
                <option key={r} value={r}>
                  {DECISION_REASON_LABELS[r]}
                </option>
              ))}
            </select>
            {reason === "other" && (
              <input
                aria-label="其他理由說明（選填）"
                value={reasonNote}
                onChange={(e) => setReasonNote(e.target.value)}
                placeholder="簡短說明（選填）"
              />
            )}
          </div>

          {revising && (
            <div className="edit-suggestion-field">
              <label className="field-label" htmlFor={`suggestion-${index}-revise`}>
                修訂後的內容
              </label>
              <textarea
                id={`suggestion-${index}-revise`}
                rows={4}
                value={revisedText}
                onChange={(e) => setRevisedText(e.target.value)}
              />
              <div className="button-row">
                <button
                  type="button"
                  className="secondary-button"
                  disabled={sending || !revisedText.trim()}
                  onClick={() => void decide("revise")}
                >
                  送出修訂
                </button>
                <button type="button" className="ghost-button" onClick={() => setRevising(false)}>
                  取消修訂
                </button>
              </div>
            </div>
          )}

          {!revising && (
            <div className="ai-decision">
              <button
                type="button"
                className="decision-button"
                disabled={sending}
                onClick={() => void decide("adopt")}
              >
                採用
              </button>
              <button
                type="button"
                className="decision-button"
                disabled={sending}
                onClick={() => setRevising(true)}
              >
                修訂
              </button>
              <button
                type="button"
                className="decision-button"
                disabled={sending}
                onClick={() => void decide("reject")}
              >
                拒絕
              </button>
            </div>
          )}
        </>
      )}

      {appended && <p className="field-hint appended-note">已加入左側教案步驟，記得按「儲存教案」。</p>}
      <ErrorText message={error} />
    </article>
  );
}
