import { useEffect, useState, type FormEvent } from "react";
import { api, errorMessage } from "../../api/client";
import type { TeacherCase } from "../../api/types";
import { logScaffold } from "../../lib/scaffold";
import { ErrorText } from "../common";

const TRACK_LABELS: Record<TeacherCase["school_track"], string> = {
  general: "普高",
  vocational: "技高",
  both: "普高・技高",
};

interface CasesPanelProps {
  /** Reports how many cases the first load found (for the 「其他老師怎麼教（n）」 count). Pass a stable function. */
  onLoaded?: (count: number) => void;
}

/**
 * 其他老師怎麼教, inside the lesson page's 「更多備課工具」. The section starts collapsed, so the
 * first load logs `SC-CASES show` with trigger `collapsed`; opening and closing the section is
 * logged by the lesson page (`expand` / `skip`, trigger `section`).
 */
export function CasesPanel({ onLoaded }: CasesPanelProps) {
  const [cases, setCases] = useState<TeacherCase[] | null>(null);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function load(q?: string) {
    setError(null);
    try {
      const list = await api.listTeacherCases(q);
      setCases(list);
      return list;
    } catch (err) {
      setError(errorMessage(err));
      return null;
    }
  }

  useEffect(() => {
    let alive = true;
    api
      .listTeacherCases()
      .then((list) => {
        if (!alive) return;
        setCases(list);
        onLoaded?.(list.length);
        if (list.length > 0) logScaffold("SC-CASES", "show", { trigger: "collapsed" });
      })
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [onLoaded]);

  function onSearch(event: FormEvent) {
    event.preventDefault();
    logScaffold("SC-CASES", "expand", { trigger: "search" });
    void load(query.trim() || undefined);
  }

  function toggleCase(code: string) {
    if (open === code) {
      setOpen(null);
      return;
    }
    setOpen(code);
    logScaffold("SC-CASES", "expand", { trigger: code });
  }

  return (
    <div className="teacher-case-panel">
      <p className="field-hint">教師經驗，非官方來源。</p>
      <form className="peer-search-row" onSubmit={onSearch}>
        <input
          type="search"
          aria-label="搜尋教師經驗"
          placeholder="關鍵字，例如：倒裝句、口說"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <button className="secondary-button" type="submit">
          搜尋
        </button>
      </form>
      <ErrorText message={error} />
      {cases && cases.length === 0 && <p className="empty-preview">目前沒有符合的教師經驗。</p>}
      {cases && cases.length > 0 && (
        <div className="teacher-case-list">
          {cases.map((c) => (
            <article key={c.case_code}>
              <div>
                <strong>
                  {c.topic}｜{c.grade}・{TRACK_LABELS[c.school_track]}
                </strong>
                <p>{c.summary}</p>
                {open === c.case_code && <p className="case-details">{c.details}</p>}
              </div>
              <button
                type="button"
                className="ghost-button case-read"
                aria-expanded={open === c.case_code}
                onClick={() => toggleCase(c.case_code)}
              >
                {open === c.case_code ? "收合" : "閱讀"}
              </button>
            </article>
          ))}
        </div>
      )}
    </div>
  );
}
