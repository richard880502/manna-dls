import { useEffect, useState, type FormEvent } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../../api/client";
import type { AiResult, MaterialCandidate, MaterialFocusesOutput, UnitMaterial } from "../../api/types";
import { useApp } from "../../context/AppContext";
import { MAX_FOCUSES, buildDraft, emptyFocus, focusFromServer } from "../../lib/core";
import { formatDateTime } from "../../lib/labels";
import { logScaffold } from "../../lib/scaffold";
import { AiContextPanel } from "../AiContextPanel";
import { AiStatusMessage, AiUnavailableNote, ErrorText, Loading, Notice } from "../common";

export const MATERIAL_MAX_CHARS = 50000;
export const MATERIAL_FILENAME_MAX_CHARS = 200;

interface CandidateState {
  status: "adopted" | "skipped";
  position?: number;
}

interface MaterialsPanelProps {
  unitId: number;
  /** 目前班級: its profile goes into Manna's context. */
  classId?: number | null;
  /** Material-first route: shown first and expanded. Otherwise collapsed but always openable. */
  emphasized: boolean;
  /** Called after a candidate was put into the CoRe draft (teacher-initiated). */
  onCoreChanged: () => void;
  onCountChange?: (count: number) => void;
}

/** 教材優先：貼上教材 → 請 Manna 從教材提出候選學習重點 → 由教師決定是否放進 CoRe. */
export function MaterialsPanel({ unitId, classId, emphasized, onCoreChanged, onCountChange }: MaterialsPanelProps) {
  const { aiEnabled } = useApp();
  /** null = follow the route default (open when material-first); true/false once the teacher toggles. */
  const [openToggle, setOpenToggle] = useState<boolean | null>(null);
  const open = openToggle ?? emphasized;
  const [materials, setMaterials] = useState<UnitMaterial[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [filename, setFilename] = useState("");
  const [text, setText] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const [confirmDelete, setConfirmDelete] = useState<number | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const [requestFor, setRequestFor] = useState<number | null>(null);
  const [requesting, setRequesting] = useState(false);
  const [requestError, setRequestError] = useState<string | null>(null);
  const [result, setResult] = useState<AiResult<MaterialFocusesOutput> | null>(null);
  const [candidateState, setCandidateState] = useState<Record<number, CandidateState>>({});
  const [busyIndex, setBusyIndex] = useState<number | null>(null);
  const [candidateError, setCandidateError] = useState<{ index: number; message: string } | null>(null);

  const count = materials?.length ?? null;
  useEffect(() => {
    if (count !== null) onCountChange?.(count);
  }, [count, onCountChange]);

  useEffect(() => {
    let alive = true;
    api
      .listMaterials(unitId)
      .then((list) => alive && setMaterials(list))
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [unitId]);

  async function onSave(event: FormEvent) {
    event.preventDefault();
    const body = text.trim();
    if (!body) {
      setSaveError("請先貼上教材文字。");
      return;
    }
    if (text.length > MATERIAL_MAX_CHARS) {
      setSaveError(`教材文字上限 ${MATERIAL_MAX_CHARS.toLocaleString()} 字，請分成幾份貼上。`);
      return;
    }
    setSaving(true);
    setSaveError(null);
    try {
      const saved = await api.createMaterial(unitId, {
        text: body,
        ...(filename.trim() ? { filename: filename.trim() } : {}),
      });
      setMaterials((prev) => [...(prev ?? []), saved]);
      setText("");
      setFilename("");
    } catch (err) {
      setSaveError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  }

  async function onDelete(materialId: number) {
    setDeleting(true);
    setDeleteError(null);
    try {
      await api.deleteMaterial(unitId, materialId);
      setMaterials((prev) => (prev ?? []).filter((m) => m.id !== materialId));
      setConfirmDelete(null);
      if (requestFor === materialId) {
        setRequestFor(null);
        setResult(null);
        setCandidateState({});
      }
    } catch (err) {
      setDeleteError(errorMessage(err));
    } finally {
      setDeleting(false);
    }
  }

  async function requestCandidates(materialId: number) {
    setRequestFor(materialId);
    setRequesting(true);
    setRequestError(null);
    setResult(null);
    setCandidateState({});
    setCandidateError(null);
    logScaffold("SC-MATERIAL", "expand", { trigger: "request" });
    try {
      const res = await api.materialFocuses(unitId, materialId, classId);
      setResult(res);
      if (res.status === "shown") logScaffold("SC-MATERIAL", "show", { trigger: "candidates" });
    } catch (err) {
      setRequestError(errorMessage(err));
    } finally {
      setRequesting(false);
    }
  }

  function sendDecision(index: number, decision: "adopt" | "skip") {
    const interactionId = result?.interaction_id ?? null;
    if (interactionId === null) return;
    void api.sendDecision(interactionId, { item_index: index, decision }).catch(() => undefined);
  }

  /** Teacher-initiated: append the candidate's big idea as a new CoRe focus via the existing draft PUT. */
  async function adopt(index: number, candidate: MaterialCandidate) {
    setBusyIndex(index);
    setCandidateError(null);
    try {
      const version = await api.getCore(unitId);
      const existing = [...version.focuses].sort((a, b) => a.position - b.position).map(focusFromServer);
      if (existing.length >= MAX_FOCUSES) {
        setCandidateError({
          index,
          message: `CoRe 已有 ${MAX_FOCUSES} 個學習重點。請先到 CoRe 移除一個，再放進這個候選重點。`,
        });
        return;
      }
      await api.saveCoreDraft(unitId, buildDraft([...existing, { ...emptyFocus(), big_idea: candidate.big_idea }]));
      setCandidateState((prev) => ({ ...prev, [index]: { status: "adopted", position: existing.length + 1 } }));
      sendDecision(index, "adopt");
      logScaffold("SC-MATERIAL", "use", { trigger: `adopt-${index}` });
      onCoreChanged();
    } catch (err) {
      setCandidateError({ index, message: errorMessage(err) });
    } finally {
      setBusyIndex(null);
    }
  }

  function skip(index: number) {
    setCandidateState((prev) => ({ ...prev, [index]: { status: "skipped" } }));
    sendDecision(index, "skip");
    logScaffold("SC-MATERIAL", "skip", { trigger: `skip-${index}` });
  }

  const candidates = result?.status === "shown" ? (result.output?.candidates ?? []) : [];
  const requestMaterial = materials?.find((m) => m.id === requestFor);

  return (
    <section
      className={`panel-card materials-panel${emphasized ? " is-route-first" : ""}`}
      aria-labelledby="materials-heading"
    >
      <div className="card-heading">
        <div>
          <p className="eyebrow warm">教材優先 · Manna</p>
          <h3 id="materials-heading" tabIndex={-1}>
            單元教材與候選學習重點
          </h3>
        </div>
        <button
          type="button"
          className="ghost-button small-button"
          aria-expanded={open}
          aria-controls="materials-body"
          onClick={() => setOpenToggle(!open)}
        >
          {open ? "收合教材區" : "展開教材區"}
        </button>
      </div>
      {!open && (
        <p className="field-hint">
          {materials && materials.length > 0 ? `已貼上 ${materials.length} 份教材。` : ""}
          可以貼上課文或教材文字，請 Manna 從教材提出候選學習重點。
        </p>
      )}

      {open && (
        <div id="materials-body">
          <p className="field-hint">
            教材只用於這個單元；刪除時會實際刪除原文。Manna 只會提出「候選」學習重點，要不要放進 CoRe 由你決定。
          </p>

          {loadError && <ErrorText message={loadError} />}
          {!materials && !loadError && <Loading />}
          {materials && materials.length === 0 && <p className="empty-preview">還沒有教材。先在下面貼上課文或教材文字。</p>}
          {materials && materials.length > 0 && (
            <ul className="material-list">
              {materials.map((m) => (
                <li key={m.id} className="material-item" aria-label={m.filename || `教材 ${m.id}`}>
                  <div className="material-item-main">
                    <strong>{m.filename || "未命名教材"}</strong>
                    <span>
                      {m.char_count.toLocaleString()} 字 · {formatDateTime(m.created_at)}
                    </span>
                  </div>
                  <div className="material-item-actions">
                    <button
                      type="button"
                      className="secondary-button small-button"
                      onClick={() => void requestCandidates(m.id)}
                      disabled={!aiEnabled || requesting}
                    >
                      {requesting && requestFor === m.id ? "Manna 閱讀中…" : "請 Manna 從教材提出候選學習重點"}
                    </button>
                    {confirmDelete === m.id ? (
                      <span className="confirm-row" role="group" aria-label="確認刪除教材">
                        確定刪除這份教材？原文會實際刪除，無法復原。
                        <button
                          type="button"
                          className="ghost-button small-button"
                          onClick={() => void onDelete(m.id)}
                          disabled={deleting}
                        >
                          {deleting ? "刪除中…" : "確定刪除"}
                        </button>
                        <button
                          type="button"
                          className="ghost-button small-button"
                          onClick={() => setConfirmDelete(null)}
                          disabled={deleting}
                        >
                          取消
                        </button>
                      </span>
                    ) : (
                      <button
                        type="button"
                        className="ghost-button small-button"
                        aria-label={`刪除教材 ${m.filename || m.id}`}
                        onClick={() => {
                          setConfirmDelete(m.id);
                          setDeleteError(null);
                        }}
                      >
                        刪除
                      </button>
                    )}
                  </div>
                </li>
              ))}
            </ul>
          )}
          <ErrorText message={deleteError} />
          {!aiEnabled && materials && materials.length > 0 && <AiUnavailableNote />}

          <form className="material-form" onSubmit={onSave}>
            <h4>貼上教材文字</h4>
            <label className="field-label" htmlFor="material-filename">
              教材名稱（選填）
            </label>
            <input
              id="material-filename"
              value={filename}
              maxLength={MATERIAL_FILENAME_MAX_CHARS}
              onChange={(e) => setFilename(e.target.value)}
              placeholder="例如：B1 L3 課文"
              disabled={saving}
            />
            <label className="field-label" htmlFor="material-text">
              教材文字
            </label>
            <textarea
              id="material-text"
              rows={6}
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder="把課文、閱讀文本或教材內容貼在這裡。"
              disabled={saving}
            />
            <p className={`field-hint${text.length > MATERIAL_MAX_CHARS ? " is-over" : ""}`}>
              {text.length.toLocaleString()} / {MATERIAL_MAX_CHARS.toLocaleString()} 字。請不要貼上含學生姓名或個人資料的內容。
            </p>
            <ErrorText message={saveError} />
            <button className="primary-button" type="submit" disabled={saving}>
              {saving ? "儲存中…" : "儲存教材"}
            </button>
          </form>

          {(requesting || result || requestError) && (
            <div className="candidates" aria-live="polite">
              <h4>
                Manna 從教材提出的候選學習重點
                {requestMaterial ? `（${requestMaterial.filename || "未命名教材"}）` : ""}
              </h4>
              {requesting && <p className="loading-text">Manna 閱讀教材中…</p>}
              <ErrorText message={requestError} />
              {result && result.status !== "shown" && <AiStatusMessage status={result.status} message={result.message} />}
              {result?.status === "shown" && candidates.length === 0 && (
                <div className="ai-status" role="status">
                  教材內容不足以提出候選重點。你可以多貼一些教材，或直接到 CoRe 自己寫。
                </div>
              )}
              {candidates.length > 0 && (
                <p className="field-hint">每個候選都附上教材原文段落。不會自動放進 CoRe；按「放進我的學習重點」才會加入，之後仍可修改或移除。</p>
              )}
              {candidates.map((c, index) => {
                const state = candidateState[index];
                const titleId = `candidate-${index}-title`;
                return (
                  <article
                    key={`${requestFor}-${index}`}
                    className={`candidate-card${state ? ` is-${state.status}` : ""}`}
                    aria-labelledby={titleId}
                  >
                    <div className="suggestion-head">
                      <span className="candidate-index">候選 {index + 1}</span>
                      {state?.status === "adopted" && <span className="status-pill">已放進學習重點 {state.position}</span>}
                      {state?.status === "skipped" && <span className="status-pill">已略過</span>}
                    </div>
                    <strong id={titleId} className="candidate-title">
                      {c.big_idea}
                    </strong>
                    <p className="candidate-rationale">{c.rationale}</p>
                    <blockquote className="material-excerpt">
                      <span className="material-excerpt-label">教材原文</span>
                      {c.excerpt}
                    </blockquote>
                    {!state && (
                      <div className="ai-decision">
                        <button
                          type="button"
                          className="decision-button"
                          disabled={busyIndex !== null}
                          onClick={() => void adopt(index, c)}
                        >
                          {busyIndex === index ? "加入中…" : "放進我的學習重點"}
                        </button>
                        <button
                          type="button"
                          className="decision-button"
                          disabled={busyIndex !== null}
                          onClick={() => skip(index)}
                        >
                          略過
                        </button>
                      </div>
                    )}
                    {state?.status === "adopted" && (
                      <p className="field-hint appended-note">
                        已加入 CoRe 草稿。<Link to={`/units/${unitId}/core`}>打開 CoRe 繼續寫八題 →</Link>
                      </p>
                    )}
                    {candidateError?.index === index && <ErrorText message={candidateError.message} />}
                  </article>
                );
              })}
              {result && <AiContextPanel items={result.context} />}
              {Object.values(candidateState).some((s) => s.status === "adopted") && (
                <Notice tone="success">
                  下一步：<Link to={`/units/${unitId}/core`}>到 CoRe 寫這些學習重點的八題</Link>
                </Notice>
              )}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
