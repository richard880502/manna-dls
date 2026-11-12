import { useEffect, useState } from "react";
import { api } from "../../api/client";
import type { PreferenceQuestion } from "../../api/types";
import { FALLBACK_PREFERENCE_QUESTIONS, normaliseQuestions } from "../../lib/preferences";

/** Survey questions from the back office; falls back to the built-in copy if the request fails. */
export function usePreferenceQuestions(): { questions: PreferenceQuestion[]; loaded: boolean } {
  const [questions, setQuestions] = useState<PreferenceQuestion[]>(FALLBACK_PREFERENCE_QUESTIONS);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .preferenceQuestions()
      .then((res) => {
        if (!alive) return;
        setQuestions(normaliseQuestions(res?.questions));
        setLoaded(true);
      })
      .catch(() => alive && setLoaded(true));
    return () => {
      alive = false;
    };
  }, []);

  return { questions, loaded };
}
