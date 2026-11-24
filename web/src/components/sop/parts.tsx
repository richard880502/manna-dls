import { useEffect, useState, type ReactNode } from "react";
import type { ObjectiveType, SopJob, SopJobKind, SopParagraph, SopScaffolds } from "../../api/types";
import { useApp } from "../../context/AppContext";
import { SCAFFOLD_FIELDS, paragraphMark } from "../../lib/sop";
import { AiContextPanel } from "../AiContextPanel";
import { AiStatusMessage, AiUnavailableNote, ErrorText } from "../common";
import type { SopController } from "./useSop";

/** The source text quoted above what was designed for it: print face, paragraph number in the margin. */
export function Quote({ paragraphs }: { paragraphs: SopParagraph[] }) {
  if (paragraphs.length === 0) return null;
  const sorted = [...paragraphs].sort((a, b) => a.no - b.no);
  return (
    <p className="sop-quote" data-n={paragraphMark(sorted)} lang="en">
      <span className="sr-only">第 {sorted.map((p) => p.no).join("、")} 段：</span>
      {sorted.map((p) => p.text).join(" ")}
    </p>
  );
}

interface MannaWellProps {
  /** What kind of suggestion and where it belongs, e.g. 「第 3 段」 or 「內容目標」. */
  label: string;
  children: ReactNode;
  onAdopt: () => void;
  onEdit: () => void;
  onDismiss: () => void;
  /** 換一個 / 這一段再建議一次: asks for just this suggestion again. */
  onAgain?: () => void;
  againLabel?: string;
}

/**
 * One suggestion from Manna, in gold, sitting under the step or paragraph it belongs to. Nothing
 * of it is written into the lesson until the teacher presses 採用 or finishes 修改後採用.
 */
export function MannaWell({ label, children, onAdopt, onEdit, onDismiss, onAgain, againLabel = "換一個" }: MannaWellProps) {
  const name = `Manna 建議 · ${label}`;
  return (
    <div className="manna-well" role="group" aria-label={name}>
      <p className="manna-label">
        <span className="star" aria-hidden="true" />
        {name}
      </p>
      {children}
      <div className="decide">
        <button type="button" className="decide-button is-take" onClick={onAdopt}>
          採用
        </button>
        <button type="button" className="decide-button" onClick={onEdit}>
          修改後採用
        </button>
        <button type="button" className="decide-button" onClick={onDismiss}>
          不用
        </button>
        {onAgain && (
          <button type="button" className="decide-button" onClick={onAgain}>
            {againLabel}
          </button>
        )}
      </div>
    </div>
  );
}

function useElapsedSeconds(since: string | null | undefined, active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [active]);
  const start = since ? new Date(since).getTime() : NaN;
  return Number.isNaN(start) ? 0 : Math.max(0, Math.round((now - start) / 1000));
}

/** One of several things Manna can be asked for in a stage (the objectives: 內容目標 first, then 語言目標). */
export interface AskOption {
  label: string;
  /** Sent with the request (`only_type`). */
  only: ObjectiveType;
  /** Not available yet: said under the button why. */
  blocked?: string | null;
}

interface AskMannaProps {
  sop: SopController;
  kind: SopJobKind;
  /** Several buttons instead of one (`label` and `againLabel` are then not used). */
  options?: AskOption[];
  /** The button before anything was asked, e.g. 「請 Manna 逐段分析」. */
  label: string;
  /** The button once there is a result. */
  againLabel: string;
  /** Said under the button once suggestions are on the page. */
  shownNote?: string | null;
}

/**
 * Asks Manna for one stage and shows what is going on while she works. A call takes 15 to 50
 * seconds (sometimes minutes), so the request only starts a job: the teacher sees the phase and
 * the seconds, and can go on with other steps.
 */
