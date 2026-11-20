import { LEVELS, LEVEL_LABEL } from "../../lib/worksheet";
import { paragraphRange } from "../../lib/sop";
import { Dialog } from "../Dialog";
import { ErrorText } from "../common";
import type { SopController } from "./useSop";

const list = (items: readonly string[] | undefined) => (items && items.length > 0 ? items.join("、") : "由 Manna 依教材判斷");

/**
 * What 產生學習單 will use, on one screen: the choices made in the steps. 產生 makes the worksheet
 * with exactly these (nothing is asked again); 調整設定 opens the generator's form with the same
 * choices filled in.
 */
export function WorksheetPlan({
  sop,
  busy,
  error,
  onGenerate,
  onAdjust,
  onClose,
}: {
  sop: SopController;
  busy: boolean;
  error: string | null;
  onGenerate: () => void;
  onAdjust: () => void;
  onClose: () => void;
}) {
  const settings = sop.worksheetSettings;
  const paragraphs = sop.material?.paragraphs ?? [];
  const blocks = sop.content.task_blocks.filter((b) => b.tasks.some((t) => t.text.trim()));
  const levels = (settings?.levels ?? []).length > 0 ? settings?.levels ?? [] : ["standard" as const];
  return (
    <Dialog title="用這些設定產生學習單" onClose={onClose} busy={busy} initialFocus=".worksheet-go">
      <div className="worksheet-summary">
        <p className="quiet-note">這些是你在各個步驟選的，產生時不會再問一次。</p>
        <dl>
          <dt>素材類型</dt>
          <dd>{list(settings?.material_types)}</dd>
          <dt>學生任務</dt>
          <dd>{list(settings?.student_tasks)}</dd>
          <dt>作答方式</dt>
          <dd>{list(settings?.answer_formats)}</dd>
          {blocks.length > 0 && (
            <>
              <dt>逐段的設計</dt>
              <dd>
                <ul>
                  {blocks.map((b) => (
                    <li key={b.id}>
                      {paragraphRange(paragraphs.filter((p) => b.paragraph_ids.includes(p.id))) || "不分段"}：
                      {list([...(b.student_tasks ?? []), ...(b.answer_formats ?? [])])}
                    </li>
                  ))}
                </ul>
              </dd>
            </>
          )}
          <dt>產生設定</dt>
          <dd>{list(settings?.generation)}</dd>
          <dt>版本</dt>
          <dd>{LEVELS.filter((l) => levels.includes(l)).map((l) => LEVEL_LABEL[l]).join("、")}</dd>
          <dt>Exit ticket</dt>
          <dd>{settings?.skip_exit_ticket ? "不放" : "放在學習單最後"}</dd>
        </dl>
        <ErrorText message={error} />
        <div className="button-row">
          <button type="button" className="primary-button worksheet-go" disabled={busy} onClick={onGenerate}>
            {busy ? "建立中…" : "產生"}
          </button>
          <button type="button" className="text-link" disabled={busy} onClick={onAdjust}>
            調整設定
          </button>
          <button type="button" className="text-link is-plain" disabled={busy} onClick={onClose}>
            取消
          </button>
        </div>
      </div>
    </Dialog>
  );
}
