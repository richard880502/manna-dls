import { useEffect, useId, useState } from "react";
import { Link, useParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { VocabularyListed, WorksheetState, WsLevel, WsVocabCheck } from "../api/types";
import { ErrorText, Loading } from "../components/common";
import { VocabActions, appliedSummary } from "../components/worksheet/VocabActions";
import { LEVEL_LABEL, formatDateTime, percent } from "../lib/worksheet";

const DEFAULT_TARGET = 4;

function ListedWords({ title, note, words }: { title: string; note: string; words: VocabularyListed[] }) {
  const ids = useId();
  if (words.length === 0) return null;
  return (
    <section className="ws-check-list" aria-labelledby={ids}>
      <h3 id={ids}>
        {title}（{words.length}）
      </h3>
      <p className="quiet-note">{note}</p>
      <p className="en ws-word-run">
        {words.map((w) => (w.count > 1 ? `${w.word} ×${w.count}` : w.word)).join(" · ")}
      </p>
    </section>
  );
}

/**
 * 詞彙級別檢查 (spec §9.6, the deterministic part): the student version of one level against the
 * imported CEEC word list. No model call. The basis, the limits and the disclaimer come from the
 * server and are always shown with the numbers.
 */
export function WorksheetCheckPage() {
  const { worksheetId } = useParams();
  const id = Number(worksheetId);
  const ids = useId();
  const [state, setState] = useState<WorksheetState | null>(null);
  const [checks, setChecks] = useState<WsVocabCheck[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [level, setLevel] = useState<WsLevel | null>(null);
  const [target, setTarget] = useState(DEFAULT_TARGET);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [applied, setApplied] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    Promise.all([api.getWorksheet(id), api.worksheetChecks(id)])
      .then(([ws, list]) => {
        if (!alive) return;
        setState(ws);
        setChecks(list);
        const first = list[0]?.level ?? ws.content.levels[0] ?? null;
        setLevel(first);
        const firstCheck = list.find((c) => c.level === first);
        if (firstCheck) setTarget(firstCheck.target_level);
      })
      .catch((err: unknown) => alive && setLoadError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [id]);

  if (loadError) {
    return (
      <>
        <ErrorText message={loadError} />
        <p className="after-rows">
          <Link to="/worksheets">回學習單</Link>
        </p>
      </>
    );
  }
  if (!state) return <Loading />;

  const levels = state.content.levels;
  const check = checks.find((c) => c.level === level) ?? null;

  async function analyse() {
    if (!level) return;
    setRunning(true);
    setError(null);
    setApplied(null);
    try {
      const result = await api.checkWorksheetVocabulary(id, level, target);
      setChecks((prev) => [result, ...prev.filter((c) => c.level !== result.level)]);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setRunning(false);
    }
  }

  const a = check?.analysis ?? null;
  const maxTypes = a ? Math.max(1, ...a.level_distribution.map((d) => d.types)) : 1;
  const excluded = a?.basis.excluded_tokens;

  return (
    <>
      <div className="head-row page-head">
        <div>
          <p className="crumbs">
            <Link to="/worksheets">學習單</Link> / <Link to={`/worksheets/${id}`}>{state.title || "未命名的學習單"}</Link>
          </p>
          <h1>詞彙級別檢查</h1>
          <p className="lead">對照大考中心高中英文參考詞彙表，看這份學習單的學生版用了哪些級別的字。</p>
        </div>
        <Link className="text-link" to={`/worksheets/${id}`}>
          回學習單修改
        </Link>
      </div>

      <p className="ws-disclaimer" role="note">
        {a?.disclaimer ??
          "本分析為備課輔助，依公開詞彙表以規則比對，不是大考中心的官方評定、命題預測或品質保證。"}
        {check && (
          <>
            {" "}
            分析的是「{LEVEL_LABEL[check.level]}」學生版第 {check.version_no} 版（{formatDateTime(check.checked_at)}）。
          </>
        )}
      </p>
      {check?.stale && (
        <p className="notice notice-warn" role="status">
          這是第 {check.version_no} 版的結果，學習單之後改過，請重新分析。
        </p>
      )}

      {levels.length === 0 ? (
        <p className="empty-line">
          這份學習單還沒有產生內容，沒有東西可以分析。<Link to={`/worksheets/${id}`}>回學習單</Link>
        </p>
      ) : (
        <form
          className="ws-check-form"
          onSubmit={(e) => {
            e.preventDefault();
            void analyse();
          }}
        >
          <label htmlFor={`${ids}-level`}>
            版本
            <select id={`${ids}-level`} value={level ?? ""} onChange={(e) => setLevel(e.target.value as WsLevel)}>
              {levels.map((l) => (
                <option key={l} value={l}>
                  {LEVEL_LABEL[l]}
                </option>
              ))}
            </select>
          </label>
          <label htmlFor={`${ids}-target`}>
            目標級別
            <select id={`${ids}-target`} value={target} onChange={(e) => setTarget(Number(e.target.value))}>
              {[1, 2, 3, 4, 5, 6].map((n) => (
                <option key={n} value={n}>
                  第 1 到 {n} 級
                </option>
              ))}
            </select>
          </label>
          <button type="submit" className="primary-button" disabled={running}>
            {running ? "分析中…" : check ? "重新分析" : "分析"}
          </button>
        </form>
      )}
      <ErrorText message={error} />

      {a && check && (
        <>
          <section className="section" aria-labelledby={`${ids}-dist`}>
            <h2 id={`${ids}-dist`}>詞彙級別分布</h2>
            <p className="quiet-note">
              詞彙表版本：{a.wordlist_version}。目標級別：第 1 到 {a.target_level} 級。計入 {a.basis.counted_tokens}{" "}
              個詞次、{a.basis.counted_types} 個不同的詞（已還原成基本詞形）。下方比例＝該級不同詞數 ÷{" "}
              {a.basis.counted_types}。
            </p>
            <p className="quiet-note">{a.basis.denominator_note}</p>
            {excluded && (
              <p className="quiet-note">
                未計入的詞次：專有名詞 {excluded.proper_nouns}、未收錄 {excluded.unknown}、連字號與複合詞／片語{" "}
                {excluded.multiword}、附錄詞彙 {excluded.appendix}。
              </p>
            )}
            <table className="ws-bars" aria-label="各級詞彙數量與比例">
              <thead>
                <tr>
                  <th scope="col">級別</th>
                  <th scope="col" aria-hidden="true" />
                  <th scope="col">不同的詞</th>
                  <th scope="col">詞次</th>
                </tr>
              </thead>
              <tbody>
                {a.level_distribution.map((d) => (
                  <tr key={d.level} className={d.level > a.target_level ? "is-over" : ""}>
                    <th scope="row">
                      第 {d.level} 級{d.level > a.target_level ? "（超出）" : ""}
                    </th>
                    <td aria-hidden="true">
                      <i style={{ width: `${Math.round((d.types / maxTypes) * 100)}%` }} />
                    </td>
                    <td>
                      {d.types} 個 · {percent(d.type_ratio)}
                    </td>
                    <td>
                      {d.tokens} 次 · {percent(d.token_ratio)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>

          <section className="section" aria-labelledby={`${ids}-above`}>
            <h2 id={`${ids}-above`}>超出目標級別的詞</h2>
            {applied && (
              <p className="notice" role="status">
                {applied} <Link to={`/worksheets/${id}`}>回學習單看看</Link>
              </p>
            )}
            {a.above_target.length === 0 ? (
              <p className="empty-line">沒有超出第 {a.target_level} 級的詞（未收錄的詞不在此列，請看下方）。</p>
            ) : (check.words?.length ?? 0) > 0 && !check.stale ? (
              <VocabActions
                key={check.id}
                worksheetId={id}
                check={check}
                levels={levels}
                onApplied={(result) => {
                  setState(result.worksheet);
                  setChecks((prev) => [result.check, ...prev.filter((c) => c.level !== result.check.level)]);
                  setApplied(appliedSummary(result));
                }}
              />
            ) : (
              <table className="ws-plain">
                <thead>
                  <tr>
                    <th scope="col">詞</th>
                    <th scope="col">級別</th>
                    <th scope="col">出現在</th>
                  </tr>
                </thead>
                <tbody>
                  {a.above_target.map((w) => (
                    <tr key={w.base}>
                      <td className="en">{w.base}</td>
                      <td>
                        {w.level}
                        {w.count > 1 ? `（${w.count} 次）` : ""}
                      </td>
                      <td>
                        <ul className="ws-positions">
                          {w.positions.map((p) => (
                            <li key={`${p.line}-${p.col}`}>
                              <span>{check.lines[p.line - 1] ?? `第 ${p.line} 行`}</span>
                              <span className="en">{p.context}</span>
                            </li>
                          ))}
                        </ul>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>

          <section className="section" aria-labelledby={`${ids}-other`}>
            <h2 id={`${ids}-other`}>另外列出、不歸入任何一級</h2>
            <ListedWords title="專有名詞" note="句中大寫且不在詞彙表內的詞。" words={a.proper_nouns} />
            <ListedWords
              title="未收錄"
              note="詞彙表查不到的詞。不計入任何一級，所以只看比例會低估難度。"
              words={a.unknown}
            />
            <ListedWords
              title="連字號與複合詞、片語"
              note="詞彙表不收片語與多數複合詞，這裡只列出，不歸級。"
              words={a.multiword}
            />
            <ListedWords title="附錄詞彙" note="數字、星期、月份、國名等，詞彙表未分級。" words={a.appendix_words} />
            {a.proper_nouns.length + a.unknown.length + a.multiword.length + a.appendix_words.length === 0 && (
              <p className="empty-line">沒有這幾類的詞。</p>
            )}

            {a.ambiguous_forms.length > 0 && (
              <section className="ws-check-list" aria-label="詞形歧義">
                <h3>詞形歧義（{a.ambiguous_forms.length}）</h3>
                <p className="quiet-note">同一個字形可以對到不同級別的詞，取較低的一級。</p>
                <p className="en ws-word-run">
                  {a.ambiguous_forms
                    .map((f) => `${f.word} → ${f.base}（${f.level}${f.also_listed_level ? `／${f.also_listed_level}` : ""}）`)
                    .join(" · ")}
                </p>
              </section>
            )}
            {a.derived_matches.length > 0 && (
              <section className="ws-check-list" aria-label="推定對應">
                <h3>規則衍生的推定對應（{a.derived_matches.length}）</h3>
                <p className="quiet-note">詞彙表不列規則衍生詞，這些是依字首字尾規則推回原詞的級別。</p>
                <p className="en ws-word-run">
                  {a.derived_matches.map((f) => `${f.word} → ${f.base}（${f.level}）`).join(" · ")}
                </p>
              </section>
            )}
            {a.multi_level_words.length > 0 && (
              <section className="ws-check-list" aria-label="多級別詞">
                <h3>多級別詞（{a.multi_level_words.length}）</h3>
                <p className="quiet-note">詞彙表把這些詞列在不只一級，取最低級。</p>
                <p className="en ws-word-run">
                  {a.multi_level_words.map((m) => `${m.base}（${m.levels.join("／")}，用 ${m.used_level}）`).join(" · ")}
                </p>
              </section>
            )}
          </section>

          <section className="section" aria-labelledby={`${ids}-limits`}>
            <h2 id={`${ids}-limits`}>這份分析的限制</h2>
            <ul className="ws-limits">
              {a.limits.map((l) => (
                <li key={l}>{l}</li>
              ))}
            </ul>
          </section>
        </>
      )}
    </>
  );
}
