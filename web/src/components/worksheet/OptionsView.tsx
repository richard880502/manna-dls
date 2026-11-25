import { useEffect, useId, useRef, useState } from "react";
import { useNavigate } from "react-router";
import type { ExitKind, WorksheetSettings, WorksheetState, WsJob, WsLevel, WsOutput } from "../../api/types";
import {
  OUTPUTS,
  OUTPUT_LABEL,
  countsTotal,
  fullCounts,
  outputOf,
  planFromSettings,
  planSummary,
  settingsFromPlan,
  type BloomPlan,
  type BloomPresetId,
} from "../../lib/bloom";
import {
  ANSWER_FORMATS,
  EXIT_KIND_LABEL,
  GENERATION_SETTINGS,
  INFO_FUNCTION_LABEL,
  LEVELS,
  LEVEL_LABEL,
  MATERIAL_TYPES,
  STEP_LABEL,
  STUDENT_TASKS,
  STUDENT_TASK_GROUPS,
  TIERED_FORMATS,
  TIERED_VERSIONS_NOTE,
  VERSIONS_HEADING,
  briefIsEmpty,
  emptyBrief,
  toggleIn,
} from "../../lib/worksheet";
import { hasObjectives, isFromFields, syncedObjectives } from "../../lib/objectives";
import { AiContextPanel } from "../AiContextPanel";
import { RunProgressPanel, useRunPanel } from "../RunProgress";
import { PROGRESS_WORDS } from "../../lib/progress";
import { ErrorText } from "../common";
import { LongMaterialNotice, MaterialFixButtons } from "../material/LongMaterial";
import { RETRY, isLongMaterial } from "../../lib/longMaterial";
import { ObjectiveFields } from "../objectives/ObjectiveFields";
import { BloomSettings } from "./BloomSettings";
import { Chip } from "./Chips";
import { scrollToEl } from "../../lib/scroll";
import { TemplateOption } from "./TemplateOption";
import type { WorksheetController } from "./useWorksheet";

type ListField = "material_types" | "student_tasks" | "answer_formats" | "generation";

/** What each output makes (the contract's explore-card lines). */
const OUTPUT_HINT: Record<WsOutput, string> = {
  worksheet: "完整學習單，可以照你的版型",
  comprehension: "依 Bloom 六個層次出題，附答案與文本依據",
  tiered: "同一篇文章做成基礎／一般／進階三個版本。三個版本目標相同，差在提示與支持的多寡。",
};

const GENERATE_LABEL: Record<WsOutput, string> = {
  worksheet: "產生學習單",
  comprehension: "產生閱讀理解題",
  tiered: "產生差異化學習單",
};

export const REQUEST_MAX = 2000;
const SHOW_OBJECTIVES = "顯示學習目標";
const OPTIONS_OBJECTIVES_HINT = "選填，一行一項，各寫一項就好。Manna 出題時會照這兩個目標，並印在學習單最上面。";

/** What each step of a run is doing; the teacher can leave and come back. */
export function ProgressPanel({ jobs }: { jobs: WsJob[] }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  const started = jobs
    .map((j) => (j.started_at ? new Date(j.started_at).getTime() : NaN))
    .filter((t) => !Number.isNaN(t))
    .sort((a, b) => a - b)[0];
  const elapsed = started ? Math.max(0, Math.round((now - started) / 1000)) : 0;

  return (
    <div className="manna-well manna-progress ws-progress">
      {/* The live region is the step list (and its title); the ticking seconds below stay out of it. */}
      <div role="status" aria-label="產生進度">
        <p className="manna-label">
          <span className="star is-working" aria-hidden="true" />
          Manna 正在產生
        </p>
        <ol className="ws-steps">
          {jobs.map((job) => (
            <li key={job.id} className={`is-${job.status}`}>
              <span className="ws-step-name">{STEP_LABEL[job.step]}</span>
              <span className="ws-step-state">{jobStateText(job)}</span>
            </li>
          ))}
        </ol>
      </div>
      <p className="why">
        已經 {elapsed} 秒。每一步通常要 15 到 110 秒。你可以先離開這一頁，好了會留在這份學習單裡。
      </p>
    </div>
  );
}

/**
 * While a run goes on: its steps (「進度」, docs/API.md「產生進度」), which stay a moment once done
 * and stay when a step failed. A run without steps (an older server or run) shows the job rows as
 * before; a closed panel leaves a working line while the run goes on.
 */
