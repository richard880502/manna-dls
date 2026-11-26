import { useState } from "react";
import { errorMessage } from "../../api/client";
import type { ObjectiveDraftRequest } from "../../api/types";
import { ocrBusyError } from "../../lib/ocr";
import type { DraftDecision } from "../../lib/objectives";
import {
  ObjectiveDrafts,
  recordDecisions,
  useObjectiveDraft,
  type DraftOutcome,
} from "../objectives/ObjectiveDrafts";

/**
 * What the home page's quick box and /create share when they start a 備課 conversation: busy and
 * error state, 「請先選材料」, Manna's objective drafts, and the refusal while a material is still
 * being read (OCR, 409). What goes into the request, and where the material lives, stays with each page.
 *
 * `attachedKey` names what is attached now (material and sample); the OCR refusal is shown for as
 * long as it stays the same.
 */
export function useStartFlow(attachedKey: string) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** The send was refused because a material is still being read: said beside the send button. */
  const [ocrRefusal, setOcrRefusal] = useState<{ key: string; message: string } | null>(null);
  const [needMaterial, setNeedMaterial] = useState(false);
  /** G2: a tool sent without objectives gets Manna's drafts first. */
  const drafts = useObjectiveDraft();
  const ocrBusy = ocrRefusal?.key === attachedKey ? ocrRefusal.message : null;

  /** A failed send or draft: still being read is said beside the button, anything else as an error. */
  function fail(err: unknown) {
    const reading = ocrBusyError(err);
    if (reading) setOcrRefusal({ key: attachedKey, message: reading });
    else setError(errorMessage(err));
  }

  /**
   * Runs the send: busy while it goes, and on failure the message (busy ends). On success busy stays
   * on, since the page is being left.
   */
  async function submit(run: () => Promise<void>) {
    setBusy(true);
    setError(null);
    setOcrRefusal(null);
    try {
      await run();
    } catch (err) {
      fail(err);
      setBusy(false);
    }
  }

  /** Asks Manna for objectives. Resolves true when none opened and nothing failed: go on without them. */
  async function askDrafts(body: ObjectiveDraftRequest): Promise<boolean> {
    setError(null);
    setOcrRefusal(null);
    try {
      return !(await drafts.start(body));
    } catch (err) {
      fail(err);
      return false;
    }
  }

  /** The panel's answer is taken: it closes and the teacher's choices are recorded. */
  async function adopt(decisions: DraftDecision[]) {
    drafts.close();
    await recordDecisions(decisions);
  }

  return {
    busy,
    setBusy,
    error,
    setError,
    ocrBusy,
    needMaterial,
    setNeedMaterial,
    drafts,
    submit,
    askDrafts,
    adopt,
  };
}

export type StartFlow = ReturnType<typeof useStartFlow>;

/** Manna's objective drafts for a start flow (nothing while none are open). */
export function StartDrafts({
  flow,
  onUse,
  onSkip,
  embedded,
  fill,
}: {
  flow: StartFlow;
  onUse: (outcome: DraftOutcome, decisions: DraftDecision[]) => void;
  /** After the panel closed and the decisions were recorded. */
  onSkip: () => void;
  embedded?: boolean;
  fill?: boolean;
}) {
  const { drafts, busy, setError } = flow;
  if (!drafts.state) return null;
  return (
    <ObjectiveDrafts
      embedded={embedded}
      fill={fill}
      state={drafts.state}
      disabled={busy}
      onUse={onUse}
      onSkip={(decisions) => {
        drafts.close();
        void recordDecisions(decisions);
        onSkip();
      }}
      onAnother={(plan) => void drafts.another(plan)}
      onLanguage={(plan) => void drafts.toLanguage(plan).catch((err) => setError(errorMessage(err)))}
      onCancel={drafts.close}
    />
  );
}
