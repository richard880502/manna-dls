import { useId, useState } from "react";
import type { WsBankWord, WsScaffold, WsVocabLang, WsWordBank } from "../../api/types";
import { POS_OPTIONS, bankWithText } from "../../lib/worksheet";
import { SheetText } from "./SheetText";
import type { WorksheetController } from "./useWorksheet";

const LANG_LABEL: Record<WsVocabLang, string> = { en: "英文", zh: "中文" };

/** The meaning a ticked word prints, in the bank's language (the other one when it has none). */
export function bankMeaning(w: WsBankWord, lang: WsVocabLang): string {
  const en = (w.definition ?? "").trim();
  const zh = (w.meaning_zh ?? "").trim();
  return lang === "zh" && zh ? zh : en || zh;
}

/**
 * 字詞表鷹架 (Word bank): every word prints with its part of speech; the teacher ticks the words
 * whose meaning prints too and picks the language of those meanings (one choice for the bank).
 * The words themselves are one line of text (comma separated); the words still listed keep their
 * part of speech, meanings and ticks.
 */
export function WordBankEditor({
  ws,
  scaffold,
  bank,
  name,
  locked,
  onChange,
}: {
  ws: WorksheetController;
  scaffold: WsScaffold;
  bank: WsWordBank;
  name: string;
  locked: boolean;
  onChange: (change: (s: WsScaffold) => WsScaffold) => void;
}) {
  const radio = useId();
  const [notice, setNotice] = useState<string | null>(null);
  const lang: WsVocabLang = bank.lang ?? "en";
  const setBank = (next: WsWordBank) => onChange((s) => ({ ...s, bank: next, content: next.words.map((w) => w.word).join(", ") }));
  const patchWord = (i: number, change: Partial<WsBankWord>) =>
    setBank({ ...bank, words: bank.words.map((w, j) => (j === i ? { ...w, ...change } : w)) });
  // Ticking a word that has no meaning yet: Manna writes the missing ones (one call).
  const tick = async (i: number, on: boolean) => {
    const w = bank.words[i]!;
    patchWord(i, { show_meaning: on });
    if (on && !(w.definition ?? "").trim() && !(w.meaning_zh ?? "").trim()) {
      setNotice(await ws.fillMeanings({ banks: true }));
    }
  };
  return (
    <div className="ws-bank">
      <SheetText
        label={`${scaffold.kind}（${name}）`}
        value={scaffold.content}
        disabled={locked}
        onChange={(text) => {
          const next = bankWithText(bank, text);
          onChange((s) => (next ? { ...s, bank: next, content: text } : { ...s, bank: undefined, content: text }));
        }}
      />
      <ul className="ws-bank-words" lang="zh-Hant">
        {bank.words.map((w, i) => (
          <li key={`${w.word}-${i}`}>
            <label className="check-row">
              <input
                type="checkbox"
                aria-label={`${w.word} 印出字義`}
                checked={w.show_meaning === true}
                disabled={locked}
                onChange={(e) => void tick(i, e.target.checked)}
              />
              <span lang="en">{w.word}</span>
            </label>
            <select
              className="ws-pos"
              aria-label={`${w.word} 的詞性（字詞表）`}
              value={w.pos ?? ""}
              disabled={locked}
              onChange={(e) => patchWord(i, { pos: e.target.value })}
            >
              <option value="">（未填）</option>
              {POS_OPTIONS.map((p) => (
                <option key={p} value={p}>
                  {p}
                </option>
              ))}
            </select>
            {w.show_meaning === true && (
              <span className="ws-vocab-level" lang={lang === "zh" ? "zh-Hant" : "en"}>
                {bankMeaning(w, lang) || "（還沒有字義）"}
              </span>
            )}
          </li>
        ))}
      </ul>
      <div className="ws-bank-lang" role="radiogroup" aria-label={`字詞表字義語言（${name}）`} lang="zh-Hant">
        <span>勾選的字印出的字義：</span>
        {(["en", "zh"] as const).map((l) => (
          <label key={l} className="check-row">
            <input
              type="radio"
              name={`${radio}-lang`}
              checked={lang === l}
              disabled={locked}
              onChange={() => setBank({ ...bank, lang: l })}
            />
            <span>{LANG_LABEL[l]}</span>
          </label>
        ))}
      </div>
      {notice && (
        <p className="field-hint" role="status" lang="zh-Hant">
          {notice}
        </p>
      )}
    </div>
  );
}