export function WorksheetRunProgress({
  state,
  onCancelled,
}: {
  state: Pick<WorksheetState, "id" | "jobs" | "running" | "progress">;
  onCancelled?: () => void;
}) {
  const progress = state.progress ?? null;
  const runNo = state.jobs[0]?.run_no;
  const panel = useRunPanel(progress, runNo === undefined ? null : `${state.id}:${runNo}`);
  if (!progress) return state.running ? <ProgressPanel jobs={state.jobs} /> : null;
  if (panel.visible) return <RunProgressPanel progress={progress} onClose={panel.close} onCancelled={onCancelled} className="ws-progress" />;
  return state.running ? (
    <p className="manna-label ws-progress" role="status">
      <span className="star is-working" aria-hidden="true" />
      {PROGRESS_WORDS.stillWorking}
    </p>
  ) : null;
}

export function jobStateText(job: WsJob): string {
  if (job.status === "queued") return "排隊中";
  if (job.status === "running") return `進行中${job.phase ? `：${job.phase}` : ""}`;
  if (job.outcome === "shown") return "完成";
  if (job.outcome === "skipped") return `略過${job.message ? `：${job.message}` : ""}`;
  return `沒有成功${job.message ? `：${job.message}` : ""}`;
}

/** After a run: the steps that did not make it, said plainly. Nothing here blocks editing. */
export function RunOutcome({ jobs }: { jobs: WsJob[] }) {
  const failed = jobs.filter((j) => j.status === "done" && j.outcome !== "shown" && j.outcome !== "skipped");
  const context = jobs.find((j) => j.context.length > 0)?.context ?? [];
  if (failed.length === 0 && context.length === 0) return null;
  return (
    <div className="ws-run-outcome">
      {failed.length > 0 && (
        <ul className="ws-failed" aria-label="沒有成功的步驟">
          {failed.map((j) => (
            <li key={j.id}>
              {STEP_LABEL[j.step]}沒有成功{j.message ? `：${j.message}` : "。"}你可以自己寫，或按那一區塊的「重新產生」。
            </li>
          ))}
        </ul>
      )}
      {context.length > 0 && <AiContextPanel items={context} />}
    </div>
  );
}

interface OptionsViewProps {
  ws: WorksheetController;
  onGenerated: () => void;
  /** 修改課文 / 只選要用的段落: the page's dialogs for the library material. */
  onEditText?: () => void;
  onPickParagraphs?: () => void;
}

/**
 * The four option blocks of the spec (§5): all multi-select, all optional, labels verbatim. The
 * one filled button generates, whatever is or is not selected.
 */
