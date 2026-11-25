import {
type SopGoalSuggestion,
type SopGoalsSuggestion,
type SopJob,
type SopParagraph,
type SopParagraphGoal
} from "../../api/types";
import {
objectiveLabels
} from "../../lib/sop";
import { AskManna,MannaWell,NoMaterialNote,Quote,RedoPending } from "./parts";
import { redoFor,type SopController } from "./useSop";

import { blank,union,type StageProps } from "./manualShared";

// ---------------------------------------------------------------- step 4

type GoalOffer =
  | { kind: "pending"; job: SopJob }
  | { kind: "entry"; job: SopJob<SopGoalsSuggestion>; entry: SopGoalSuggestion };

/**
 * The suggestion a paragraph shows now: the newest of the whole run and of 這一段再建議一次, as long
 * as the teacher has not decided on it.
 */
function goalOffer(sop: SopController, paragraph: SopParagraph): GoalOffer | null {
  const whole = sop.jobs.goals;
  const redo = redoFor(sop.jobs, "goals", null, `p:${paragraph.id}`) as SopJob<SopGoalsSuggestion> | null;
  const source = redo && (!whole || redo.id > whole.id) ? redo : whole;
  if (!source) return null;
  if (source.status === "running" || source.result?.status !== "shown") {
    return source === redo ? { kind: "pending", job: source } : null;
  }
  const entry = source.result.output?.paragraphs.find((g) => g.paragraph_id === paragraph.id);
  if (!entry) return null;
  if (source.decided.includes(entry.paragraph_no)) return null;
  return { kind: "entry", job: source, entry };
}

