import { useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { MemoryCandidate, MemoryState, TeacherMemory } from "../../api/types";
import { ErrorText, Loading, Notice } from "../common";
import { Dialog } from "../Dialog";
import { formatDateTime } from "../../lib/labels";

export function TeacherMemories() {
  const [data, setData] = useState<MemoryState | null>(null);
  const [text, setText] = useState("");
  const [candidate, setCandidate] = useState<MemoryCandidate | null>(null);
  const [removing, setRemoving] = useState<TeacherMemory | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
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

  const active = data?.memories.filter((m) => !m.invalid_at) ?? [];
  const history = data?.memories.filter((m) => m.invalid_at) ?? [];
  const replaced = data?.memories.find((m) => m.id === candidate?.replaces_id);
  return (
    <section className="panel-card teacher-memories" aria-labelledby="memories-heading">
      <h2 id="memories-heading">我的出題習慣</h2>
      <p>每次先整理一項，確認後才記住。可以修改或停止使用，每一步歷程都會保留。最多 10 項。</p>
      <p className="field-hint">支援選項數、配對組數、避免 Which of the following 開頭、說明與題目語言、參考答案、字彙提示、句長、難度及題號。請只寫教學形式，不填學生姓名或健康資訊。</p>
      {data && <Notice>{data.enabled ? "已開放：之後產生學習單與備課內容時，Manna 會參考有效的出題習慣；本次明確要求優先。" : "目前尚未開放讓習慣影響出題。可以先整理與管理，待主持人開啟後使用。"}</Notice>}
      {!data && !error && <Loading />}
      {data && <>
        <p>{active.length} / 10 項有效習慣</p>
        {active.length === 0 && <p>尚未記住出題習慣。</p>}
        <ul className="memory-list">
          {active.map((m) => <li key={m.id}>
            <div><strong>{m.statement}</strong><small>確認於 {formatDateTime(m.valid_at)}</small></div>
            <button type="button" className="secondary-button" disabled={busy} onClick={() => {
              setText(`之後${m.statement}`); discard(); setMessage(null); field.current?.focus();
            }}>修改<span className="sr-only">：{m.statement}</span></button>
            <button type="button" className="text-link is-plain" disabled={busy} onClick={() => setRemoving(m)}>停止使用<span className="sr-only">：{m.statement}</span></button>
          </li>)}
        </ul>
        <label className="field-label" htmlFor="memory-draft">想記住的長期習慣</label>
        <textarea id="memory-draft" ref={field} value={text} maxLength={1000} rows={3}
          disabled={busy} placeholder="例如：之後選擇題都用三個選項。"
          onChange={(e) => { setText(e.target.value); discard(); setMessage(null); }} />
        <button type="button" className="secondary-button" disabled={busy || !text.trim()} onClick={() => void preview()}>{busy ? "處理中…" : "整理成習慣（尚未記住）"}</button>
        {candidate && <div className="memory-preview" role="region" aria-label="確認出題習慣">
          <h3>確認要記住的內容</h3>
          <p>{candidate.statement}</p>
          {replaced && <p>將取代：{replaced.statement} 舊的內容會保留在歷程。</p>}
          <button type="button" className="primary-button" disabled={busy} onClick={() => void confirm()}>確認並記住</button>
          <button type="button" className="text-link is-plain" disabled={busy} onClick={() => void cancel()}>取消</button>
        </div>}
        <details><summary>查看歷程（{history.length} 項）</summary>
          {history.length === 0 && <p>尚無已停止使用的版本。</p>}
          <ul className="memory-list">{history.map((m) => <li key={m.id}><div>
            <strong>{m.statement}</strong>
            <small>{formatDateTime(m.valid_at)} 至 {formatDateTime(m.invalid_at!)}・{m.superseded_by ? "已被新習慣取代" : "已停止使用"}</small>
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