export function OptionsView({ ws, onGenerated, onEditText, onPickParagraphs }: OptionsViewProps) {
  const ids = useId();
  const navigate = useNavigate();
  const [longSeen, setLongSeen] = useState(false);
  // The preset is not stored (only the counts are); the teacher's last pick is kept here so that
  // 自訂 stays 自訂 even while its counts happen to equal a preset's.
  const [presetChoice, setPresetChoice] = useState<BloomPresetId | null>(null);
  const state = ws.state;
  const topRef = useRef<HTMLDivElement>(null);
  const running0 = state?.running ?? false;
  // When a run starts, bring the top of the column into view only if it is scrolled out of sight.
  useEffect(() => {
    const el = topRef.current;
    if (!running0 || !el) return;
    if (el.getBoundingClientRect().top < 0) scrollToEl(el, "start");
  }, [running0]);
  if (!state) return null;
  const { settings, suggested, brief, content } = state;
  const running = state.running;
  const locked = running || ws.busy;
  const fromLesson = state.lesson !== null;
  const output = outputOf(settings);
  const fullWorksheet = output === "worksheet";
  const derived = planFromSettings(settings);
  const plan: BloomPlan = { ...derived, preset: presetChoice ?? derived.preset };
  const countsSet = countsTotal(fullCounts(settings.bloom_counts)) > 0;
  const request = state.request ?? "";

  const setPlan = (next: BloomPlan) => {
    setPresetChoice(next.preset);
    ws.setSettings((s) => ({ ...s, ...settingsFromPlan(next) }));
  };

  const toggle = (field: ListField, value: string, order: readonly string[]) =>
    ws.setSettings((s) => ({ ...s, [field]: toggleIn(s[field], value, order) }) as WorksheetSettings);
  const toggleLevel = (level: WsLevel) => ws.setSettings((s) => ({ ...s, levels: toggleIn(s.levels, level, LEVELS) }));

  const chips = (field: ListField, labels: readonly string[], order: readonly string[]) => (
    <div className="ws-chips">
      {labels.map((label) => (
        <Chip
          key={label}
          label={label}
          checked={settings[field].includes(label)}
          starred={suggested[field].includes(label)}
          disabled={locked}
          onChange={() => toggle(field, label, order)}
        />
      ))}
    </div>
  );

  const anySuggested =
    suggested.material_types.length +
      suggested.student_tasks.length +
      suggested.answer_formats.length +
      suggested.generation.length +
      suggested.levels.length >
    0;

  // The objectives of the two fields are edited above; the lesson's (and the teacher's own on the sheet) here.
  const carried = content.objectives.filter((o) => !isFromFields(o));
  const hasCarried = carried.length > 0 || !briefIsEmpty(brief);

  /**
   * One of the two objective fields changed: the worksheet's objectives follow (and, given where
   * there were none, 顯示學習目標 is ticked), as the server does on save.
   */
  const setObjective = (field: "content_objective" | "language_objective", value: string) => {
    const next = { ...settings, [field]: value === "" ? null : value };
    const shown =
      !hasObjectives(settings.content_objective, settings.language_objective) &&
      hasObjectives(next.content_objective, next.language_objective) &&
      !state.template &&
      !settings.generation.includes(SHOW_OBJECTIVES);
    ws.setSettings((s) => ({
      ...s,
      [field]: next[field],
      generation: shown ? toggleIn(s.generation, SHOW_OBJECTIVES, GENERATION_SETTINGS) : s.generation,
    }));
    ws.setContent((c) => ({
      ...c,
      objectives: syncedObjectives(c.objectives, next.content_objective, next.language_objective),
    }));
  };

  async function generate() {
    // What the picker shows is what is asked for: the default preset is written before it is sent.
    if (!fullWorksheet && !countsSet) ws.setSettings((s) => ({ ...s, ...settingsFromPlan(plan) }));
    const ok = await ws.generate();
    if (ok) onGenerated();
  }

  return (
    <div className="ws-options" ref={topRef}>
      {/* 進度 comes first, so it is the first thing seen once a run starts. */}
      {running && <WorksheetRunProgress state={state} onCancelled={() => void ws.refresh()} />}
      <section className="ws-opt" aria-labelledby={`${ids}-what`}>
        <h2 id={`${ids}-what`}>要做什麼</h2>
        <div>
          <div className="ws-seg ws-output" role="radiogroup" aria-labelledby={`${ids}-what`}>
            {OUTPUTS.map((o) => (
              <label key={o}>
                <input
                  type="radio"
                  name={`${ids}-output`}
                  checked={output === o}
                  // 版型只適用於完整學習單 (the server says 400 otherwise).
                  disabled={locked || (o !== "worksheet" && Boolean(state.template))}
                  onChange={() => ws.setSettings((s) => ({ ...s, output: o }))}
                />
                <span>{OUTPUT_LABEL[o]}</span>
              </label>
            ))}
          </div>
          <p className="field-hint">{OUTPUT_HINT[output]}</p>
          {state.template && fullWorksheet && (
            <p className="field-hint">版型只適用於完整學習單；要做閱讀理解題或差異化學習單，先把下面的版型改成「不使用版型」。</p>
          )}
        </div>
      </section>

      <section className="ws-opt" aria-labelledby={`${ids}-goal`}>
        <h2 id={`${ids}-goal`}>學習目標</h2>
        <ObjectiveFields
          content={settings.content_objective ?? ""}
          language={settings.language_objective ?? ""}
          disabled={locked}
          hint={OPTIONS_OBJECTIVES_HINT}
          onContent={(value) => setObjective("content_objective", value)}
          onLanguage={(value) => setObjective("language_objective", value)}
        />
      </section>

      {!fullWorksheet && (
        <>
          <section className="ws-opt" aria-labelledby={`${ids}-bloom`}>
            <h2 id={`${ids}-bloom`}>Bloom 認知層次</h2>
            <BloomSettings value={plan} disabled={locked} onChange={setPlan} />
          </section>
          <section className="ws-opt" aria-labelledby={`${ids}-fmt`}>
            <h2 id={`${ids}-fmt`}>作答方式</h2>
            <div>
              {chips("answer_formats", output === "tiered" ? TIERED_FORMATS : ANSWER_FORMATS, ANSWER_FORMATS)}
              <p className="field-hint">
                {output === "tiered"
                  ? "差異化學習單只用開放式作答，三個版本才能各給不同的支持與預期答案。不選：每個層次用適合它的。"
                  : "不選：每個層次用適合它的作答方式。有選：只用選了的，而且適合那個層次的。"}
              </p>
            </div>
          </section>
          <section className="ws-opt" aria-labelledby={`${ids}-levels`}>
            <h2 id={`${ids}-levels`}>版本</h2>
            {output === "tiered" ? (
              <p className="field-hint ws-opt-note">{TIERED_VERSIONS_NOTE}</p>
            ) : (
              <div role="group" aria-label={VERSIONS_HEADING}>
                <div className="ws-chips">
                  {LEVELS.map((level) => (
                    <Chip
                      key={level}
                      label={LEVEL_LABEL[level]}
                      checked={settings.levels.includes(level)}
                      starred={suggested.levels.includes(level)}
                      disabled={locked}
                      onChange={() => toggleLevel(level)}
                    />
                  ))}
                </div>
                <p className="field-hint">不選：只做一般版。</p>
              </div>
            )}
          </section>
        </>
      )}

      {fullWorksheet && (
        <>
          <p className="ws-legend">
            {anySuggested && (
              <>
                有 <span className="star" aria-hidden="true" />
                <span className="sr-only">星號</span> 的是 Manna 依{fromLesson ? "課例" : "教材"}先幫你選的。
              </>
            )}
            都可以改；全部不選也能產生，Manna 會依教材判斷。
          </p>

          <section className="ws-opt" aria-labelledby={`${ids}-o1`}>
            <h2 id={`${ids}-o1`}>素材類型</h2>
            {chips("material_types", MATERIAL_TYPES, MATERIAL_TYPES)}
          </section>

          <section className="ws-opt" aria-labelledby={`${ids}-o2`}>
            <h2 id={`${ids}-o2`}>學生任務</h2>
            <div>
              {STUDENT_TASK_GROUPS.map((group) => (
                <div key={group.title} role="group" aria-label={group.title}>
                  <p className="ws-sub">{group.title}</p>
                  {chips("student_tasks", group.labels, STUDENT_TASKS)}
                </div>
              ))}
            </div>
          </section>

          <section className="ws-opt" aria-labelledby={`${ids}-o3`}>
            <h2 id={`${ids}-o3`}>作答方式</h2>
            {chips("answer_formats", ANSWER_FORMATS, ANSWER_FORMATS)}
          </section>

          <section className="ws-opt" aria-labelledby={`${ids}-o4`}>
            <h2 id={`${ids}-o4`}>產生設定</h2>
            <div>
              {chips("generation", GENERATION_SETTINGS, GENERATION_SETTINGS)}
              <div role="group" aria-label={VERSIONS_HEADING}>
                <p className="ws-sub">{VERSIONS_HEADING}</p>
                <div className="ws-chips">
                  {LEVELS.map((level) => (
                    <Chip
                      key={level}
                      label={LEVEL_LABEL[level]}
                      checked={settings.levels.includes(level)}
                      starred={suggested.levels.includes(level)}
                      disabled={locked}
                      onChange={() => toggleLevel(level)}
                    />
                  ))}
                </div>
              </div>
            </div>
          </section>
        </>
      )}

      {(fullWorksheet || state.template) && <TemplateOption ws={ws} locked={locked} />}

      <section className="ws-opt" aria-labelledby={`${ids}-ask`}>
        <h2 id={`${ids}-ask`}>給 Manna 的要求</h2>
        <div>
          <textarea
            rows={3}
            aria-labelledby={`${ids}-ask`}
            aria-describedby={`${ids}-ask-count`}
            maxLength={REQUEST_MAX}
            value={request}
            placeholder="例如：B1 程度，多一點推論題；選項不要太長"
            disabled={locked}
            onChange={(e) => ws.setRequest(e.target.value)}
          />
          <p className="field-hint ws-count" id={`${ids}-ask-count`}>
            選填 · {request.length}/{REQUEST_MAX}
          </p>
        </div>
      </section>

      {hasCarried && (
        <section className="ws-carried" aria-labelledby={`${ids}-carried`}>
          <div className="head-row">
            <h2 id={`${ids}-carried`}>從課例帶入的內容</h2>
            <button
              type="button"
              className="text-link is-plain"
              disabled={locked}
              onClick={() => {
                ws.setBrief(() => emptyBrief());
                ws.setContent((c) => ({ ...c, objectives: c.objectives.filter(isFromFields) }));
              }}
            >
              全部不帶入
            </button>
          </div>
          <p className="quiet-note">這些只是起點。可以改、可以拿掉；在這裡改不會動到課例。</p>

          {carried.length > 0 && (
            <div role="group" aria-label="帶入的學習目標">
              <p className="ws-sub">學習目標</p>
              <ul className="ws-carried-list">
                {carried.map((o, i) => (
                  <li key={o.id}>
                    <span className="soft-tag">{o.type === "content" ? "內容目標" : "語言目標"}</span>
                    <input
                      type="text"
                      lang="en"
                      aria-label={`學習目標 ${i + 1}`}
                      value={o.student_facing_en}
                      placeholder={o.text || "I can …"}
                      disabled={locked}
                      onChange={(e) =>
                        ws.setContent((c) => ({
                          ...c,
                          objectives: c.objectives.map((x) =>
                            x.id === o.id ? { ...x, student_facing_en: e.target.value } : x,
                          ),
                        }))
                      }
                    />
                    <button
                      type="button"
                      className="text-link is-plain"
                      aria-label={`拿掉學習目標 ${i + 1}`}
                      disabled={locked}
                      onClick={() =>
                        ws.setContent((c) => ({ ...c, objectives: c.objectives.filter((x) => x.id !== o.id) }))
                      }
                    >
                      拿掉
                    </button>
                    {o.text && <p className="quiet-note">{o.text}</p>}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {brief.tasks.length > 0 && (
            <div role="group" aria-label="帶入的問題與任務">
              <p className="ws-sub">問題與任務</p>
              <ul className="ws-carried-list">
                {brief.tasks.map((t, i) => (
                  <li key={t.id}>
                    {t.function && <span className="soft-tag">{INFO_FUNCTION_LABEL[t.function]}</span>}
                    <input
                      type="text"
                      aria-label={`帶入的任務 ${i + 1}`}
                      value={t.text}
                      disabled={locked}
                      onChange={(e) =>
                        ws.setBrief((b) => ({
                          ...b,
                          tasks: b.tasks.map((x) => (x.id === t.id ? { ...x, text: e.target.value } : x)),
                        }))
                      }
                    />
                    <button
                      type="button"
                      className="text-link is-plain"
                      aria-label={`拿掉任務 ${i + 1}`}
                      disabled={locked}
                      onClick={() => ws.setBrief((b) => ({ ...b, tasks: b.tasks.filter((x) => x.id !== t.id) }))}
                    >
                      拿掉
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          )}

          <TextList
            title="暖身想法"
            itemName="暖身想法"
            values={brief.warmup}
            locked={locked}
            onChange={(warmup) => ws.setBrief((b) => ({ ...b, warmup }))}
          />
          <TextList
            title="關鍵字彙"
            itemName="關鍵字"
            values={brief.vocabulary}
            locked={locked}
            lang="en"
            onChange={(vocabulary) => ws.setBrief((b) => ({ ...b, vocabulary }))}
          />
          {brief.exit_ticket.length > 0 && (
            <div role="group" aria-label="帶入的 Exit ticket">
              <p className="ws-sub">Exit ticket</p>
              <ul className="ws-carried-list">
                {brief.exit_ticket.map((e, i) => (
                  // oxlint-disable-next-line react/no-array-index-key -- plain strings, edited in place
                  <li key={i}>
                    <span className="soft-tag">{EXIT_KIND_LABEL[e.kind as ExitKind]}</span>
                    <input
                      type="text"
                      aria-label={`帶入的 Exit ticket ${i + 1}`}
                      value={e.text}
                      disabled={locked}
                      onChange={(ev) =>
                        ws.setBrief((b) => ({
                          ...b,
                          exit_ticket: b.exit_ticket.map((x, j) => (j === i ? { ...x, text: ev.target.value } : x)),
                        }))
                      }
                    />
                    <button
                      type="button"
                      className="text-link is-plain"
                      aria-label={`拿掉 Exit ticket ${i + 1}`}
                      disabled={locked}
                      onClick={() =>
                        ws.setBrief((b) => ({ ...b, exit_ticket: b.exit_ticket.filter((_, j) => j !== i) }))
                      }
                    >
                      拿掉
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </section>
      )}

      {!running &&
        !longSeen &&
        !state.material.source.selected_ids &&
        isLongMaterial(state.material.paragraphs.length) && (
          <LongMaterialNotice
            paragraphs={state.material.paragraphs.length}
            disabled={ws.busy}
            onEdit={() => onEditText?.()}
            onPick={() => onPickParagraphs?.()}
            onGo={() => {
              setLongSeen(true);
              void generate();
            }}
          />
        )}

      {running ? null : fullWorksheet ? (
        <div className="ws-gen-bar">
          <label className="ws-time" htmlFor={`${ids}-minutes`}>
            預計完成時間（分鐘）
            <input
              id={`${ids}-minutes`}
              type="text"
              inputMode="numeric"
              value={settings.minutes === null ? "" : String(settings.minutes)}
              aria-describedby={`${ids}-minutes-hint`}
              disabled={locked}
              onChange={(e) => {
                const digits = e.target.value.replace(/\D/g, "").slice(0, 3);
                ws.setSettings((s) => ({ ...s, minutes: digits === "" ? null : Number(digits) }));
              }}
            />
          </label>
          <span className="quiet-note" id={`${ids}-minutes-hint`}>
            不填也可以
          </span>
          <span className="ws-grow" />
          <button type="button" className="primary-button big-button" disabled={ws.busy} onClick={() => void generate()}>
            {ws.busy ? "送出中…" : GENERATE_LABEL.worksheet}
          </button>
        </div>
      ) : (
        <div className="ws-gen-bar">
          <p className="ws-summary" role="status" aria-label="這次會出">
            {planSummary(plan.counts)}
          </p>
          <span className="ws-grow" />
          <button type="button" className="primary-button big-button" disabled={ws.busy} onClick={() => void generate()}>
            {ws.busy ? "送出中…" : GENERATE_LABEL[output]}
          </button>
        </div>
      )}
      <ErrorText message={ws.actionError} />
      {ws.actionError && (
        <MaterialFixButtons
          disabled={ws.busy}
          onBack={state.prep_session_id ? () => navigate(`/prep/${state.prep_session_id}`) : undefined}
          onEdit={onEditText}
          onPick={onPickParagraphs}
          onRetry={() => void generate()}
          retryLabel={RETRY}
        />
      )}
      <p className="quiet-note">
        {fullWorksheet
          ? "會同時產生學生版和教師版。Exit ticket 預設放在最後，之後可以關掉。"
          : "會同時產生學生版和教師版；教師版每一題都標出 Bloom 層次。"}
        {state.generated ? "再產生一次會換掉目前的區塊；之前的內容仍在版本紀錄裡。" : ""}
      </p>
    </div>
  );
}

function TextList({
  title,
  itemName,
  values,
  locked,
  lang,
  onChange,
}: {
  title: string;
  itemName: string;
  values: string[];
  locked: boolean;
  lang?: string;
  onChange: (next: string[]) => void;
}) {
  if (values.length === 0) return null;
  return (
    <div role="group" aria-label={`帶入的${title}`}>
      <p className="ws-sub">{title}</p>
      <ul className="ws-carried-list">
        {values.map((value, i) => (
          // oxlint-disable-next-line react/no-array-index-key -- plain strings, edited in place
          <li key={i}>
            <input
              type="text"
              lang={lang}
              aria-label={`帶入的${itemName} ${i + 1}`}
              value={value}
              disabled={locked}
              onChange={(e) => onChange(values.map((v, j) => (j === i ? e.target.value : v)))}
            />
            <button
              type="button"
              className="text-link is-plain"
              aria-label={`拿掉${itemName} ${i + 1}`}
              disabled={locked}
              onClick={() => onChange(values.filter((_, j) => j !== i))}
            >
              拿掉
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
