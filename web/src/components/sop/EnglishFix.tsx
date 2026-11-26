import { useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { SopEnglishSuggestion, SopTextField } from "../../api/types";
import { useApp } from "../../context/AppContext";
import { logScaffold } from "../../lib/scaffold";
import { ErrorText } from "../common";

interface EnglishFixProps {
  lessonId: number;
  /** Which text this is: a task (step 5), a warm-up activity (step 6) or an exit-ticket item (step 7). */
  field: SopTextField;
  /** What the teacher wrote now. */
  text: string;
  /** Only for a task: the paragraphs it belongs to. */
  paragraphIds?: string[];
  /** The teacher pressed 用這個: the text is to be replaced by Manna's version. */
  onUse: (english: string) => void;
}

interface Proposal extends SopEnglishSuggestion {
  interactionId: number | null;
}

/**
 * 幫我修英文 for the text a teacher writes in steps 5–7: Manna's corrected version is shown next to
 * what the teacher wrote, and nothing is replaced until she presses 用這個.
 */
export function EnglishFix({ lessonId, field, text, paragraphIds, onUse }: EnglishFixProps) {
  const { config } = useApp();
  const aiEnabled = config?.ai_enabled !== false;
  const [asking, setAsking] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [proposal, setProposal] = useState<Proposal | null>(null);
  const empty = !text.trim();

  async function ask() {
    setAsking(true);
    setProblem(null);
    setProposal(null);
    logScaffold("SC-SOP-TEXT-ENGLISH", "expand", { trigger: "request" });
    try {
      const result = await api.sopTextEnglish(lessonId, {
        field,
        text,
        ...(field === "task" ? { paragraph_ids: paragraphIds ?? [] } : {}),
      });
      if (result.status === "shown" && result.output) {
        logScaffold("SC-SOP-TEXT-ENGLISH", "show", { trigger: "suggestion" });
        setProposal({ interactionId: result.interaction_id, ...result.output });
      } else {
        setProblem(result.message ?? "這次沒有改好，請再試一次。");
      }
    } catch (err) {
      setProblem(errorMessage(err));
    } finally {
      setAsking(false);
    }
  }

  function decide(decision: "adopt" | "reject") {
    if (!proposal) return;
    if (proposal.interactionId != null) {
      // Research logging never blocks the teacher.
      void api.sendDecision(proposal.interactionId, { item_index: 0, decision }).catch(() => undefined);
    }
    logScaffold("SC-SOP-TEXT-ENGLISH", decision === "adopt" ? "use" : "skip", { trigger: decision });
    if (decision === "adopt") onUse(proposal.english);
    setProposal(null);
  }

  return (
    <>
      <p className="para-actions">
        <button type="button" className="manna-button" disabled={asking || !aiEnabled || empty} onClick={() => void ask()}>
          <span className="star" aria-hidden="true" />
          {asking ? "Manna 正在看…" : "幫我修英文"}
        </button>
        {empty && aiEnabled && <span className="field-hint">先寫一些文字（寫中文也可以），Manna 才知道要修什麼。</span>}
        {!aiEnabled && <span className="field-hint">AI 功能目前未開放。</span>}
      </p>
      <ErrorText message={problem} />
      {proposal && (
        <div className="manna-well english-proposal" role="group" aria-label="Manna 建議的英文">
          <p className="manna-label">
            <span className="star" aria-hidden="true" />
            Manna 建議：修過的英文
          </p>
          <p className="mine-english pre-line" lang="en">
            <span className="quiet-term">你寫的</span>
            {text}
          </p>
          <p className="proposed-english pre-line" lang="en">
            {proposal.english}
          </p>
          <p className="why">{proposal.note}</p>
          <div className="decide">
            {proposal.changed && (
              <button type="button" className="decide-button is-take" onClick={() => decide("adopt")}>
                用這個
              </button>
            )}
            <button type="button" className="decide-button" onClick={() => decide("reject")}>
              保留我的
            </button>
          </div>
        </div>
      )}
    </>
  );
}
