import { useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { MemoryCandidate, MemoryScopeCode, MemoryState, TeacherMemory } from "../../api/types";
import { ErrorText, Loading, Notice } from "../common";
import { Dialog } from "../Dialog";
import { formatDateTime } from "../../lib/labels";
import { MemorySuggestionCards } from "../memory/MemorySuggestions";

const DEFAULT_MAX = 100;
const DEFAULT_SCOPES: { code: MemoryScopeCode; name: string }[] = [
  { code: "content", name: "題目內容" },
  { code: "format", name: "題型與作答方式" },
  { code: "layout", name: "版面與格式" },
  { code: "language", name: "語言與用字" },
  { code: "scaffold", name: "鷹架與支持" },
  { code: "workflow", name: "備課活動與流程" },
  { code: "all", name: "全部" },
];
const CLOSED_GROUP = "固定類型（選項數、說明語言等）";

export function TeacherMemories() {
  const [data, setData] = useState<MemoryState | null>(null);
  const [text, setText] = useState("");
  const [candidate, setCandidate] = useState<MemoryCandidate | null>(null);
  const [removing, setRemoving] = useState<TeacherMemory | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const field = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    let alive = true;
    api.memories().then((d) => { if (alive) setData(d); })
      .catch((e: unknown) => { if (alive) setError(errorMessage(e)); });
    return () => { alive = false; };
  }, []);

  async function run(action: () => Promise<void>) {
    setBusy(true); setError(null); setMessage(null);
    try { await action(); } catch (e) { setError(errorMessage(e)); }
    finally { setBusy(false); }
  }

  async function preview() {
    await run(async () => {
      const next = await api.previewMemory(text);
      setCandidate(next);
    });
  }

  async function confirm() {
    if (!candidate) return;
    await run(async () => {
      const saved = await api.confirmMemory(candidate.id);
      setCandidate(null); setText("");
      setData(await api.memories());
      setMessage(saved.unchanged ? "這項習慣已經記住了，沒有新增版本。" : "已記住這項習慣，舊版本會保留在歷程。");
    });
  }

  /** 取消預覽要告訴伺服器（研究紀錄老師沒有採用）；改寫草稿也算取消。 */
  async function cancel() {
    if (!candidate) return;
    await run(async () => {
      await api.cancelMemory(candidate.id);
      setCandidate(null);
      setMessage("已取消，沒有記住這項習慣。");
    });
  }

  function discard() {
    if (!candidate) return;
    const id = candidate.id;
    setCandidate(null);
    api.cancelMemory(id).catch(() => undefined);
  }

  async function remove() {
    if (!removing) return;
    await run(async () => {
      await api.removeMemory(removing.id);
      setRemoving(null); setCandidate(null);
      setData(await api.memories());
      setMessage("已停止使用這項習慣，歷程仍保留。");
    });
  }

  const reload = async () => setData(await api.memories());
  const max = data?.max_active ?? DEFAULT_MAX;
  const active = data?.memories.filter((m) => !m.invalid_at) ?? [];
  const needle = search.trim().toLowerCase();
  const matching = needle ? active.filter((m) => m.statement.toLowerCase().includes(needle)) : active;
  // 固定類型 first, then the free-form habits by where they apply.
  const groups: { name: string; items: TeacherMemory[] }[] = [
    { name: CLOSED_GROUP, items: matching.filter((m) => !m.scope) },
    ...(data?.scopes ?? DEFAULT_SCOPES).map((sc) => ({ name: sc.name, items: matching.filter((m) => m.scope === sc.code) })),
  ].filter((g) => g.items.length > 0);
  const history = data?.memories.filter((m) => m.invalid_at) ?? [];
  const replaced = data?.memories.find((m) => m.id === candidate?.replaces_id);
  const replacedText = replaced?.statement ?? candidate?.replaces_statement ?? null;
  return (
    <section className="panel-card teacher-memories" aria-labelledby="memories-heading">
      <h2 id="memories-heading">我的出題習慣</h2>
      <p>每次先整理一項，確認後才記住。可以修改或停止使用，每一步歷程都會保留。最多 {max} 項。</p>
      <p className="field-hint">可以寫固定類型（選項數、配對組數、避免 Which of the following 開頭、說明與題目語言、參考答案、字彙提示、句長、難度及題號），也可以寫其他教學習慣，例如「每份學習單最後附一題反思問題」。請只寫教學形式，不寫學生姓名、健康、家庭或成績。備課時你說的「之後都…」也可能被偵測到，但一定要你按「記住」才會記。</p>
      {data && <Notice>{data.enabled ? "已開放：之後產生學習單與備課內容時，Manna 會參考有效的出題習慣；本次明確要求優先。" : "目前尚未開放讓習慣影響出題。可以先整理與管理，待主持人開啟後使用。"}</Notice>}
      {!data && !error && <Loading />}
      {data && <>
        {(data.pending?.length ?? 0) > 0 && <div className="memory-pending">
          <h3>等你回答的建議</h3>
          <MemorySuggestionCards items={data.pending ?? []} onDone={(id) => setData((d) => d && { ...d, pending: (d.pending ?? []).filter((c) => c.id !== id) })} onRemembered={() => void reload()} />
        </div>}
        <p>{active.length} / {max} 項有效習慣</p>
        {active.length === 0 && <p>尚未記住出題習慣。</p>}
        {active.length > 0 && <>
          <label className="field-label" htmlFor="memory-search">搜尋習慣</label>
          <input id="memory-search" type="search" value={search} placeholder="輸入關鍵字" onChange={(e) => setSearch(e.target.value)} />
          {needle && <p className="field-hint" role="status">找到 {matching.length} 項。</p>}
        </>}
        {groups.map((g) => <section key={g.name} className="memory-group" aria-label={`${g.name}（${g.items.length} 項）`}>
          <h3>{g.name}<small>（{g.items.length} 項）</small></h3>
          <ul className="memory-list">
            {g.items.map((m) => <li key={m.id}>
              <div><strong>{m.statement}</strong><small>確認於 {formatDateTime(m.valid_at)}</small></div>
              <button type="button" className="secondary-button" disabled={busy} onClick={() => {
                setText(`之後${m.statement}`); discard(); setMessage(null); field.current?.focus();
              }}>修改<span className="sr-only">：{m.statement}</span></button>
              <button type="button" className="text-link is-plain" disabled={busy} onClick={() => setRemoving(m)}>停止使用<span className="sr-only">：{m.statement}</span></button>
            </li>)}
          </ul>
        </section>)}
        <label className="field-label" htmlFor="memory-draft">想記住的長期習慣</label>
        <textarea id="memory-draft" ref={field} value={text} maxLength={1000} rows={3}
          disabled={busy} placeholder="例如：之後選擇題都用三個選項；或：每份學習單最後都附一題反思問題。"
          onChange={(e) => { setText(e.target.value); discard(); setMessage(null); }} />
        <button type="button" className="secondary-button" disabled={busy || !text.trim()} onClick={() => void preview()}>{busy ? "處理中…" : "整理成習慣（尚未記住）"}</button>
        {candidate && <div className="memory-preview" role="region" aria-label="確認出題習慣">
          <h3>確認要記住的內容</h3>
          <p>{candidate.statement}</p>
          {replacedText && <p>將取代：{replacedText} 舊的內容會保留在歷程。</p>}
          <button type="button" className="primary-button" disabled={busy} onClick={() => void confirm()}>確認並記住</button>
          <button type="button" className="text-link is-plain" disabled={busy} onClick={() => void cancel()}>取消</button>
        </div>}
        <details><summary>查看歷程（{history.length} 項）</summary>
          {history.length === 0 && <p>尚無已停止使用的版本。</p>}
          <ul className="memory-list">{history.map((m) => <li key={m.id}><div>
            <strong>{m.statement}</strong>
            <small>{formatDateTime(m.valid_at)} 至 {formatDateTime(m.invalid_at!)}，{m.superseded_by ? "已被新習慣取代" : "已停止使用"}</small>
            {m.superseded_by && <span>後續版本：{data.memories.find((next) => next.id === m.superseded_by)?.statement}</span>}
          </div></li>)}</ul>
        </details>
        {data.graph_enabled && data.index && <p className="field-hint">{data.index.error ?? (data.index.revision === data.index.indexed_revision ? "記憶索引已同步。" : "記憶索引等待同步，已確認的習慣仍會保存在系統中。")}</p>}
      </>}
      {message && <Notice tone="success">{message}</Notice>}
      <ErrorText message={error} />
      {removing && <Dialog title="停止使用這項習慣" busy={busy} onClose={() => setRemoving(null)}>
        <p>{removing.statement}</p><p>之後不再參考這項習慣；已產生的內容不變，歷程仍會保留。</p>
        <button type="button" className="primary-button" disabled={busy} onClick={() => void remove()}>確認停止使用</button>
        <button type="button" className="text-link is-plain" disabled={busy} onClick={() => setRemoving(null)}>取消</button>
      </Dialog>}
    </section>
  );
}
