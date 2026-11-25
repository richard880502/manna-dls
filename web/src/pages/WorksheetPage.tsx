import { useEffect, useRef, useState } from "react";
import { MaterialTextDialog, ParagraphPickerDialog } from "../components/material/LongMaterial";
import { Link, useParams } from "react-router";
import { ErrorText, Loading } from "../components/common";
import { MemorySuggestions } from "../components/memory/MemorySuggestions";
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
  /** 修改課文 / 只選要用的段落, opened from the material column or from a message about the material. */
  const [fix, setFix] = useState<"edit" | "pick" | null>(null);

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
          　<Link to="/prep">回我的作品</Link>
        </p>
      </>
    );
  }

  const shown = view ?? (state.generated ? "result" : "options");
  /** The library entry the material came from: its text can be edited and paragraphs chosen. */
  const libraryId = state.material.source.material_id ?? null;
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
          {state.prep_session_id ? (
            <p className="crumbs prep-back">
              <Link to={`/prep/${state.prep_session_id}`}>← 回到備課對話</Link>
            </p>
          ) : null}
          <p className="crumbs">
            <Link to="/prep">我的作品</Link>
            {state.lesson && (
              <>
                {" "}
                / <Link to={`/units/${state.lesson.unit_id}`}>{state.lesson.unit_title}</Link> /{" "}
                <Link to={`/lessons/${state.lesson.id}`}>{state.lesson.title}</Link>
              </>
            )}
          </p>
          <h1>{shown === "options" ? "做一份學習單" : state.title || "未命名的學習單"}</h1>
          {state.remixed_from && (
            <p className="quiet-note ws-remix-line">
              改編自「{state.remixed_from.title.trim() || "未命名的學習單"}」（{state.remixed_from.owner_name}）
            </p>
          )}
          {state.template && (
            <p className="quiet-note ws-template-line">
              依版型：<Link to={`/templates/${state.template.id}`}>{state.template.title.trim() || "未命名的版型"}</Link>{" "}
              第 {state.template.version_no} 版
            </p>
          )}
        </div>
        <p className={`saved${ws.saveError ? " is-failed" : ""}`} role="status">
          {savedLine}
          {ws.conflict ? (
            <>
              {" "}
              <button type="button" className="text-link is-plain" onClick={ws.reload}>
                重新載入
              </button>
            </>
          ) : ws.saveError && (
            <>
              {" "}
              <button type="button" className="text-link is-plain" onClick={() => void ws.flush()}>
                再試一次
              </button>
            </>
          )}
        </p>
      </div>

      <MemorySuggestions floating filter={{ worksheetId: id }} watch={`${state.versions_count}:${state.running}`} />

      <div className="ws-split">
        <div className="ws-pane-material">
          {materialPane({
            material: state.material,
            onReplace: ws.saveMaterial,
            locked: state.running || ws.busy,
            ...(libraryId ? { onEditText: () => setFix("edit"), onPickParagraphs: () => setFix("pick") } : {}),
          })}
        </div>
        <div className="ws-main">
          {shown === "options" ? (
            <OptionsView
              ws={ws}
              onGenerated={() => setView(null)}
              onBack={state.generated ? () => setView("result") : undefined}
              {...(libraryId ? { onEditText: () => setFix("edit"), onPickParagraphs: () => setFix("pick") } : {})}
            />
          ) : (
            <ResultView ws={ws} onAdjust={() => setView("options")} />
          )}
        </div>
      </div>

      {fix === "edit" && libraryId && (
        <MaterialTextDialog
          materialId={libraryId}
          from="worksheet"
          onClose={() => setFix(null)}
          onSaved={async (detail) => {
            // The worksheet keeps its own copy: take the edited text again, keeping the chosen
            // paragraphs that are still there.
            const chosen = state.material.source.selected_ids ?? [];
            const still = chosen.filter((pid) => detail.paragraphs.some((p) => p.id === pid));
            await ws.saveSelection(still.length > 0 ? still : null);
          }}
        />
      )}
      {fix === "pick" && libraryId && (
        <ParagraphPickerDialog
          materialId={libraryId}
          initial={state.material.source.selected_ids ?? null}
          onClose={() => setFix(null)}
          onSave={async (ids) => {
            const ok = await ws.saveSelection(ids);
            if (!ok) throw new Error("沒有儲存成功，請再試一次。");
          }}
        />
      )}
    </>
  );
}
