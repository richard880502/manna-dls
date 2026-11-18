import { useId, useState } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../../api/client";
import type {
  WsLevel,
  WsOverLevelWord,
  WsVocabAction,
  WsVocabApplied,
  WsVocabCheck,
  WsVocabGlossText,
  WsVocabPlace,
  WsVocabRewrite,
  WsVocabSuggestions,
  WsVocabWordChoice,
} from "../../api/types";
import { ErrorText } from "../common";
import { LEVEL_LABEL } from "../../lib/worksheet";

/** One suggestion request is one model call: the server takes at most this many words. */
export const SUGGEST_CHUNK = 20;

export const ACTION_LABEL: Record<WsVocabAction, string> = {
  gloss: "提供字義",
  replace: "換字",
  leave: "不處理",
};

/** What the teacher has chosen and been given for one word. */
interface Choice {
  action: WsVocabAction | null;
  gloss: WsVocabGlossText;
  /** The call that wrote the meaning shown (null: typed by the teacher). */
  glossFrom: number | null;
  /** The call that rewrote its sentences. */
  replaceFrom: number | null;
  /** The word the rewrites mostly use (a hint). */
  hint: string | null;
  /** Its sentences (keys into the page's places); null until asked. */
  places: string[] | null;
  notes: string[];
}

const emptyChoice = (): Choice => ({
  action: null,
  gloss: { pos: "", meaning_zh: "", definition_en: "" },
  glossFrom: null,
  replaceFrom: null,
  hint: null,
  places: null,
  notes: [],
});

function chunks<T>(list: T[], size: number): T[][] {
  const out: T[][] = [];
  for (let i = 0; i < list.length; i += size) out.push(list.slice(i, i + size));
  return out;
}

/** The before → after of a sentence, the word to avoid marked in the before. */
function Sentence({ place }: { place: WsVocabPlace }) {
  return (
    <span className="ws-vocab-sentence">
      <span className="en ws-vocab-before">{place.original}</span>
      {place.rewritten ? (
        <>
          <span aria-hidden="true"> → </span>
          <span className="en ws-vocab-after">{place.rewritten}</span>
        </>
      ) : (
        <span className="quiet-note">（沒有可用的改寫{place.problem ? `：${place.problem}` : ""}，這裡不會改）</span>
      )}
    </span>
  );
}

/**
 * 處理超出級別的字: for each word above the target level the teacher chooses 提供字義, 換字 or
 * 不處理 (one by one or for all at once) and asks Manna once per action. 換字 rewrites whole
 * sentences: each sentence that has the word is shown before → after, can be unticked, and is
 * applied exactly as shown. The reading text, key words, word banks and quoted words never
 * change.
 */
