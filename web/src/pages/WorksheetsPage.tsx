import { useEffect, useId, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { CreateWorksheetRequest, WorksheetSummary } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import {
  MaterialChooser,
  emptyMaterialChoice,
  materialFields,
  missingMaterialLine,
} from "../components/material/MaterialChooser";
import { TemplateSelect, useReadyTemplates } from "../components/template/TemplateSelect";
import { LEVEL_LABEL, formatDateTime } from "../lib/worksheet";

/**
 * 學習單: make one directly (no unit, no lesson needed; spec §4.2) from an uploaded file, a
 * library material or pasted text, optionally following one of the teacher's templates; and the
 * worksheets made so far. Worksheets made from a lesson start on the lesson page.
 */
export function WorksheetsPage() {
  const ids = useId();
  const navigate = useNavigate();
  const [list, setList] = useState<WorksheetSummary[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [choice, setChoice] = useState(emptyMaterialChoice);
  const templates = useReadyTemplates();
  const [templateId, setTemplateId] = useState<number | null>(null);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<number | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .listWorksheets()
      .then((rows) => alive && setList(rows))
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, []);

  async function create() {
    let body: CreateWorksheetRequest;
    const fields = materialFields(choice);
    if (choice.way === "paste") {
      // Pasted text may stay empty here: the worksheet page asks for it next.
      body = { material: { title: choice.title.trim(), text: choice.text } };
    } else if (fields) {
      body = fields;
    } else {
      setCreateError(missingMaterialLine(choice));
      return;
    }
    if (templateId !== null) body.template_id = templateId;
    setCreating(true);
    setCreateError(null);
    try {
      const created = await api.createWorksheet(body);
      navigate(`/worksheets/${created.id}`);
    } catch (err) {
      setCreateError(errorMessage(err));
      setCreating(false);
    }
  }

  async function remove(id: number) {
    setDeleteError(null);
    try {
      await api.deleteWorksheet(id);
      setList((rows) => rows?.filter((r) => r.id !== id) ?? null);
      setConfirming(null);
    } catch (err) {
      setDeleteError(errorMessage(err));
    }
  }

  return (
    <>
      <div className="page-head">
        <h1>學習單</h1>
        <p className="lead">
          放進一篇教材，選想要的任務和作答方式（都不選也可以），Manna 會產生一份可以修改、可以匯出的學習單，學生版和教師參考版一起。也可以照你自己的學習單版型做，或從課例頁按「產生學習單」，把那節課的目標與任務帶進來。
        </p>
      </div>

      <section className="section" aria-labelledby={`${ids}-new`}>
        <h2 id={`${ids}-new`}>直接做一份</h2>
        <div className="ws-new-form">
          <MaterialChooser
            choice={choice}
            onChange={(next) => {
              setChoice(next);
              setCreateError(null);
            }}
            pasteHint="段落之間空一行。請不要貼上學生姓名或其他個人資料。也可以先不貼，下一步再補。"
          />

          {templates !== null && templates.length > 0 && (
            <>
              <label className="field-label" htmlFor={`${ids}-template`}>
                照我的版型 <span className="optional-label">可以不選</span>
              </label>
              <TemplateSelect
                id={`${ids}-template`}
                templates={templates}
                value={templateId}
                describedBy={`${ids}-template-hint`}
                onChange={setTemplateId}
              />
              <p className="field-hint" id={`${ids}-template-hint`}>
                選了版型，Manna 會照它的區塊與題型出題；任務與作答方式也會先照版型勾好，可以再改。
              </p>
            </>
          )}
          {templates !== null && templates.length === 0 && (
            <p className="quiet-note">
              想照你自己的學習單格式做？到<Link to="/materials">教材</Link>上傳一份做過的學習單，就能建立版型。
            </p>
          )}

          <div className="actions">
            <button type="button" className="primary-button" disabled={creating} onClick={() => void create()}>
              {creating ? "建立中…" : "下一步：設定學習單"}
            </button>
          </div>
          <ErrorText message={createError} />
        </div>
      </section>

      <section className="section" aria-labelledby={`${ids}-list`}>
        <h2 id={`${ids}-list`}>做過的學習單</h2>
        <ErrorText message={loadError} />
        {list === null && !loadError && <Loading />}
        {list !== null && list.length === 0 && <p className="empty-line">還沒有學習單。做好的會列在這裡。</p>}
        {list !== null && list.length > 0 && (
          <ul className="rows">
            {list.map((w) => (
              <li key={w.id} className="row">
                <div>
                  <h3>
                    <Link to={`/worksheets/${w.id}`}>{w.title || "未命名的學習單"}</Link>
                  </h3>
                  <p className="quiet-note">
                    {w.lesson ? `${w.lesson.unit_title} / ${w.lesson.title}` : "獨立學習單"}
                    {" · "}
                    {w.generated
                      ? w.levels.length > 0
                        ? w.levels.map((l) => LEVEL_LABEL[l]).join("、")
                        : "已產生"
                      : "還沒產生"}
                    {" · "}
                    {formatDateTime(w.updated_at)} 更新
                  </p>
                </div>
                <div className="row-tools">
                  {confirming === w.id ? (
                    <span role="group" aria-label="確認刪除">
                      確定刪除？
                      <button type="button" className="text-link is-danger" onClick={() => void remove(w.id)}>
                        確定刪除
                      </button>
                      <button type="button" className="text-link is-plain" onClick={() => setConfirming(null)}>
                        不刪
                      </button>
                    </span>
                  ) : (
                    <button
                      type="button"
                      className="text-link is-plain"
                      aria-label={`刪除 ${w.title || "未命名的學習單"}`}
                      onClick={() => setConfirming(w.id)}
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
      </section>
    </>
  );
}
