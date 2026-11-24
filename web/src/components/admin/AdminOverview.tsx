import { useEffect, useState, type ReactNode } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../../api/client";
import type { AiModelSettings, MemoryIndexStatus, MemorySettings } from "../../api/types";
import { Loading } from "../common";

type Load<T> = { data: T | null; error: string | null };

/** Loads one thing for one row, so a slow or failing request never blocks the other rows. */
function useLoad<T>(fetcher: () => Promise<T>): Load<T> {
  const [state, setState] = useState<Load<T>>({ data: null, error: null });
  useEffect(() => {
    let alive = true;
    fetcher()
      .then((data) => alive && setState({ data, error: null }))
      .catch((err: unknown) => alive && setState({ data: null, error: errorMessage(err) }));
    return () => {
      alive = false;
    };
    // The fetcher is a fresh closure each render; the rows load once when the tab opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return state;
}

function modelText(s: AiModelSettings): string {
  const provider = s.providers.find((p) => p.id === s.active.provider);
  const effort = provider?.uses_effort ? `・推理強度 ${s.active.effort}` : "";
  return `${provider?.label ?? s.active.provider}・${s.active.model}${effort}`;
}

function memoryText(s: MemorySettings, i: MemoryIndexStatus): string {
  const parts = [s.enabled ? "開啟" : "關閉"];
  if (i.pending > 0) parts.push(`索引待同步 ${i.pending}`);
  if (i.failed > 0) parts.push(`同步失敗 ${i.failed}`);
  return parts.join("・");
}

function Row({ label, to, linkLabel, state, children }: {
  label: string;
  to: string;
  linkLabel: string;
  state: Load<unknown>;
  children: ReactNode;
}) {
  return (
    <li className="row">
      <div>
        <h3>{label}</h3>
        {state.error ? (
          <p className="meta" role="alert">讀取失敗：{state.error}</p>
        ) : state.data === null ? (
          <Loading />
        ) : (
          <p className="meta">{children}</p>
        )}
      </div>
      <div className="side">
        <Link className="text-link" to={to} aria-label={`${linkLabel}（${label}）`}>
          {linkLabel}
        </Link>
      </div>
    </li>
  );
}

/** 後台管理 → 總覽: what is set right now, read-only, each row linking to the tab that changes it. */
export function AdminOverview() {
  const model = useLoad(() => api.admin.getAiModel());
  const memory = useLoad(() => Promise.all([api.memorySettings(), api.memoryIndex()]));
  const settings = useLoad(() => api.admin.getSettings());
  const accounts = useLoad(() => api.admin.listAccounts());
  const prompts = useLoad(() => api.admin.listPrompts());
  return (
    <section className="panel-card" aria-labelledby="overview-heading">
      <h2 id="overview-heading">目前設定</h2>
      <ul className="rows is-compact admin-overview" aria-label="目前設定">
        <Row label="文字模型" to="/admin?tab=settings" linkLabel="調整" state={model}>
          {model.data && modelText(model.data)}
        </Row>
        <Row label="教師長期記憶" to="/admin?tab=settings" linkLabel="調整" state={memory}>
          {memory.data && memoryText(memory.data[0], memory.data[1])}
        </Row>
        <Row label="自選備課菜單" to="/admin?tab=settings" linkLabel="調整" state={settings}>
          {settings.data && (settings.data.route_choice_enabled ? "開放" : "固定順序")}
        </Row>
        <Row label="帳號" to="/admin?tab=accounts" linkLabel="管理" state={accounts}>
          {accounts.data && `${accounts.data.length} 個帳號`}
        </Row>
        <Row label="提示詞" to="/admin?tab=prompts" linkLabel="編輯" state={prompts}>
          {prompts.data && `${prompts.data.prompts.length} 個提示詞`}
        </Row>
      </ul>
    </section>
  );
}