export function VocabActions({
  worksheetId,
  check,
  levels,
  onApplied,
}: {
  worksheetId: number;
  check: WsVocabCheck;
  /** The worksheet's versions (a gloss can go to all of them). */
  levels: WsLevel[];
  onApplied: (result: WsVocabApplied) => void;
}) {
  const ids = useId();
  const words = check.words ?? [];
  const a = check.analysis;
  const [choices, setChoices] = useState<Record<string, Choice>>({});
  const [places, setPlaces] = useState<Record<string, WsVocabPlace>>({});
  const [ticked, setTicked] = useState<Record<string, boolean>>({});
  const [scope, setScope] = useState<"level" | "all">("level");
  const [asking, setAsking] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [asks, setAsks] = useState(0);

  const choiceOf = (base: string) => choices[base] ?? emptyChoice();
  const patch = (base: string, change: (c: Choice) => Choice) =>
    setChoices((prev) => ({ ...prev, [base]: change(prev[base] ?? emptyChoice()) }));
  const setAction = (w: WsOverLevelWord, action: WsVocabAction | null) =>
    patch(w.base, (c) => ({
      ...c,
      action,
      // A word already glossed starts from the meaning it has.
      gloss: action === "gloss" && !c.gloss.meaning_zh && w.glossed ? { ...c.gloss, meaning_zh: w.glossed } : c.gloss,
    }));
  const setAll = (pick: (w: WsOverLevelWord) => WsVocabAction | null) =>
    words.forEach((w) => {
      const next = pick(w);
      if (next) setAction(w, next);
    });

  /** A sentence is written when it is ticked, has a rewrite, and every word in it is 換字. */
  const usable = (key: string) => {
    const p = places[key];
    return (
      !!p &&
      p.rewritten !== null &&
      ticked[key] !== false &&
      p.bases.every((b) => choiceOf(b).action === "replace")
    );
  };
  const ready = (base: string) => {
    const c = choiceOf(base);
    if (c.action === "gloss") return c.gloss.meaning_zh.trim() !== "";
    if (c.action === "replace") return (c.places ?? []).some(usable);
    return c.action === "leave";
  };

  const pending = (action: "gloss" | "replace") =>
    words.filter((w) => {
      const c = choiceOf(w.base);
      if (c.action !== action) return false;
      return action === "gloss" ? c.glossFrom === null && c.gloss.meaning_zh.trim() === "" : c.places === null;
    });
  const toAsk = [...pending("gloss"), ...pending("replace")];

  async function ask() {
    setAsking(true);
    setError(null);
    setMessage(null);
    const said: string[] = [];
    let n = asks;
    try {
      for (const action of ["gloss", "replace"] as const) {
        for (const part of chunks(pending(action), SUGGEST_CHUNK)) {
          const out: WsVocabSuggestions = await api.suggestWorksheetVocab(
            worksheetId,
            check.id,
            action,
            part.map((w) => w.base),
          );
          // Place ids are only unique within one answer.
          n += 1;
          const key = (id: string) => `${n}:${id}`;
          if (out.message) said.push(out.message);
          setPlaces((prev) => ({
            ...prev,
            ...Object.fromEntries((out.places ?? []).map((p) => [key(p.id), p])),
          }));
          for (const s of out.words) {
            patch(s.base, (c) =>
              action === "gloss"
                ? {
                    ...c,
                    gloss: s.gloss ?? c.gloss,
                    glossFrom: s.gloss ? s.interaction_id : c.glossFrom,
                    notes: s.notes,
                  }
                : {
                    ...c,
                    replaceFrom: s.interaction_id ?? c.replaceFrom,
                    hint: s.replacement,
                    places: s.places.map(key),
                    notes: s.notes,
                  },
            );
          }
        }
      }
      setAsks(n);
      setMessage(said.length > 0 ? said.join(" ") : null);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setAsking(false);
    }
  }

  const chosen = words.filter((w) => choiceOf(w.base).action !== null);
  const notReady = chosen.filter((w) => !ready(w.base));
  const glossing = chosen.some((w) => choiceOf(w.base).action === "gloss");

  async function apply() {
    const body: WsVocabWordChoice[] = chosen.map((w) => {
      const c = choiceOf(w.base);
      if (c.action === "gloss") {
        // A meaning the teacher typed without asking still counts as a gloss; a call that
        // rewrote its sentences is recorded as turned down.
        return { base: w.base, action: "gloss", gloss: c.gloss, interaction_id: c.glossFrom ?? c.replaceFrom };
      }
      if (c.action === "replace") {
        const offered = (c.places ?? []).filter((k) => places[k]?.rewritten).length;
        return { base: w.base, action: "replace", interaction_id: c.replaceFrom, offered };
      }
      return { base: w.base, action: "leave", interaction_id: c.glossFrom ?? c.replaceFrom };
    });
    const keys = [...new Set(chosen.flatMap((w) => choiceOf(w.base).places ?? []))].filter(usable);
    const rewrites: WsVocabRewrite[] = keys.flatMap((k) => {
      const p = places[k];
      if (!p?.rewritten) return [];
      return [
        { field: p.field, start: p.start, end: p.end, original: p.original, rewritten: p.rewritten, bases: p.bases },
      ];
    });
    setApplying(true);
    setError(null);
    setMessage(null);
    try {
      const result = await api.applyWorksheetVocab(worksheetId, check.id, {
        base_version_no: check.version_no,
        gloss_scope: scope,
        words: body,
        rewrites,
      });
      setChoices({});
      setPlaces({});
      setTicked({});
      onApplied(result);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setApplying(false);
    }
  }

  if (words.length === 0) return null;
  const busy = asking || applying;
  const positionsOf = (base: string) => a.above_target.find((x) => x.base === base)?.positions ?? [];

  return (
    <div className="ws-vocab-actions">
      <p className="quiet-note">
        每個字選一種處理方式：<strong>提供字義</strong>保留原字，在這一區最後的 Glossary 附上中文意思；
        <strong>換字</strong>由 Manna 把題目、選項與作答指示裡有這個字的句子各改寫一次（只改那個字附近，用第{" "}
        {check.target_level} 級以內的字），你逐句確認改寫前後再套用；課文、字詞表、字庫與引號內的字不會改，共用的題目其他版本也會一起改；
        <strong>不處理</strong>例如專有名詞或這節課要教的字。
      </p>
      <div className="ws-vocab-bulk" role="group" aria-label="一次選全部">
        <button type="button" className="text-link is-plain" disabled={busy} onClick={() => setAll(() => "gloss")}>
          全部提供字義
        </button>
        <button
          type="button"
          className="text-link is-plain"
          disabled={busy}
          onClick={() => setAll((w) => (w.replaceable > 0 ? "replace" : null))}
        >
          可以換的全部換字
        </button>
        <button type="button" className="text-link is-plain" disabled={busy} onClick={() => setAll(() => "leave")}>
          全部不處理
        </button>
        <button type="button" className="text-link is-plain" disabled={busy} onClick={() => setAll((w) => w.suggested)}>
          照建議選
        </button>
        <button
          type="button"
          className="text-link is-plain"
          disabled={busy}
          onClick={() => {
            setChoices({});
            setPlaces({});
            setTicked({});
          }}
        >
          清除
        </button>
      </div>

      <table className="ws-plain ws-vocab-table">
        <thead>
          <tr>
            <th scope="col">詞</th>
            <th scope="col">級別</th>
            <th scope="col">出現在</th>
            <th scope="col">處理方式</th>
          </tr>
        </thead>
        <tbody>
          {words.map((w) => {
            const c = choiceOf(w.base);
            const group = `${ids}-${w.base}`;
            return (
              <tr key={w.base}>
                <td className="en">
                  {w.base}
                  {w.pos && <span className="quiet-note"> {w.pos}</span>}
                </td>
                <td>
                  {w.level}
                  {w.count > 1 ? `（${w.count} 次）` : ""}
                </td>
                <td>
                  <ul className="ws-positions">
                    {positionsOf(w.base).map((p) => (
                      <li key={`${p.line}-${p.col}`}>
                        <span>{check.lines[p.line - 1] ?? `第 ${p.line} 行`}</span>
                        <span className="en">{p.context}</span>
                      </li>
                    ))}
                  </ul>
                  {w.in_material && <p className="quiet-note">課文裡也有這個字（課文不會改）。</p>}
                  {w.fixed.length > 0 && <p className="quiet-note">換字不會動：{w.fixed.join("、")}</p>}
                </td>
                <td>
                  {(w.glossed || w.kept) && (
                    <p className="ws-vocab-status">{w.glossed ? `已提供字義：${w.glossed}` : "已標記不處理"}</p>
                  )}
                  <fieldset className="ws-vocab-choice" disabled={busy}>
                    <legend className="sr-only">{`${w.base} 的處理方式`}</legend>
                    {(["gloss", "replace", "leave"] as const).map((action) => (
                      <label key={action}>
                        <input
                          type="radio"
                          name={group}
                          checked={c.action === action}
                          disabled={action === "replace" && w.replaceable === 0}
                          aria-label={`${w.base}：${ACTION_LABEL[action]}`}
                          onChange={() => setAction(w, action)}
                        />
                        <span>{ACTION_LABEL[action]}</span>
                      </label>
                    ))}
                  </fieldset>
                  <p className="quiet-note">
                    建議：{ACTION_LABEL[w.suggested]}。{w.reason}
                  </p>
                  {c.action === "gloss" && (
                    <div className="ws-vocab-gloss">
                      <label>
                        中文意思
                        <input
                          type="text"
                          lang="zh-Hant"
                          maxLength={60}
                          aria-label={`${w.base} 的中文意思`}
                          value={c.gloss.meaning_zh}
                          onChange={(e) => patch(w.base, (x) => ({ ...x, gloss: { ...x.gloss, meaning_zh: e.target.value } }))}
                        />
                      </label>
                      <label>
                        詞性
                        <input
                          type="text"
                          lang="en"
                          maxLength={20}
                          aria-label={`${w.base} 的詞性`}
                          value={c.gloss.pos}
                          onChange={(e) => patch(w.base, (x) => ({ ...x, gloss: { ...x.gloss, pos: e.target.value } }))}
                        />
                      </label>
                      <label>
                        英文解釋（可留空）
                        <input
                          type="text"
                          lang="en"
                          maxLength={200}
                          aria-label={`${w.base} 的英文解釋`}
                          value={c.gloss.definition_en}
                          onChange={(e) =>
                            patch(w.base, (x) => ({ ...x, gloss: { ...x.gloss, definition_en: e.target.value } }))
                          }
                        />
                      </label>
                    </div>
                  )}
                  {c.action === "replace" &&
                    (c.places === null ? (
                      <p className="quiet-note">按「請 Manna 建議」取得改寫後的句子。</p>
                    ) : (
                      <fieldset className="ws-vocab-sentences" disabled={busy}>
                        <legend className="quiet-note">
                          {c.hint ? `主要換成 ${c.hint}。` : ""}勾選的句子會照這樣寫進學習單：
                        </legend>
                        {c.places.map((k) => {
                          const p = places[k];
                          if (!p) return null;
                          const others = p.bases.filter((b) => b !== w.base && choiceOf(b).action !== "replace");
                          return (
                            <label key={k}>
                              <input
                                type="checkbox"
                                checked={!!p.rewritten && ticked[k] !== false}
                                disabled={!p.rewritten}
                                aria-label={`${w.base}：${p.place}`}
                                onChange={(e) => setTicked((prev) => ({ ...prev, [k]: e.target.checked }))}
                              />
                              <span className="quiet-note">{p.place}</span>
                              <Sentence place={p} />
                              {others.length > 0 && (
                                <span className="quiet-note">
                                  （這一句也改了 {others.join("、")}，那個字也要選換字才會套用）
                                </span>
                              )}
                            </label>
                          );
                        })}
                      </fieldset>
                    ))}
                  {c.notes.length > 0 && <p className="quiet-note">說明：{c.notes.join("；")}</p>}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

      {glossing && levels.length > 1 && (
        <fieldset className="ws-vocab-scope" disabled={busy}>
          <legend>字義加在</legend>
          <label>
            <input type="radio" name={`${ids}-scope`} checked={scope === "level"} onChange={() => setScope("level")} />
            <span>只有{LEVEL_LABEL[check.level]}</span>
          </label>
          <label>
            <input type="radio" name={`${ids}-scope`} checked={scope === "all"} onChange={() => setScope("all")} />
            <span>所有印出這個字的版本</span>
          </label>
        </fieldset>
      )}

      <p className="ws-vocab-submit">
        <button type="button" className="secondary-button" disabled={busy || toAsk.length === 0} onClick={() => void ask()}>
          {asking ? "Manna 正在想…" : `請 Manna 建議${toAsk.length > 0 ? `（${toAsk.length} 個字）` : ""}`}
        </button>
        <button
          type="button"
          className="primary-button"
          disabled={busy || chosen.length === 0 || notReady.length > 0}
          onClick={() => void apply()}
        >
          {applying ? "套用中…" : `套用到學習單${chosen.length > 0 ? `（${chosen.length} 個字）` : ""}`}
        </button>
      </p>
      {notReady.length > 0 && (
        <p className="quiet-note" role="status">
          還差：{notReady.map((w) => w.base).join("、")}（提供字義要有中文意思，換字要至少勾選一句可用的改寫）。
        </p>
      )}
      {message && (
        <p className="notice" role="status">
          {message}
        </p>
      )}
      <ErrorText message={error} />
      {error && (
        <p className="quiet-note">
          學習單在分析之後改過的話，請按上方「重新分析」再處理；也可以<Link to={`/worksheets/${worksheetId}`}>回學習單修改</Link>。
        </p>
      )}
    </div>
  );
}

/** What 套用 did, in one sentence. */
export function appliedSummary(r: WsVocabApplied): string {
  const parts: string[] = [];
  if (r.glossed.length > 0) parts.push(`提供字義 ${r.glossed.length} 個`);
  if (r.replaced.length > 0) {
    const sentences = r.replaced.reduce((n, x) => n + x.sentences, 0);
    parts.push(`換字 ${r.replaced.length} 個（改寫 ${sentences} 句）`);
  }
  if (r.left.length > 0) parts.push(`不處理 ${r.left.length} 個`);
  const done = parts.join("、");
  return r.version_no === null
    ? `已記下：${done}。學習單沒有改變。`
    : `已寫成第 ${r.version_no} 版：${done}。在學習單上可以逐區「復原上一版」。`;
}
