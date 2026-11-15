import { useEffect, useId, useState } from "react";
import type { ExitKind, WorksheetSettings, WsJob, WsLevel } from "../../api/types";
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
  VERSIONS_HEADING,
  briefIsEmpty,
  emptyBrief,
  toggleIn,
} from "../../lib/worksheet";
import { AiContextPanel } from "../AiContextPanel";
import { ErrorText } from "../common";
import type { WorksheetController } from "./useWorksheet";

type ListField = "material_types" | "student_tasks" | "answer_formats" | "generation";

function Chip({
  label,
  checked,
  starred,
  disabled,
  onChange,
}: {
  label: string;
  checked: boolean;
  starred: boolean;
  disabled: boolean;
  onChange: () => void;
}) {
  return (
    <label className={`ws-chip${starred ? " is-suggested" : ""}`}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={onChange} />
      <span>
        {starred && <span className="star" aria-hidden="true" />}
        {label}
        {starred && <span className="sr-only">（Manna 先選的）</span>}
      </span>
    </label>
  );
}

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
    <div className="manna-well manna-progress ws-progress" role="status" aria-label="產生進度">
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
      <p className="why">
        已經 {elapsed} 秒。每一步通常要 15 到 110 秒。你可以先離開這一頁，好了會留在這份學習單裡。
      </p>
    </div>
  );
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
}

/**
 * The four option blocks of the spec (§5): all multi-select, all optional, labels verbatim. The
 * one filled button generates, whatever is or is not selected.
 */
export function OptionsView({ ws, onGenerated }: OptionsViewProps) {
  const ids = useId();
  const state = ws.state;
  if (!state) return null;
  const { settings, suggested, brief, content } = state;
  const running = state.running;
  const locked = running || ws.busy;
  const fromLesson = state.lesson !== null;

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

  const hasCarried = content.objectives.length > 0 || !briefIsEmpty(brief);

  async function generate() {
    const ok = await ws.generate();
    if (ok) onGenerated();
  }

  return (
    <div className="ws-options">
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
                ws.setContent((c) => ({ ...c, objectives: [] }));
              }}
            >
              全部不帶入
            </button>
          </div>
          <p className="quiet-note">這些只是起點。可以改、可以拿掉；在這裡改不會動到課例。</p>

          {content.objectives.length > 0 && (
            <div role="group" aria-label="帶入的學習目標">
              <p className="ws-sub">學習目標</p>
              <ul className="ws-carried-list">
                {content.objectives.map((o, i) => (
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

      {running ? (
        <ProgressPanel jobs={state.jobs} />
      ) : (
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
            {ws.busy ? "送出中…" : "產生學習單"}
          </button>
        </div>
      )}
      <ErrorText message={ws.actionError} />
      <p className="quiet-note">
        會同時產生學生版和教師參考版。Exit ticket 預設放在最後，之後可以關掉。
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