export function AskManna({ sop, kind, label, againLabel, shownNote, options }: AskMannaProps) {
  const { config } = useApp();
  const aiEnabled = config?.ai_enabled !== false;
  const job = sop.jobs[kind];
  const running = job?.status === "running";
  const result = job?.status === "done" ? job.result : null;
  const noMaterial = !sop.material || sop.material.paragraphs.length === 0;
  const elapsed = useElapsedSeconds(job?.started_at, running);

  return (
    <div className="ask-manna">
      {!running && !options && (
        <button
          type="button"
          className="manna-button"
          disabled={!aiEnabled || noMaterial}
          onClick={() => void sop.startJob(kind)}
        >
          <span className="star" aria-hidden="true" />
          {result ? againLabel : label}
        </button>
      )}
      {!running &&
        options?.map((option) => (
          <div key={option.only} className="ask-option">
            <button
              type="button"
              className="manna-button"
              disabled={!aiEnabled || noMaterial || Boolean(option.blocked)}
              aria-describedby={option.blocked ? `ask-${kind}-${option.only}-why` : undefined}
              onClick={() => void sop.startJob(kind, { only_type: option.only })}
            >
              <span className="star" aria-hidden="true" />
              {option.label}
            </button>
            {option.blocked && aiEnabled && !noMaterial && (
              <p className="quiet-note" id={`ask-${kind}-${option.only}-why`}>
                {option.blocked}
              </p>
            )}
          </div>
        ))}
      {!running && noMaterial && aiEnabled && (
        <p className="quiet-note">先在第 1 步貼上教材，Manna 才有東西可以讀。這一步也可以全部自己寫。</p>
      )}
      {!aiEnabled && <AiUnavailableNote />}
      {running && (
        <div className="manna-well manna-progress" role="status">
          <p className="manna-label">
            <span className="star is-working" aria-hidden="true" />
            Manna 正在處理
          </p>
          <p>{job?.phase || "正在處理"}…</p>
          <p className="why">
            已經 {elapsed} 秒。通常要 15 到 50 秒，偶爾更久。你可以先做別的步驟，好了會出現在這裡。
          </p>
        </div>
      )}
      <ErrorText message={sop.jobError[kind]} />
      {result && result.status !== "shown" && <AiStatusMessage status={result.status} message={result.message} />}
      {result?.status === "shown" && shownNote && <p className="quiet-note">{shownNote}</p>}
      {result?.status === "shown" && <AiContextPanel items={result.context} />}
    </div>
  );
}

/** The three scaffold fields of a task or a warm-up activity (規格 §3). */
export function ScaffoldFields({
  idPrefix,
  value,
  onChange,
}: {
  idPrefix: string;
  value: SopScaffolds;
  onChange: (next: SopScaffolds) => void;
}) {
  return (
    <div className="scaffold-fields">
      {SCAFFOLD_FIELDS.map((field) => (
        <div key={field.key} className="scaffold-field">
          <label className="field-label" htmlFor={`${idPrefix}-${field.key}`}>
            {field.label}
          </label>
          <textarea
            id={`${idPrefix}-${field.key}`}
            rows={field.key === "procedural" ? 3 : 2}
            value={value[field.key]}
            placeholder={field.hint}
            onChange={(e) => onChange({ ...value, [field.key]: e.target.value })}
          />
        </div>
      ))}
    </div>
  );
}

/** The scaffolds of a task as a quiet definition list; empty ones are left out. */
export function ScaffoldList({ value }: { value: SopScaffolds }) {
  const filled = SCAFFOLD_FIELDS.filter((f) => value[f.key].trim());
  if (filled.length === 0) return null;
  return (
    <dl className="scaf">
      {filled.map((f) => (
        <div key={f.key}>
          <dt>{f.label}</dt>
          <dd className="pre-line">{value[f.key]}</dd>
        </div>
      ))}
    </dl>
  );
}

/** What a step says when there is no material to go paragraph by paragraph. */
export function NoMaterialNote({ onGoToMaterial }: { onGoToMaterial: () => void }) {
  return (
    <p className="quiet-note">
      還沒有教材，所以沒有段落可以對照。
      <button type="button" className="text-link is-plain" onClick={onGoToMaterial}>
        到第 1 步貼上教材
      </button>
      。
    </p>
  );
}

/** A suggestion that is being asked for again: what is going on, or that it did not work. */
export function RedoPending({
  label,
  job,
  onAgain,
}: {
  label: string;
  job: SopJob | null;
  onAgain: () => void;
}) {
  const elapsed = useElapsedSeconds(job?.started_at, job?.status === "running");
  if (!job) return null;
  if (job.status === "running") {
    return (
      <div className="manna-well manna-progress" role="status" aria-label={`Manna 正在重新建議 · ${label}`}>
        <p className="manna-label">
          <span className="star is-working" aria-hidden="true" />
          Manna 正在重新建議 · {label}
        </p>
        <p>{job.phase || "正在處理"}…（已經 {elapsed} 秒）</p>
      </div>
    );
  }
  if (job.result?.status === "shown") return null;
  return (
    <div className="manna-well" role="status">
      <p className="manna-label">
        <span className="star" aria-hidden="true" />
        Manna 建議 · {label}
      </p>
      <AiStatusMessage status={job.result?.status ?? "error"} message={job.result?.message ?? null} />
      <div className="decide">
        <button type="button" className="decide-button" onClick={onAgain}>
          再試一次
        </button>
      </div>
    </div>
  );
}
