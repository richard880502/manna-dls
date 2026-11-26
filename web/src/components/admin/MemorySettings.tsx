import { useEffect, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { MemorySettings as Settings, MemoryIndexStatus } from "../../api/types";
import { ErrorText, Notice } from "../common";

export function MemorySettings({ refreshVersion = 0 }: { refreshVersion?: number } = {}) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [index, setIndex] = useState<MemoryIndexStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let alive = true;
    Promise.all([api.memorySettings(), api.memoryIndex()]).then(([s, i]) => {
      if (alive) { setSettings(s); setIndex(i); }
    }).catch((e: unknown) => { if (alive) setError(errorMessage(e)); });
    return () => { alive = false; };
  }, [refreshVersion]);
  async function change(action: () => Promise<void>) {
    setBusy(true); setError(null); setMessage(null);
    try { await action(); } catch (e) { setError(errorMessage(e)); }
    finally { setBusy(false); }
  }
  return <section className="panel-card admin-settings" aria-labelledby="memory-settings-heading">
    <div className="card-heading">
      <h2 id="memory-settings-heading">教師長期記憶</h2>
      {settings && <span className="soft-tag">{settings.enabled ? "開啟" : "關閉"}</span>}
    </div>
    <p>預設關閉。主持人同意研究介入後再開啟；每次模型呼叫會記錄是否使用記憶與確切版本。</p>
    {settings && <label className="check-row">
      <input type="checkbox" checked={settings.enabled} disabled={busy} onChange={(e) => {
        const next = e.target.checked;
        void change(async () => { setSettings(await api.saveMemorySettings(next)); setMessage(next ? "已開放讓出題習慣影響產生結果。" : "已停止在產生時帶入出題習慣。"); });
      }} />讓已確認的出題習慣影響學習單與備課產生
    </label>}
    {index && <p>索引待同步：{index.pending}，同步失敗待重試：{index.failed}</p>}
    {settings?.graph_enabled && index?.providers_ready === false && <Notice>記憶索引需要可用的 OpenAI 相容文字模型與 embeddings，目前只使用 Postgres 的有效習慣；模型設定可用後會自動接續同步。</Notice>}
    {settings?.graph_enabled && index?.compatible === false && <Notice>記憶索引服務未就緒或介面版本不相容，目前只使用 Postgres 的有效習慣；服務恢復後會自動接續同步。</Notice>}
    {index?.sidecar && <p className="field-hint">記憶介面 v{index.sidecar.api_version}，Graphiti {index.sidecar.graphiti_version}</p>}
    {settings?.graph_enabled && <button className="secondary-button" type="button" disabled={busy} onClick={() => void change(async () => {
      await api.rebuildMemoryIndex(); setIndex(await api.memoryIndex()); setMessage("已排入重建，會從已確認的習慣重新建立 Graphiti 索引。");
    })}>從正本重建記憶索引</button>}
    {settings && !settings.graph_enabled && <p className="field-hint">尚未設定 Graphiti；確認過的習慣仍可由 Postgres 完整使用。</p>}
    {message && <Notice tone="success">{message}</Notice>}<ErrorText message={error} />
  </section>;
}
