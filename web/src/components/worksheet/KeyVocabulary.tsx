import type { ReactNode } from "react";
import { keyVocabularyGroups } from "../../lib/worksheet";

/** One row to draw: the cells are nodes, so the editor can put inputs in them. */
export interface KeyVocabularyRow {
  id: string;
  task: boolean;
  word: ReactNode;
  pos: ReactNode;
  meaning: ReactNode;
  /** The teacher's extra column (the word-list level); only drawn when `extraHead` is given. */
  extra?: ReactNode;
}

/**
 * Key Vocabulary: Content (the key words of the text) and Academic (the task words of the
 * instructions) side by side, each a `Word | POS | Meaning` table; stacked on a narrow screen;
 * one column only when the other group is empty. The student preview and the editor draw it the
 * same way (the exports print the same two tables).
 */
export function KeyVocabulary({
  rows,
  label,
  extraHead,
}: {
  rows: KeyVocabularyRow[];
  /** The tables' accessible name: 「字詞表 B」 */
  label: string;
  extraHead?: ReactNode;
}) {
  const groups = keyVocabularyGroups(rows);
  if (groups.length === 0) return null;
  return (
    <div className="ws-keyvocab">
      <h4 className="ws-keyvocab-title">Key Vocabulary</h4>
      <div className={`ws-keyvocab-cols${groups.length === 2 ? " is-two" : ""}`}>
        {groups.map((g) => (
          <div key={g.title} className="ws-keyvocab-col">
            <p className="ws-keyvocab-group">{g.title}</p>
            <table className="ws-vocab" aria-label={groups.length === 2 ? `${label}（${g.title}）` : label}>
              <thead>
                <tr>
                  <th scope="col">Word</th>
                  <th scope="col">POS</th>
                  <th scope="col">Meaning</th>
                  {extraHead && <th scope="col">{extraHead}</th>}
                </tr>
              </thead>
              <tbody>
                {g.rows.map((r) => (
                  <tr key={r.id}>
                    <td>{r.word}</td>
                    <td className="ws-keyvocab-pos">{r.pos}</td>
                    <td>{r.meaning}</td>
                    {extraHead && <td>{r.extra}</td>}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ))}
      </div>
    </div>
  );
}
