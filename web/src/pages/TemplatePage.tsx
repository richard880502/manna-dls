import { useCallback, useEffect, useId, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { WsTemplate, WsTemplateDetail, WsTemplateSection } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { LeaveConfirm, useLeaveGuard } from "../components/LeaveGuard";
import { MaterialViewer } from "../components/material/MaterialViewer";
import { ExtractingLine } from "../components/template/ExtractingLine";
import { COPY_TEMPLATE, USE_TEMPLATE, VIEW_SOURCE } from "../components/library/labels";
import { useApp } from "../context/AppContext";
import { can } from "../lib/permissions";
import { mimicTemplatePath } from "../lib/create";
import {
  MAX_ITEMS,
  MIN_ITEMS,
  moveItem,
  notesFromText,
  notesProblem,
  sectionLine,
  templateConfig,
  templateTitle,
} from "../lib/templates";
import { ANSWER_FORMATS, BLOCK_KIND_LABEL, toggleIn } from "../lib/worksheet";

const ITEM_COUNTS = Array.from({ length: MAX_ITEMS - MIN_ITEMS + 1 }, (_, i) => MIN_ITEMS + i);

/** Where 我的範本 is listed (the template page's way back, and where a deleted one leads). */
const MY_TEMPLATES = "/templates?tab=mine";

/**
 * /templates/:id — check and edit one of my templates (saving makes a new version). A 平台範本 is
 * read only for teachers: 照這個範本做, 另存成我的範本 (a personal copy to edit) and 看原檔. Only
 * the knowledge-base managers (pi, kb_manager) edit a platform template here.
 */
export function TemplatePage() {
  const { templateId } = useParams();
  const id = Number(templateId);
  if (!Number.isInteger(id) || id <= 0) return <ErrorText message="找不到這個版型。" />;
  return <TemplateWorkspace key={id} id={id} />;
}

interface Draft {
  title: string;
  sections: WsTemplateSection[];
  /** Style notes as the teacher types them: one per line. */
  notesText: string;
}

const draftOf = (d: WsTemplateDetail): Draft | null =>
  d.content ? { title: d.title, sections: d.content.sections, notesText: d.content.style_notes.join("\n") } : null;

const sameJson = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

function TemplateWorkspace({ id }: { id: number }) {
  const ids = useId();
  const navigate = useNavigate();
  const { teacher } = useApp();
  const manager = can(teacher, "manage_knowledge");
  const [detail, setDetail] = useState<WsTemplateDetail | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loadSeq, setLoadSeq] = useState(0);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [savedLine, setSavedLine] = useState<string | null>(null);
  const [retrying, setRetrying] = useState(false);
  const [retryError, setRetryError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [viewing, setViewing] = useState(false);

  /** Takes what the server says; the editor starts over from it. */
  const take = useCallback((next: WsTemplateDetail) => {
    setDetail(next);
    setDraft(draftOf(next));
  }, []);

  useEffect(() => {
    let alive = true;
    api
      .getWorksheetTemplate(id)
      .then((loaded) => alive && take(loaded))
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [id, loadSeq, take]);

  // While Manna reads the worksheet, ask again every few seconds.
  const extracting = detail?.status === "extracting";
  useEffect(() => {
    if (!extracting) return undefined;
    let stopped = false;
    const timer = setInterval(() => {
      api
        .getWorksheetTemplate(id)
        .then((next) => {
          if (!stopped && next.status !== "extracting") take(next);
        })
        .catch(() => {
          // A missed poll is not a failure; the next one tries again.
        });
    }, templateConfig.pollMs);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [extracting, id, take]);

  const content = detail?.content ?? null;
  const notes = draft ? notesFromText(draft.notesText) : [];
  const titleChanged = draft !== null && detail !== null && draft.title.trim() !== detail.title;
  const contentChanged =
    draft !== null &&
    content !== null &&
    (!sameJson(draft.sections, content.sections) || !sameJson(notes, content.style_notes));
  const dirty = titleChanged || contentChanged;
  const guard = useLeaveGuard(dirty);

  const edit = (change: (d: Draft) => Draft) => {
    setSavedLine(null);
    setDraft((d) => (d ? change(d) : d));
  };
  const editSection = (sectionId: string, change: (s: WsTemplateSection) => WsTemplateSection) =>
    edit((d) => ({ ...d, sections: d.sections.map((s) => (s.id === sectionId ? change(s) : s)) }));

  async function save(): Promise<string | null> {
    if (!draft || !content || !dirty) return null;
    const problem = notesProblem(notes);
    if (problem) {
      setSaveError(problem);
      return problem;
    }
    if (titleChanged && draft.title.trim() === "") {
      const message = "請幫版型取個名字。";
      setSaveError(message);
      return message;
    }
    setSaving(true);
    setSaveError(null);
    try {
      const nextContent: WsTemplate = { ...content, sections: draft.sections, style_notes: notes };
      const saved = await api.updateWorksheetTemplate(id, {
        ...(titleChanged ? { title: draft.title.trim() } : {}),
        ...(contentChanged ? { content: nextContent } : {}),
      });
      take(saved);
      setSavedLine(contentChanged && saved.version_no ? `已存成第 ${saved.version_no} 版` : "已儲存名稱");
      return null;
    } catch (err) {
      const message = errorMessage(err);
      setSaveError(message);
      return message;
    } finally {
      setSaving(false);
    }
  }

  async function retry() {
    setRetrying(true);
    setRetryError(null);
    try {
      take(await api.retryWorksheetTemplate(id));
    } catch (err) {
      setRetryError(errorMessage(err));
    } finally {
      setRetrying(false);
    }
  }

  async function remove() {
    setDeleting(true);
    setDeleteError(null);
    try {
      await api.deleteWorksheetTemplate(id);
      guard.release();
      navigate(detail?.scope === "platform" ? "/templates" : MY_TEMPLATES);
    } catch (err) {
      setDeleteError(errorMessage(err));
      setDeleting(false);
    }
  }

  if (loadError) {
    return (
      <>
        <ErrorText message={loadError} />
        <p className="after-rows">
          <button
            type="button"
            className="text-link is-plain"
            onClick={() => {
              setLoadError(null);
              setLoadSeq((n) => n + 1);
            }}
          >
            再試一次
          </button>
          　<Link to="/templates">回學習單範本</Link>
        </p>
      </>
    );
  }
  if (!detail) return <Loading />;
  const platform = detail.scope === "platform";
  if (platform && !manager) return <PlatformTemplate detail={detail} />;

  const source = detail.source_material;

  return (
    <>
      <div className="head-row page-head">
        <div>
          <p className="crumbs">
            <Link to="/templates">學習單範本</Link> /{" "}
            {platform ? <Link to="/templates">平台範本</Link> : <Link to={MY_TEMPLATES}>我的範本</Link>}
          </p>
          <h1>{templateTitle(detail)}</h1>
          {platform && <p className="quiet-note">這是平台範本：你在這裡的修改，所有老師都會看到。</p>}
        </div>
        {detail.status === "ready" && detail.version_no !== null && (
          <p className="saved" role="status">
            {savedLine ?? `第 ${detail.version_no} 版`}
          </p>
        )}
      </div>

      <p className="quiet-note template-source">
        {source ? (
          <>
            從「{source.title.trim() || "未命名教材"}」讀出來的。
            <button
              type="button"
              className="text-link is-plain"
              aria-expanded={viewing}
              aria-controls="material-viewer"
              onClick={() => setViewing((v) => !v)}
            >
              看原本的學習單
            </button>
          </>
        ) : (
          "原本的檔案已經刪除；範本本身還在，可以照常使用。"
        )}
      </p>

      {detail.status === "extracting" && (
        <div className="manna-well manna-progress">
          <ExtractingLine />
          <p className="why">你可以先離開這一頁，讀好了會留在「學習單範本」的「我的範本」裡。</p>
        </div>
      )}

      {detail.status === "failed" && (
        <section className="section template-failed" aria-label="沒有讀成功">
          <p className="error-text" role="alert">
            Manna 沒有讀成功這份學習單的設計{detail.error ? `：${detail.error}` : "。"}
          </p>
          <div className="button-row">
            <button type="button" className="primary-button" disabled={retrying} onClick={() => void retry()}>
              {retrying ? "送出中…" : "再讀一次"}
            </button>
          </div>
          <ErrorText message={retryError} />
        </section>
      )}

      {detail.status === "ready" && content && draft && (
        <form
          className="template-editor"
          aria-label="編修版型"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <TemplateFacts content={content} />

          <label className="field-label" htmlFor={`${ids}-title`}>
            版型名稱
          </label>
          <input
            id={`${ids}-title`}
            type="text"
            maxLength={200}
            value={draft.title}
            onChange={(e) => {
              const title = e.target.value;
              edit((d) => ({ ...d, title }));
            }}
          />

          <h2 className="template-sections-head">學習單的區塊</h2>
          <p className="quiet-note">由上到下就是新學習單的順序。標題裡的 [n]、[主題] 會換成新課的內容。</p>
          <ol className="template-sections">
            {draft.sections.map((s, i) => (
              <SectionCard
                key={s.id}
                section={s}
                index={i}
                count={draft.sections.length}
                disabled={saving}
                onChange={(change) => editSection(s.id, change)}
                onMove={(to) => edit((d) => ({ ...d, sections: moveItem(d.sections, i, to) }))}
                onDelete={() => edit((d) => ({ ...d, sections: d.sections.filter((x) => x.id !== s.id) }))}
              />
            ))}
          </ol>

          <label className="field-label" htmlFor={`${ids}-notes`}>
            風格備註
          </label>
          <textarea
            id={`${ids}-notes`}
            rows={Math.max(3, Math.min(8, draft.notesText.split("\n").length + 1))}
            value={draft.notesText}
            aria-describedby={`${ids}-notes-hint`}
            disabled={saving}
            onChange={(e) => {
              const notesText = e.target.value;
              edit((d) => ({ ...d, notesText }));
            }}
          />
          <p className="field-hint" id={`${ids}-notes-hint`}>
            一行一則，最多 8 則，每則 200 字以內。例如：題目指示一律用英文、每題留兩行作答空間。
          </p>

          <div className="button-row template-save">
            <button type="submit" className="primary-button" disabled={saving || !dirty}>
              {saving ? "儲存中…" : "儲存版型"}
            </button>
            {dirty && !saving && (
              <button type="button" className="text-link is-plain" onClick={() => take(detail)}>
                放棄修改
              </button>
            )}
          </div>
          <p className="field-hint">每次儲存都會多一個版本；已經做好的學習單不會跟著改。</p>
          <ErrorText message={saveError} />
        </form>
      )}

      <section className="section template-danger" aria-label="刪除版型">
        {confirmDelete ? (
          <div className="material-delete" role="alertdialog" aria-label="確認刪除版型">
            <p>
              <strong>刪除「{templateTitle(detail)}」？</strong>
            </p>
            <p>刪除後無法復原：這個版型的所有版本都會刪除。用它做過的學習單會留著，只是不再連到版型；原本上傳的檔案會留著。</p>
            <div className="button-row">
              <button type="button" className="danger-button" disabled={deleting} onClick={() => void remove()}>
                {deleting ? "刪除中…" : "永久刪除"}
              </button>
              <button
                type="button"
                className="text-link is-plain"
                disabled={deleting}
                onClick={() => setConfirmDelete(false)}
              >
                取消
              </button>
            </div>
            <ErrorText message={deleteError} />
          </div>
        ) : (
          <button type="button" className="text-link is-plain" onClick={() => setConfirmDelete(true)}>
            刪除這個版型
          </button>
        )}
      </section>

      {source && (
        <MaterialViewer mode="panel" materialId={source.id} open={viewing} onClose={() => setViewing(false)} />
      )}
      <LeaveConfirm guard={guard} message="離開這頁，還沒儲存的版型修改就會遺失。" onSave={save} />
    </>
  );
}

/** A 平台範本: what it asks for, section by section, read only; a personal copy can be edited. */
function PlatformTemplate({ detail }: { detail: WsTemplateDetail }) {
  const navigate = useNavigate();
  const [viewing, setViewing] = useState(false);
  const [copying, setCopying] = useState(false);
  const [copyError, setCopyError] = useState<string | null>(null);
  const closeViewer = useCallback(() => setViewing(false), []);
  const source = detail.source_material;
  const content = detail.content;
  const ready = detail.status === "ready";

  async function copy() {
    setCopying(true);
    setCopyError(null);
    try {
      const mine = await api.copyWorksheetTemplate(detail.id);
      navigate(`/templates/${mine.id}`);
    } catch (err) {
      setCopyError(errorMessage(err));
      setCopying(false);
    }
  }

  return (
    <>
      <div className="page-head">
        <p className="crumbs">
          <Link to="/templates">學習單範本</Link> / 平台範本
        </p>
        <h1>{templateTitle(detail)}</h1>
        <p className="quiet-note">這是平台範本，只能看、不能改。想照自己的習慣調整，先另存成我的範本。</p>
      </div>

      <div className="button-row template-platform-actions">
        {ready ? (
          <Link className="primary-button" to={mimicTemplatePath(detail.id)}>
            {USE_TEMPLATE}
          </Link>
        ) : (
          <button type="button" className="primary-button" disabled>
            {USE_TEMPLATE}
          </button>
        )}
        <button type="button" className="secondary-button" disabled={copying || !ready} onClick={() => void copy()}>
          {copying ? "另存中…" : COPY_TEMPLATE}
        </button>
        {source && (
          <button
            type="button"
            className="text-link is-plain"
            aria-expanded={viewing}
            aria-controls="material-viewer"
            onClick={() => setViewing((v) => !v)}
          >
            {VIEW_SOURCE}
          </button>
        )}
      </div>
      <ErrorText message={copyError} />

      {detail.status === "extracting" && (
        <div className="manna-well manna-progress">
          <ExtractingLine />
        </div>
      )}
      {detail.status === "failed" && <p className="quiet-note">這個範本還沒有讀成功，暫時不能使用。</p>}

      {ready && content && (
        <section className="template-readonly" aria-label="範本內容">
          <TemplateFacts content={content} />
          <h2 className="template-sections-head">學習單的區塊</h2>
          <p className="quiet-note">由上到下就是新學習單的順序。標題裡的 [n]、[主題] 會換成新課的內容。</p>
          <ol className="template-sections is-readonly">
            {content.sections.map((s, i) => (
              <li key={s.id} className="template-section" aria-label={`第 ${i + 1} 區`}>
                <h3>
                  <span className="num">{i + 1}.</span>
                  <span className="soft-tag">{BLOCK_KIND_LABEL[s.block_kind] ?? s.block_kind}</span>
                </h3>
                <p className="template-section-line" lang="en">
                  {sectionLine(s)}
                </p>
                {s.purpose.trim() && <p className="quiet-note">用途：{s.purpose}</p>}
                {s.answer_formats.length > 0 && <p className="meta">作答方式：{s.answer_formats.join("、")}</p>}
              </li>
            ))}
          </ol>
          {content.style_notes.length > 0 && (
            <>
              <h2 className="template-sections-head">風格備註</h2>
              <ul className="template-notes">
                {content.style_notes.map((n, i) => (
                  // oxlint-disable-next-line react/no-array-index-key -- read-only strings
                  <li key={i}>{n}</li>
                ))}
              </ul>
            </>
          )}
        </section>
      )}

      {source && (
        <MaterialViewer mode="panel" materialId={source.id} open={viewing} onClose={closeViewer} />
      )}
    </>
  );
}

/** The template's header facts, compact and read-only. */
function TemplateFacts({ content }: { content: WsTemplate }) {
  const facts = [
    content.worksheet_kind.trim(),
    `指示語：${content.instruction_language}`,
    content.header_fields.length > 0 ? `抬頭：${content.header_fields.join("、")}` : "",
    content.shows_objectives ? "列出學習目標" : "不列學習目標",
    content.includes_passage_text ? "附課文" : "不附課文",
  ].filter((f) => f !== "");
  return (
    <div className="template-facts">
      <p className="meta">{facts.join(" · ")}</p>
      {content.generation_options.length > 0 && (
        <p className="meta">產生設定：{content.generation_options.join("、")}</p>
      )}
    </div>
  );
}

interface SectionCardProps {
  section: WsTemplateSection;
  index: number;
  count: number;
  disabled: boolean;
  onChange: (change: (s: WsTemplateSection) => WsTemplateSection) => void;
  onMove: (to: number) => void;
  onDelete: () => void;
}

/** One section of the template: what the teacher may change, and what Manna read (read-only). */
function SectionCard({ section: s, index, count, disabled, onChange, onMove, onDelete }: SectionCardProps) {
  const ids = useId();
  const [more, setMore] = useState(false);
  const name = `第 ${index + 1} 區`;
  const itemCount = Math.min(MAX_ITEMS, Math.max(MIN_ITEMS, s.item_count));
  const hasMore =
    s.item_patterns.length > 0 || s.student_tasks.length > 0 || s.scaffolds.length > 0 || s.text_scope.trim() !== "";

  return (
    <li className="template-section" aria-label={name}>
      <div className="template-section-head">
        <h3>
          <span className="num">{index + 1}.</span>
          <span className="soft-tag">{BLOCK_KIND_LABEL[s.block_kind] ?? s.block_kind}</span>
        </h3>
        <div className="row-tools">
          <button
            type="button"
            className="text-link is-plain"
            aria-label={`上移${name}`}
            disabled={disabled || index === 0}
            onClick={() => onMove(index - 1)}
          >
            上移
          </button>
          <button
            type="button"
            className="text-link is-plain"
            aria-label={`下移${name}`}
            disabled={disabled || index === count - 1}
            onClick={() => onMove(index + 1)}
          >
            下移
          </button>
          <button
            type="button"
            className="text-link is-plain"
            aria-label={`刪除${name}`}
            disabled={disabled || count <= 1}
            onClick={onDelete}
          >
            刪除
          </button>
        </div>
      </div>

      <label className="field-label" htmlFor={`${ids}-title`}>
        標題寫法
      </label>
      <input
        id={`${ids}-title`}
        type="text"
        maxLength={200}
        value={s.title_pattern}
        disabled={disabled}
        onChange={(e) => {
          const title_pattern = e.target.value;
          onChange((x) => ({ ...x, title_pattern }));
        }}
      />
      {s.purpose.trim() && <p className="quiet-note">用途：{s.purpose}</p>}

      <div className="template-toggles">
        <label className="check-row">
          <input
            type="checkbox"
            checked={s.per_paragraph}
            disabled={disabled}
            onChange={(e) => {
              const per_paragraph = e.target.checked;
              onChange((x) => ({ ...x, per_paragraph }));
            }}
          />
          逐段（每段各做一次）
        </label>
        <label className="check-row">
          <input
            type="checkbox"
            checked={s.needs_external_material}
            disabled={disabled}
            onChange={(e) => {
              const needs_external_material = e.target.checked;
              onChange((x) => ({ ...x, needs_external_material }));
            }}
          />
          需要我提供素材
        </label>
      </div>

      <div role="group" aria-label={`作答方式（${name}）`}>
        <p className="ws-sub">作答方式</p>
        <div className="ws-chips">
          {ANSWER_FORMATS.map((label) => (
            <label key={label} className="ws-chip">
              <input
                type="checkbox"
                checked={s.answer_formats.includes(label)}
                disabled={disabled}
                onChange={() =>
                  onChange((x) => ({ ...x, answer_formats: toggleIn(x.answer_formats, label, ANSWER_FORMATS) }))
                }
              />
              <span>{label}</span>
            </label>
          ))}
        </div>
      </div>

      <div className="template-fields">
        <label className="field-label" htmlFor={`${ids}-count`}>
          題數{s.per_paragraph ? "（每段）" : ""}
        </label>
        <select
          id={`${ids}-count`}
          value={itemCount}
          disabled={disabled}
          onChange={(e) => {
            const item_count = Number(e.target.value);
            onChange((x) => ({ ...x, item_count }));
          }}
        >
          {ITEM_COUNTS.map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
      </div>

      <label className="field-label" htmlFor={`${ids}-style`}>
        指示語寫法
      </label>
      <input
        id={`${ids}-style`}
        type="text"
        maxLength={200}
        value={s.instruction_style}
        disabled={disabled}
        onChange={(e) => {
          const instruction_style = e.target.value;
          onChange((x) => ({ ...x, instruction_style }));
        }}
      />

      {hasMore && (
        <>
          <p className="template-more-toggle">
            <button
              type="button"
              className="text-link is-plain"
              aria-expanded={more}
              aria-controls={`${ids}-more`}
              onClick={() => setMore((m) => !m)}
            >
              {more ? "收起" : "看 Manna 讀到的細節"}
            </button>
          </p>
          {more && (
            <dl className="template-more" id={`${ids}-more`}>
              {s.text_scope.trim() && (
                <>
                  <dt>範圍</dt>
                  <dd>{s.text_scope}</dd>
                </>
              )}
              {s.student_tasks.length > 0 && (
                <>
                  <dt>學生任務</dt>
                  <dd>{s.student_tasks.join("、")}</dd>
                </>
              )}
              {s.scaffolds.length > 0 && (
                <>
                  <dt>鷹架</dt>
                  <dd>{s.scaffolds.join("、")}</dd>
                </>
              )}
              {s.item_patterns.length > 0 && (
                <>
                  <dt>題型範例</dt>
                  <dd>
                    <ul>
                      {s.item_patterns.map((p, i) => (
                        // oxlint-disable-next-line react/no-array-index-key -- read-only strings
                        <li key={i}>{p}</li>
                      ))}
                    </ul>
                  </dd>
                </>
              )}
            </dl>
          )}
        </>
      )}
    </li>
  );
}
