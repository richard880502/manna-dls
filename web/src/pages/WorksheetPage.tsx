import { useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router";
import { ErrorText, Loading } from "../components/common";
import { WorksheetMaterialPane, type MaterialPaneSlot } from "../components/worksheet/MaterialPane";
import { OptionsView } from "../components/worksheet/OptionsView";
import { ResultView } from "../components/worksheet/ResultView";
import { useWorksheet } from "../components/worksheet/useWorksheet";
import { formatClock } from "../lib/worksheet";

const defaultMaterialPane: MaterialPaneSlot = (props) => <WorksheetMaterialPane {...props} />;

interface WorksheetPageProps {
  /**
   * The material column. This is the seam for the materials branch: its `MaterialViewer` (with
   * annotations) is passed here and nothing else on the page changes. Default: the read-only
   * print view of [`WorksheetMaterialPane`].
   */
  materialPane?: MaterialPaneSlot;
}

export function WorksheetPage({ materialPane = defaultMaterialPane }: WorksheetPageProps) {
  const { worksheetId } = useParams();
  const id = Number(worksheetId);
  if (!Number.isInteger(id) || id <= 0) return <ErrorText message="找不到這份學習單。" />;
  return <WorksheetWorkspace key={id} id={id} materialPane={materialPane} />;
}

function WorksheetWorkspace({ id, materialPane }: { id: number; materialPane: MaterialPaneSlot }) {
  const ws = useWorksheet(id);
  const state = ws.state;
  /** null: follow the worksheet (options until it was generated, then the result). */
  const [view, setView] = useState<"options" | "result" | null>(null);

  // Whatever is still pending is saved when the page is left.
  const flushRef = useRef(ws.flush);
  useEffect(() => {
    flushRef.current = ws.flush;
  }, [ws.flush]);
  useEffect(() => () => void flushRef.current(), []);

  if (ws.status === "loading") return <Loading />;
  if (ws.status === "error" || !state) {
    return (
      <>
        <ErrorText message={ws.loadError ?? "找不到這份學習單。"} />
        <p className="after-rows">
          <button type="button" className="text-link is-plain" onClick={ws.reload}>
            再試一次
          </button>
          　<Link to="/worksheets">回學習單</Link>
        </p>
      </>
    );
  }

  const shown = view ?? (state.generated ? "result" : "options");
  const savedLine = ws.saveError
    ? `沒有存到：${ws.saveError}`
    : ws.saving
      ? "儲存中…"
      : ws.savedAt
        ? `已自動儲存 ${formatClock(ws.savedAt)}`
        : ws.dirty
          ? "尚未儲存"
          : `已自動儲存 ${formatClock(state.saved_at)}`;

  return (
    <>
      <div className="head-row page-head">
        <div>
          <p className="crumbs">
            <Link to="/worksheets">學習單</Link>
            {state.lesson && (
              <>
                {" "}
                / <Link to={`/units/${state.lesson.unit_id}`}>{state.lesson.unit_title}</Link> /{" "}
                <Link to={`/lessons/${state.lesson.id}`}>{state.lesson.title}</Link>
              </>
            )}
          </p>
          <h1>{shown === "options" ? "做一份學習單" : state.title || "未命名的學習單"}</h1>
        </div>
        <p className={`saved${ws.saveError ? " is-failed" : ""}`} role="status">
          {savedLine}
          {ws.saveError && (
            <>
              {" "}
              <button type="button" className="text-link is-plain" onClick={() => void ws.flush()}>
                再試一次
              </button>
            </>
          )}
        </p>
      </div>

      {shown === "options" && state.generated && (
        <p className="quiet-note">
          <button type="button" className="text-link is-plain" onClick={() => setView("result")}>
            回到學習單
          </button>
        </p>
      )}

      <div className="ws-split">
        <div className="ws-pane-material">
          {materialPane({
            material: state.material,
            onReplace: ws.saveMaterial,
            locked: state.running || ws.busy,
          })}
        </div>
        <div className="ws-main">
          {shown === "options" ? (
            <OptionsView ws={ws} onGenerated={() => setView(null)} />
          ) : (
            <ResultView ws={ws} onAdjust={() => setView("options")} />
          )}
        </div>
      </div>
    </>
  );
}