/** Step 4, 逐段整合目標: which objectives each paragraph serves, and a note on how. */
export function StageIntegrate({ sop, onGoToStage }: StageProps) {
  const { material, content, update, decide } = sop;
  const paragraphs = material?.paragraphs ?? [];
  const labels = objectiveLabels(content.objectives);
  const objectives = content.objectives.filter((o) => o.text.trim());
  const goals = new Map(content.paragraph_goals.map((g) => [g.paragraph_id, g]));
  const whole = sop.jobs.goals;
  const wholeShown = whole?.status === "done" && whole.result?.status === "shown" ? whole.result.output : null;

  const setGoal = (paragraphId: string, patch: Partial<SopParagraphGoal>) =>
    update((c) => {
      const current = c.paragraph_goals.find((g) => g.paragraph_id === paragraphId) ?? {
        paragraph_id: paragraphId,
        objective_ids: [],
        note: "",
        origin: "teacher" as const,
      };
      const rest = c.paragraph_goals.filter((g) => g.paragraph_id !== paragraphId);
      // The teacher changing what Manna wrote makes it a shared piece of work.
      const origin = patch.origin ?? (current.origin === "ai_adopted" ? "ai_edited" : (current.origin ?? "teacher"));
      return { ...c, paragraph_goals: [...rest, { ...current, ...patch, origin }] };
    });

  /** What a paragraph's goal becomes when a suggestion is taken: what the teacher chose or wrote stays. */
  function take(p: SopParagraph, offer: Extract<GoalOffer, { kind: "entry" }>, edit: boolean) {
    const existing = goals.get(p.id);
    const had = !!existing && (existing.objective_ids.length > 0 || !blank(existing.note));
    setGoal(p.id, {
      objective_ids: union(existing?.objective_ids, offer.entry.objective_ids),
      note: blank(existing?.note) ? offer.entry.note : (existing?.note ?? ""),
      origin: !had && !edit ? "ai_adopted" : "ai_edited",
    });
    decide(
      "goals",
      offer.entry.paragraph_no,
      edit ? "revise" : "adopt",
      edit ? offer.entry.note : undefined,
      offer.job.id === whole?.id ? undefined : offer.job.id,
    );
    if (edit) setTimeout(() => document.getElementById(`goal-note-${p.id}`)?.focus(), 0);
  }

  function again(p: SopParagraph, offer: GoalOffer | null) {
    if (offer?.kind === "entry") {
      decide("goals", offer.entry.paragraph_no, "reject", undefined, offer.job.id === whole?.id ? undefined : offer.job.id);
    }
    void sop.startJob("goals", { paragraph_id: p.id, parent_job_id: whole?.id });
  }

  return (
    <>
      <p className="lead">一段一段讀：這一段可以帶到哪一項內容目標、哪一項語言目標？</p>
      <AskManna
        sop={sop}
        kind="goals"
        label="請 Manna 逐段建議"
        againLabel="請 Manna 再建議一次"
        shownNote={
          wholeShown
            ? `Manna 看了 ${wholeShown.paragraphs.length} 段，建議在各段下方。${
                wholeShown.missing_paragraph_ids.length > 0
                  ? `有 ${wholeShown.missing_paragraph_ids.length} 段這次沒有建議到，可以自己選或再請她試一次。`
                  : ""
              }`
            : null
        }
      />
      {objectives.length === 0 && (
        <p className="quiet-note">
          還沒有目標可以對應。
          <button type="button" className="text-link is-plain" onClick={() => onGoToStage(3)}>
            到第 3 步寫目標
          </button>
          ，或先只寫每一段的筆記。
        </p>
      )}
      {paragraphs.length === 0 && <NoMaterialNote onGoToMaterial={() => onGoToStage(1)} />}
      {paragraphs.map((p) => {
        const goal = goals.get(p.id);
        const chosen = goal?.objective_ids ?? [];
        const offer = goalOffer(sop, p);
        return (
          <section key={p.id} className="sop-para" aria-label={`第 ${p.no} 段`}>
            <Quote paragraphs={[p]} />
            {objectives.length > 0 && (
              <fieldset className="goal-choices">
                <legend className="field-label">這一段對應的目標</legend>
                {objectives.map((o) => (
                  <label key={o.id}>
                    <input
                      type="checkbox"
                      checked={chosen.includes(o.id)}
                      onChange={(e) =>
                        setGoal(p.id, {
                          objective_ids: e.target.checked ? [...chosen, o.id] : chosen.filter((id) => id !== o.id),
                        })
                      }
                    />
                    <span>
                      <span className="quiet-term">{labels.get(o.id)}</span>
                      {o.text}
                    </span>
                  </label>
                ))}
              </fieldset>
            )}
            <label className="field-label" htmlFor={`goal-note-${p.id}`}>
              這一段要怎麼帶
            </label>
            <input
              id={`goal-note-${p.id}`}
              type="text"
              value={goal?.note ?? ""}
              onChange={(e) => setGoal(p.id, { note: e.target.value })}
              placeholder="例如：先找出轉折語，再比較前後兩句"
            />
            {offer?.kind === "pending" && (
              <RedoPending
                label={`第 ${p.no} 段`}
                job={offer.job}
                onAgain={() => again(p, null)}
                onCancelled={() => void sop.refreshJob(offer.job.id)}
              />
            )}
            {offer?.kind === "entry" && (
              <MannaWell
                label={`第 ${p.no} 段`}
                onAdopt={() => take(p, offer, false)}
                onEdit={() => take(p, offer, true)}
                onDismiss={() =>
                  decide(
                    "goals",
                    offer.entry.paragraph_no,
                    "reject",
                    undefined,
                    offer.job.id === whole?.id ? undefined : offer.job.id,
                  )
                }
                onAgain={() => again(p, offer)}
                againLabel="這一段再建議一次"
              >
                {offer.entry.objective_ids.length > 0 ? (
                  <ul className="goal-offer">
                    {offer.entry.objective_ids.map((id) => (
                      <li key={id}>
                        <span className="quiet-term">{labels.get(id) ?? "目標"}</span>
                        {content.objectives.find((o) => o.id === id)?.text}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="why">Manna 認為這一段不特別對應目標。</p>
                )}
                <p>{offer.entry.note}</p>
                <p className="why">採用只會補上你還沒選的目標，不會改掉你已經寫的筆記。</p>
              </MannaWell>
            )}
            {!offer && wholeShown && (
              <p className="para-actions">
                <button
                  type="button"
                  className="text-link is-plain"
                  aria-label={`請 Manna 再建議第 ${p.no} 段`}
                  onClick={() => again(p, null)}
                >
                  這一段再建議一次
                </button>
              </p>
            )}
          </section>
        );
      })}
    </>
  );
}
