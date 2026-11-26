import type { WorksheetContent, WsJob } from "../../../api/types";
import { NOTES_HINT } from "../../../lib/blockRevise";
import { AUTO_FIELD_LABEL, LEVEL_LABEL } from "../../../lib/worksheet";
import { AiContextPanel } from "../../AiContextPanel";
import { ToolbarPopover } from "./ToolbarPopover";

/** 「這次由 Manna 決定：…」: the settings left to Manna (null: none). */
export function chosenLine(content: WorksheetContent): string | null {
  const chosen = content.chosen;
  if (!chosen || chosen.auto.length === 0) return null;
  const parts = chosen.auto.map((field) => {
    const s = chosen.settings;
    const values =
      field === "levels"
        ? s.levels.map((l) => LEVEL_LABEL[l])
        : field === "minutes"
          ? [`主要任務約 ${chosen.item_budget} 題`]
          : s[field];
    return `${AUTO_FIELD_LABEL[field]}：${values.length > 0 ? values.join("、") : "依教材判斷"}`;
  });
  return `這次由 Manna 決定：${parts.join("；")}。`;
}

/** ⓘ: what Manna was given and what it decided for this worksheet, and what a draft is. */
export function InfoPopover({
  content,
  jobs,
  teacherView,
}: {
  content: WorksheetContent;
  jobs: WsJob[];
  teacherView: boolean;
}) {
  const chosen = chosenLine(content);
  const context = jobs.find((j) => j.context.length > 0)?.context ?? [];
  return (
    <ToolbarPopover
      label="ⓘ"
      ariaLabel="關於這份學習單"
      panelLabel="關於這份學習單"
      panelClassName="ws-info-panel"
      className="ws-bar-tool ws-bar-info"
    >
      <p className="manna-label ws-draft-note">
        <span className="star" aria-hidden="true" />
        Manna 產生的草稿。直接點文字就能改；每個區塊都可以移動、重新產生或刪除，也可以按「改這一區」或加註記，請 Manna 只改那一區。
        {teacherView ? "參考答案只在教師版。" : ""}
      </p>
      <p className="field-hint">{NOTES_HINT}</p>
      {chosen && <p className="quiet-note ws-chosen">{chosen}</p>}
      {content.chosen?.reasons.map((r) => (
        <p key={r} className="quiet-note">
          {r}
        </p>
      ))}
      <AiContextPanel items={context} />
    </ToolbarPopover>
  );
}
