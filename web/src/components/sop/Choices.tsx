import { useId } from "react";
import type { SopOptions, WsLevel } from "../../api/types";
import { emptyOptions } from "../../lib/sop";
import {
  GENERATION_SETTINGS,
  LEVELS,
  LEVEL_LABEL,
  MATERIAL_TYPES,
  VERSIONS_HEADING,
  toggleIn,
} from "../../lib/worksheet";
import { Chip, ChipRow } from "../worksheet/Chips";
import type { SopController } from "./useSop";

/**
 * The generator's own options, set where the lesson steps meet them (spec §7). What is ticked is
 * what a worksheet made from this lesson starts with; the star (✦) marks what the lesson's
 * content alone suggested. Choosing writes the choice into the SOP document (saved and versioned
 * like everything else); until she chooses, the default stands.
 */
function useChoices(sop: SopController) {
  const effective = sop.worksheetSettings;
  const suggested = sop.worksheetSuggested;
  const setOptions = (patch: Partial<SopOptions>) =>
    sop.update((c) => ({ ...c, options: { ...(c.options ?? emptyOptions()), ...patch } }));
  // What she chose in this session wins over the server's answer, which only catches up after a save.
  const chosen = sop.content.options ?? emptyOptions();
  return { effective, suggested, setOptions, chosen };
}

/** 素材類型 (step 1). */
export function MaterialTypeChoice({ sop }: { sop: SopController }) {
  const { effective, suggested, setOptions, chosen } = useChoices(sop);
  const current = chosen.material_types ?? effective?.material_types ?? [];
  return (
    <section className="stage-block ws-choice" aria-label="學習單的素材類型">
      <h3>學習單的素材類型</h3>
      <ChipRow
        labels={MATERIAL_TYPES}
        selected={current}
        starred={suggested?.material_types ?? []}
        onToggle={(label) => setOptions({ material_types: toggleIn(current, label, MATERIAL_TYPES) })}
      />
      <p className="field-hint">做學習單時會帶過去，不選也可以。</p>
    </section>
  );
}

/** A few of the 產生設定 chips, with a line saying what they do on the sheet. */
export function GenerationChoice({
  sop,
  labels,
  heading,
  hint,
}: {
  sop: SopController;
  labels: readonly (typeof GENERATION_SETTINGS)[number][];
  heading: string;
  hint?: string;
}) {
  const { effective, suggested, setOptions, chosen } = useChoices(sop);
  const current = chosen.generation ?? effective?.generation ?? [];
  return (
    <section className="stage-block ws-choice" aria-label={heading}>
      <h3>{heading}</h3>
      <ChipRow
        labels={labels}
        selected={current}
        starred={suggested?.generation ?? []}
        onToggle={(label) =>
          setOptions({ generation: toggleIn(current, label as (typeof GENERATION_SETTINGS)[number], GENERATION_SETTINGS) })
        }
      />
      {hint && <p className="field-hint">{hint}</p>}
    </section>
  );
}

/** 基礎版／一般版／進階版. */
export function LevelChoice({ sop }: { sop: SopController }) {
  const { effective, suggested, setOptions, chosen } = useChoices(sop);
  const current = chosen.levels ?? effective?.levels ?? [];
  return (
    <div role="group" aria-label={VERSIONS_HEADING}>
      <p className="ws-sub">{VERSIONS_HEADING}</p>
      <div className="ws-chips">
        {LEVELS.map((level: WsLevel) => (
          <Chip
            key={level}
            label={LEVEL_LABEL[level]}
            checked={current.includes(level)}
            starred={(suggested?.levels ?? []).includes(level)}
            disabled={false}
            onChange={() => setOptions({ levels: toggleIn(current, level, LEVELS) })}
          />
        ))}
      </div>
      <p className="field-hint">不選：只做一般版。</p>
    </div>
  );
}

/** Step 5's 學習支持: what the scaffolds of the tasks become on the sheet, and the versions. */
export function SupportChoice({ sop }: { sop: SopController }) {
  return (
    <section className="stage-block ws-choice" aria-label="鷹架在學習單上的呈現">
      <h3>鷹架在學習單上的呈現</h3>
      <GenerationChoice
        sop={sop}
        heading="語言與教學鷹架"
        labels={["提供引導問題", "顯示作答示例", "提供字詞表", "提供句型框架"]}
        hint="程序鷹架（分步指示）一律由系統依任務加上，不用選。"
      />
      <LevelChoice sop={sop} />
    </section>
  );
}

/** Step 7's compact 產生設定: everything that goes into the worksheet, pre-filled from the steps. */
export function WorksheetChoices({ sop }: { sop: SopController }) {
  const ids = useId();
  const { effective, setOptions, chosen } = useChoices(sop);
  const exitOn = chosen.exit_ticket !== null ? chosen.exit_ticket : !(effective?.skip_exit_ticket ?? false);
  const minutes = chosen.minutes ?? effective?.minutes ?? null;
  return (
    <section className="stage-block ws-choice" aria-labelledby={`${ids}-h`}>
      <h3 id={`${ids}-h`}>產生設定</h3>
      <p className="quiet-note">
        做學習單時會照這裡的選擇，不會再問一次。學生任務與作答方式在第 5 步逐段選；有 ✦ 的是 Manna 依課例內容先選的。
      </p>
      <MaterialTypeChoice sop={sop} />
      <GenerationChoice sop={sop} labels={GENERATION_SETTINGS} heading="學習單要有什麼" />
      <LevelChoice sop={sop} />
      <div className="ws-choice-row">
        <label className="ws-chip">
          <input
            type="checkbox"
            checked={exitOn}
            onChange={() => setOptions({ exit_ticket: !exitOn })}
          />
          <span>學習單最後放 Exit ticket</span>
        </label>
        <label className="field-label" htmlFor={`${ids}-minutes`}>
          預計完成時間（分鐘，選填）
        </label>
        <input
          id={`${ids}-minutes`}
          type="number"
          min={5}
          max={240}
          value={minutes ?? ""}
          onChange={(e) => {
            const n = e.target.value === "" ? null : Number(e.target.value);
            setOptions({ minutes: n !== null && Number.isFinite(n) ? n : null });
          }}
        />
      </div>
    </section>
  );
}
