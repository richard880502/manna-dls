import { useEffect, useState } from "react";
import { api, errorMessage } from "../../api/client";
import { useApp } from "../../context/AppContext";
import { ErrorText, Loading, Notice } from "../common";

export function PlatformSettings() {
  const { patchConfig } = useApp();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    api.admin
      .getSettings()
      .then((s) => alive && setEnabled(s.route_choice_enabled))
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, []);

  async function toggle(next: boolean) {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const saved = await api.admin.updateSettings({ route_choice_enabled: next });
      setEnabled(saved.route_choice_enabled);
      patchConfig({ route_choice_enabled: saved.route_choice_enabled });
      setMessage(
        saved.route_choice_enabled
          ? "已開放：教師可以自選四種備課菜單。"
          : "已關閉：所有單元改用固定的「目標優先」順序，偏好仍會調整預設展開的輔助。",
      );
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel-card admin-settings" aria-labelledby="settings-heading">
      <div className="card-heading">
        <h2 id="settings-heading">平台設定</h2>
        {enabled !== null && <span className="soft-tag">{enabled ? "自選備課菜單：開放" : "固定順序"}</span>}
      </div>
      {enabled === null && !error && <Loading />}
      {enabled !== null && (
        <label className="check-row admin-toggle">
          <input
            type="checkbox"
            checked={enabled}
            disabled={busy}
            onChange={(e) => void toggle(e.target.checked)}
          />
          允許教師自選備課菜單（目標／教材／學生／活動優先）
        </label>
      )}
      <p className="field-hint">關閉後，所有單元使用固定的「目標優先」順序，適合需要一致介入的研究；已建立單元的資料不受影響。</p>
      {message && <Notice tone="success">{message}</Notice>}
      <ErrorText message={error} />
    </section>
  );
}
