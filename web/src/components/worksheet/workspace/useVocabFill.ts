import { useEffect, useRef, useState } from "react";
import { bankNeedsFill } from "../../../lib/worksheet";
import type { WorksheetController } from "../useWorksheet";

/**
 * Key Vocabulary in Chinese and the word banks' parts of speech: Manna writes what is missing (one
 * call); what it could not write is said plainly and prints as it is. A worksheet made before the
 * word banks had parts of speech gets them the first time it is open.
 */
export function useVocabFill(ws: WorksheetController) {
  const state = ws.state;
  const [notice, setNotice] = useState<string | null>(null);
  const [filling, setFilling] = useState(false);
  const fill = async (scope: { tables?: boolean; banks?: boolean }) => {
    setFilling(true);
    setNotice(await ws.fillMeanings(scope));
    setFilling(false);
  };
  const banksTried = useRef(false);
  const banksMissing =
    state !== null && !state.running && state.content.blocks.some((b) => b.scaffolds.some((s) => bankNeedsFill(s.bank)));
  useEffect(() => {
    if (!banksMissing || banksTried.current) return;
    banksTried.current = true;
    void fill({ banks: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once per opened worksheet
  }, [banksMissing]);
  const missingZh = state?.content.blocks.some((b) => b.vocabulary.some((v) => !(v.meaning_zh ?? "").trim())) ?? false;
  return { notice, filling, fill, missingZh };
}

export type VocabFill = ReturnType<typeof useVocabFill>;
