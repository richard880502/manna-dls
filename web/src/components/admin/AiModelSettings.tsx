import { useEffect, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { AiModelProvider, AiModelSettings as Settings } from "../../api/types";
import { formatDateTime } from "../../lib/labels";
import { ErrorText, Loading, Notice } from "../common";

const OTHER = "__other__";

/** The form's starting point: what is in use now, when that is something the PI may pick. */
function initialForm(s: Settings) {
  const current = s.providers.find((p) => p.id === s.active.provider && p.available);
  const provider = current ?? s.providers.find((p) => p.available) ?? s.providers[0];
  const model = current ? s.active.model : (provider?.models[0] ?? "");
  const known = provider?.models.includes(model) ?? false;
  return {
    provider: provider?.id ?? "",
    choice: known ? model : OTHER,
    custom: known ? "" : model,
    effort: s.efforts.includes(s.active.effort) ? s.active.effort : "medium",
    timeout: String(s.active.timeout_secs),
  };
}

function describe(s: Settings, value: Settings["active"]): string {
  const label = s.providers.find((p) => p.id === value.provider)?.label ?? value.provider;
  const uses = s.providers.find((p) => p.id === value.provider)?.uses_effort;
  return `${label}・${value.model}${uses ? `・推理強度 ${value.effort}` : ""}・等待上限 ${value.timeout_secs} 秒`;
}

/** 後台管理 → 平台設定 → 文字模型（docs/API.md「文字模型設定」）。 */
export function AiModelSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [form, setForm] = useState<ReturnType<typeof initialForm> | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmReset, setConfirmReset] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api.admin
      .getAiModel()
      .then((s) => {
        if (!alive) return;
        setSettings(s);
        setForm(initialForm(s));
      })
      .catch((e: unknown) => alive && setError(errorMessage(e)));
    return () => {
      alive = false;
    };
  }, []);

  if (!settings || !form) {
    return (
      <section className="panel-card admin-settings" aria-labelledby="ai-model-heading">
        <h2 id="ai-model-heading">文字模型</h2>
        {error ? <ErrorText message={error} /> : <Loading />}
      </section>
    );
  }

  const provider: AiModelProvider | undefined = settings.providers.find((p) => p.id === form.provider);
  const model = (form.choice === OTHER ? form.custom : form.choice).trim();
  const timeout = Number(form.timeout);
  const { min, max } = settings.timeout_range;
  const timeoutOk = Number.isInteger(timeout) && timeout >= min && timeout <= max;
  const canSubmit = !busy && !!provider?.available && model !== "" && timeoutOk;

  function update(change: Partial<NonNullable<typeof form>>) {
    setForm((f) => (f ? { ...f, ...change } : f));
  }

  function pickProvider(id: string) {
    const next = settings?.providers.find((p) => p.id === id);
    update({ provider: id, choice: next?.models[0] ?? OTHER, custom: "" });
  }

  async function apply() {
    if (!canSubmit || !provider) return;
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const saved = await api.admin.saveAiModel({
        provider: provider.id,
        model,
        ...(provider.uses_effort ? { effort: form?.effort } : {}),
        timeout_secs: timeout,
      });
      setSettings(saved);
      setForm(initialForm(saved));
      const seconds = ((saved.test_latency_ms ?? 0) / 1000).toFixed(1);
      setMessage(`測試成功（約 ${seconds} 秒），已套用。新的 AI 要求會用這個模型。`);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function reset() {
    setBusy(true);
    setError(null);
    setMessage(null);
    setConfirmReset(false);
    try {
      const saved = await api.admin.resetAiModel();
      setSettings(saved);
      setForm(initialForm(saved));
      setMessage("已恢復成環境變數的預設模型。");
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const fromAdmin = settings.source === "admin";
  return (
    <section className="panel-card admin-settings" aria-labelledby="ai-model-heading">
      <div className="card-heading">
        <h2 id="ai-model-heading">文字模型</h2>
        <span className="soft-tag">{fromAdmin ? "後台設定" : "環境變數預設"}</span>
      </div>
      <p>
        目前使用：<strong>{describe(settings, settings.active)}</strong>
      </p>
      <p className="field-hint">
        {fromAdmin
          ? `由 ${settings.changed_by ?? "主持人"} 於 ${formatDateTime(settings.changed_at)} 設定；預設是 ${describe(settings, settings.env_default)}。`
          : "目前用的是部署時設定的預設值。"}
      </p>
      <p className="field-hint">套用後，新的 AI 要求立刻使用新模型；已經送出的要求會用原本的模型完成（多步驟的工作，後面的步驟會改用新模型）。</p>

      <div className="two-fields">
        <div>
          <label className="field-label" htmlFor="ai-model-provider">
            模型來源
          </label>
          <select
            id="ai-model-provider"
            value={form.provider}
            disabled={busy}
            onChange={(e) => pickProvider(e.target.value)}
          >
            {settings.providers.map((p) => (
              <option key={p.id} value={p.id} disabled={!p.available}>
                {p.label}
                {p.available ? "" : "（尚未設定金鑰）"}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="field-label" htmlFor="ai-model-name">
            模型
          </label>
          <select
            id="ai-model-name"
            value={form.choice}
            disabled={busy}
            onChange={(e) => update({ choice: e.target.value })}
          >
            {(provider?.models ?? []).map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
            <option value={OTHER}>其他（自行輸入）</option>
          </select>
        </div>
      </div>
      {form.choice === OTHER && (
        <>
          <label className="field-label" htmlFor="ai-model-custom">
            模型名稱
          </label>
          <input
            id="ai-model-custom"
            value={form.custom}
            disabled={busy}
            onChange={(e) => update({ custom: e.target.value })}
            placeholder="照服務商提供的名稱輸入，例如 gpt-5.6-sol"
          />
        </>
      )}
      <div className="two-fields">
        {provider?.uses_effort && (
          <div>
            <label className="field-label" htmlFor="ai-model-effort">
              推理強度
            </label>
            <select
              id="ai-model-effort"
              value={form.effort}
              disabled={busy}
              onChange={(e) => update({ effort: e.target.value })}
            >
              {settings.efforts.map((x) => (
                <option key={x} value={x}>
                  {x}
                </option>
              ))}
            </select>
          </div>
        )}
        <div>
          <label className="field-label" htmlFor="ai-model-timeout">
            等待上限（秒）
          </label>
          <input
            id="ai-model-timeout"
            type="number"
            min={min}
            max={max}
            value={form.timeout}
            disabled={busy}
            onChange={(e) => update({ timeout: e.target.value })}
          />
        </div>
      </div>
      <p className="field-hint">
        等待上限可設 {min}–{max} 秒。按下按鈕會先用新設定問模型一個小問題，成功才會儲存；沒通過就維持原本的模型。
        {provider?.id === "openai" && " OpenAI 是海外服務，只在研究計畫同意的範圍內使用。"}
      </p>

      <div className="button-row admin-actions">
        <button className="primary-button" type="button" disabled={!canSubmit} onClick={() => void apply()}>
          {busy ? "測試中，請稍候…" : "測試並套用"}
        </button>
        {fromAdmin && !confirmReset && (
          <button className="secondary-button" type="button" disabled={busy} onClick={() => setConfirmReset(true)}>
            恢復環境變數預設
          </button>
        )}
        {fromAdmin && confirmReset && (
          <>
            <span>恢復成 {describe(settings, settings.env_default)}？</span>
            <button className="secondary-button" type="button" disabled={busy} onClick={() => void reset()}>
              確定恢復
            </button>
            <button className="text-link is-plain" type="button" onClick={() => setConfirmReset(false)}>
              取消
            </button>
          </>
        )}
      </div>
      {message && <Notice tone="success">{message}</Notice>}
      <ErrorText message={error} />
    </section>
  );
}
