import { useEffect, useId, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { WorksheetSummary } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { LEVEL_LABEL, formatDateTime } from "../lib/worksheet";

/**
 * 學習單: make one directly from pasted material (no unit, no lesson needed; spec §4.2), and the
 * worksheets made so far. Worksheets made from a lesson start on the lesson page.
 */
export function WorksheetsPage() {
  const ids = useId();
  const navigate = useNavigate();
  const [list, setList] = useState<WorksheetSummary[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [text, setText] = useState("");
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
    setCreating(true);
    setCreateError(null);
    try {
      const created = await api.createWorksheet({ material: { title: title.trim(), text } });
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
          貼上一篇教材，選想要的任務和作答方式（都不選也可以），Manna 會產生一份可以修改、可以匯出的學習單，學生版和教師參考版一起。也可以從課例頁按「產生學習單」，把那節課的目標與任務帶進來。
        </p>
      </div>

      <section className="section" aria-labelledby={`${ids}-new`}>
        <h2 id={`${ids}-new`}>直接做一份</h2>
        <form
          className="ws-new-form"
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <label className="field-label" htmlFor={`${ids}-title`}>
            教材標題 <span className="optional-label">可以不填</span>
          </label>
          <input id={`${ids}-title`} type="text" value={title} onChange={(e) => setTitle(e.target.value)} />
          <label className="field-label" htmlFor={`${ids}-text`}>
            貼上教材文字
          </label>
          <textarea
            id={`${ids}-text`}
            className="print-input"
            rows={8}
            lang="en"
            value={text}
            onChange={(e) => setText(e.target.value)}
            aria-describedby={`${ids}-text-hint`}
          />
          <p className="field-hint" id={`${ids}-text-hint`}>
            段落之間空一行。請不要貼上學生姓名或其他個人資料。也可以先不貼，下一步再補。
          </p>
          <div className="actions">
            <button type="submit" className="primary-button" disabled={creating}>
              {creating ? "建立中…" : "下一步：設定學習單"}
            </button>
          </div>
          <ErrorText message={createError} />
        </form>
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
