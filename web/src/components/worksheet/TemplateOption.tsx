import { useEffect, useId, useState } from "react";
import { api } from "../../api/client";
import type { WsTemplateDetail, WsTemplateSummary } from "../../api/types";
import { sectionLine } from "../../lib/templates";
import { TemplateSelect, useReadyTemplates } from "../template/TemplateSelect";
import type { WorksheetController } from "./useWorksheet";

/**
 * 照我的版型 on the settings view: pick one of the teacher's templates (the server re-derives the
 * settings from it) or none. With a template, its sections are listed read-only, in order. Shown
 * only when the teacher has a template to choose or the worksheet already follows one.
 */
export function TemplateOption({ ws, locked }: { ws: WorksheetController; locked: boolean }) {
  const ids = useId();
  const ready = useReadyTemplates();
  const current = ws.state?.template ?? null;
  const currentId = current?.id ?? null;
  const [detail, setDetail] = useState<WsTemplateDetail | null>(null);

  useEffect(() => {
    if (currentId === null) return undefined;
    let alive = true;
    api
      .getWorksheetTemplate(currentId)
      .then((d) => alive && setDetail(d))
      .catch(() => alive && setDetail(null));
    return () => {
      alive = false;
    };
  }, [currentId]);

  if (ready === null) return null;
  if (ready.length === 0 && !current) return null;

  // The worksheet's template is always offered, even if it is not in the ready list any more.
  const options: WsTemplateSummary[] =
    current && !ready.some((t) => t.id === current.id)
      ? [
          {
            id: current.id,
            title: current.title,
            status: "ready",
            section_count: 0,
            source_material: null,
            version_no: current.version_no,
            updated_at: "",
          },
          ...ready,
        ]
      : ready;

  const shown = detail && current && detail.id === current.id ? detail : null;
  const newer = shown && shown.version_no !== null && shown.version_no !== current?.version_no ? shown.version_no : null;
  const sections = shown?.content?.sections ?? [];

  return (
    <section className="ws-opt ws-template-opt" aria-labelledby={`${ids}-h`}>
      <h2 id={`${ids}-h`}>照我的版型</h2>
      <div>
        <label className="sr-only" htmlFor={`${ids}-select`}>
          照我的版型
        </label>
        <TemplateSelect
          id={`${ids}-select`}
          templates={options}
          value={currentId}
          disabled={locked}
          describedBy={`${ids}-hint`}
          onChange={(templateId) => void ws.setTemplate(templateId)}
        />
        <p className="field-hint" id={`${ids}-hint`}>
          選了版型，上面的任務與作答方式會照版型重新勾好；改回「不使用版型」時保留目前的勾選。
        </p>
        {current && newer !== null && (
          <p className="quiet-note">
            這份學習單用的是第 {current.version_no} 版；版型後來改成第 {newer} 版。
            <button
              type="button"
              className="text-link is-plain"
              disabled={locked}
              onClick={() => void ws.setTemplate(current.id)}
            >
              改用第 {newer} 版
            </button>
          </p>
        )}
        {current && newer === null && sections.length > 0 && (
          <div className="ws-template-skeleton">
            <p className="ws-sub">會依這個版型產生：</p>
            <ol aria-label="版型的區塊">
              {sections.map((s) => (
                <li key={s.id}>{sectionLine(s)}</li>
              ))}
            </ol>
          </div>
        )}
      </div>
    </section>
  );
}
