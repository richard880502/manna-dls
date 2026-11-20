import { useEffect,useRef } from "react";
import {
type ExitKind,
type SopExitItem,
type SopExitSuggestion
} from "../../api/types";
import {
EXIT_KINDS,
exitKindLabel,
newId,
objectiveLabels
} from "../../lib/sop";
import { WorksheetChoices } from "./Choices";
import { AskManna,MannaWell } from "./parts";
import { type SopController } from "./useSop";


// ---------------------------------------------------------------- step 7

/** Step 7, Exit ticket: a few short items for formative checking, reflection or collecting questions. */
export function StageExit({ sop }: { sop: SopController }) {
  const { content, update, decide } = sop;
  const items = content.exit_ticket.items;
  const labels = objectiveLabels(content.objectives);
  const objectives = content.objectives.filter((o) => o.text.trim());
  const job = sop.jobs.exit_ticket;
  const suggestion: SopExitSuggestion | null =
    job?.status === "done" && job.result?.status === "shown" ? (job.result.output ?? null) : null;
  // The field to focus once the edit just made has been drawn.
  const focusNext = useRef<string | null>(null);
  useEffect(() => {
    if (!focusNext.current) return;
    document.getElementById(focusNext.current)?.focus();
    focusNext.current = null;
  });

  const setItems = (change: (list: SopExitItem[]) => SopExitItem[]) =>
    update((c) => ({ ...c, exit_ticket: { items: change(c.exit_ticket.items) } }));
  const patchItem = (id: string, patch: Partial<SopExitItem>) =>
    setItems((list) =>
      list.map((i) =>
        i.id === id ? { ...i, ...patch, origin: i.origin === "ai_adopted" ? "ai_edited" : i.origin } : i,
      ),
    );

  function take(index: number, edit: boolean) {
    const s = suggestion?.items[index];
    if (!s) return;
    const id = newId("e");
    setItems((list) => [
      ...list,
      {
        id,
        kind: s.kind,
        text: s.text,
        origin: edit ? "ai_edited" : "ai_adopted",
        // A link to an objective the teacher has deleted since is not carried over.
        objective_id: objectives.some((o) => o.id === s.objective_id) ? s.objective_id : "",
        answer: s.answer,
      },
    ]);
    decide("exit_ticket", index, edit ? "revise" : "adopt", edit ? s.text : undefined);
    if (edit) focusNext.current = `exit-${id}-text`;
  }

  const uncovered = (suggestion?.uncovered_objective_ids ?? [])
    .map((id) => labels.get(id))
    .filter((l): l is string => !!l);

  return (
    <>
      <p className="lead">下課前兩三分鐘，想知道什麼？題數少一點。學習單上會放在最後，可以拿掉。</p>
      <AskManna
        sop={sop}
        kind="exit_ticket"
        label="請 Manna 設計 Exit ticket"
        againLabel="請 Manna 再設計一次"
        shownNote={
          suggestion
            ? `建議在下方，形成性評量的題目都連到你的目標，也附參考答案。${
                uncovered.length > 0 ? `這次沒有檢核到：${uncovered.join("、")}。` : ""
              }`
            : null
        }
      />
      <section className="stage-block" aria-label="Exit ticket 的題目">
        {items.length === 0 && <p className="quiet-note">還沒有題目。</p>}
        {items.map((item, index) => (
          <div key={item.id} className="stage-form" role="group" aria-label={`Exit ticket 第 ${index + 1} 題`}>
            <label className="field-label" htmlFor={`exit-${item.id}-kind`}>
              第 {index + 1} 題的用途
            </label>
            <select
              id={`exit-${item.id}-kind`}
              value={item.kind}
              onChange={(e) => patchItem(item.id, { kind: e.target.value as ExitKind })}
            >
              {EXIT_KINDS.map((k) => (
                <option key={k.value} value={k.value}>
                  {k.label}：{k.hint}
                </option>
              ))}
            </select>
            <label className="field-label" htmlFor={`exit-${item.id}-text`}>
              題目
            </label>
            <textarea
              id={`exit-${item.id}-text`}
              className="print-input"
              rows={2}
              lang="en"
              value={item.text}
              onChange={(e) => patchItem(item.id, { text: e.target.value })}
            />
            {item.kind === "formative" && (
              <>
                <label className="field-label" htmlFor={`exit-${item.id}-objective`}>
                  檢核哪一項目標
                </label>
                <select
                  id={`exit-${item.id}-objective`}
                  value={item.objective_id ?? ""}
                  onChange={(e) => patchItem(item.id, { objective_id: e.target.value })}
                >
                  <option value="">沒有指定</option>
                  {objectives.map((o) => (
                    <option key={o.id} value={o.id}>
                      {labels.get(o.id)}：{o.text}
                    </option>
                  ))}
                </select>
                <label className="field-label" htmlFor={`exit-${item.id}-answer`}>
                  參考答案（教師版）
                </label>
                <input
                  id={`exit-${item.id}-answer`}
                  type="text"
                  lang="en"
                  value={item.answer ?? ""}
                  onChange={(e) => patchItem(item.id, { answer: e.target.value })}
                />
              </>
            )}
            <button
              type="button"
              className="text-link is-plain"
              aria-label={`刪除第 ${index + 1} 題（${exitKindLabel(item.kind)}）`}
              onClick={() => setItems((list) => list.filter((i) => i.id !== item.id))}
            >
              刪除這一題
            </button>
          </div>
        ))}
        <p className="para-actions">
          {EXIT_KINDS.map((k) => (
            <button
              key={k.value}
              type="button"
              className="text-link is-plain"
              onClick={() =>
                setItems((list) => [
                  ...list,
                  { id: newId("e"), kind: k.value, text: "", origin: "teacher", objective_id: "", answer: "" },
                ])
              }
            >
              ＋ {k.label}
            </button>
          ))}
        </p>
        {suggestion?.items.map((s, index) =>
          !job?.decided.includes(index) ? (
            // oxlint-disable-next-line react/no-array-index-key -- the decision is logged by this position
            <MannaWell
              key={index}
              label={exitKindLabel(s.kind)}
              onAdopt={() => take(index, false)}
              onEdit={() => take(index, true)}
              onDismiss={() => decide("exit_ticket", index, "reject")}
            >
              <p lang="en">{s.text}</p>
              {s.objective_id && (
                <p className="why">
                  <span className="quiet-term">檢核</span>
                  {labels.get(s.objective_id)}：{content.objectives.find((o) => o.id === s.objective_id)?.text}
                </p>
              )}
              {s.answer && (
                <p className="why" lang="en">
                  <span className="quiet-term">參考答案</span>
                  {s.answer}
                </p>
              )}
            </MannaWell>
          ) : null,
        )}
      </section>

      <WorksheetChoices sop={sop} />
    </>
  );
}
