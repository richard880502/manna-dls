import { useEffect, useId, useState } from "react";
import { Link, useNavigate } from "react-router";
import { api, errorMessage } from "../api/client";
import type { CreateWorksheetRequest, MaterialDetail, Material, WorksheetSummary } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { MaterialAddForm, MaterialPicker, useMaterialLibrary } from "../components/material/MaterialLibrary";
import { TemplateSelect, useReadyTemplates } from "../components/template/TemplateSelect";
import { withTextOnly } from "../lib/templates";
import { LEVEL_LABEL, formatDateTime } from "../lib/worksheet";

type Way = "upload" | "library" | "paste";

const WAYS: Array<{ key: Way; label: string }> = [
  { key: "upload", label: "上傳檔案" },
  { key: "library", label: "從教材庫選" },
  { key: "paste", label: "貼上文字" },
];

type Picked = Pick<Material, "id" | "title">;

/** 上傳檔案 and 從教材庫選: both end with one library material that has text. */
function LibraryStep({ way, onPicked }: { way: "upload" | "library"; onPicked: (m: Picked) => void }) {
  const { library, error, reload } = useMaterialLibrary();
  const [problem, setProblem] = useState<string | null>(null);

  function take(m: Pick<Material, "id" | "title" | "text_status">) {
    if (m.text_status !== "ready") {
      setProblem("這份檔案讀不到文字（可能是掃描檔），Manna 沒辦法照它出題。請換一份，或改用貼上文字。");
      return;
    }
    setProblem(null);
    onPicked(m);
  }

  if (error) {
    return (
      <div>
        <ErrorText message={error} />
        <button type="button" className="text-link is-plain" onClick={reload}>
          再試一次
        </button>
      </div>
    );
  }
  if (!library) return <Loading />;
  return (
    <>
      {way === "upload" ? (
        <MaterialAddForm way="upload" library={library} onAdded={(m: MaterialDetail) => take(m)} />
      ) : (
        <MaterialPicker library={withTextOnly(library)} currentId={null} busy={false} onPick={take} />
      )}
      <ErrorText message={problem} />
    </>
  );
}

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
  const [way, setWay] = useState<Way>("upload");
  const [picked, setPicked] = useState<Picked | null>(null);
  const [title, setTitle] = useState("");
  const [text, setText] = useState("");
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
    if (way === "paste") {
      body = { material: { title: title.trim(), text } };
    } else if (picked) {
      body = { material_id: picked.id };
    } else {
      setCreateError(way === "upload" ? "請先上傳教材檔案。" : "請先從教材庫選一份教材。");
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
          <div className="material-ways" role="group" aria-label="教材從哪裡來">
            {WAYS.map((w) => (
              <button
                key={w.key}
                type="button"
                className="way-button"
                aria-pressed={way === w.key}
                onClick={() => {
                  setWay(w.key);
                  setPicked(null);
                  setCreateError(null);
                }}
              >
                {w.label}
              </button>
            ))}
          </div>

          {way !== "paste" &&
            (picked ? (
              <p className="ws-picked" role="status">
                教材：「{picked.title.trim() || "未命名教材"}」
                <button type="button" className="text-link is-plain" onClick={() => setPicked(null)}>
                  換一份
                </button>
              </p>
            ) : (
              <div role="region" aria-label={way === "upload" ? "上傳檔案" : "從教材庫選"}>
                <LibraryStep key={way} way={way} onPicked={setPicked} />
              </div>
            ))}

          {way === "paste" && (
            <div role="region" aria-label="貼上文字">
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
            </div>
          )}

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
