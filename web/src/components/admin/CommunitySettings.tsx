import { useEffect, useState } from "react";
import { api, errorMessage } from "../../api/client";
import { useApp } from "../../context/AppContext";
import { ErrorText, Notice } from "../common";

/** 社群 (the research switch, default off): teachers may share worksheets with each other. */
export function CommunitySettings() {
  const { patchConfig } = useApp();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .communitySettings()
      .then((s) => alive && setEnabled(s.enabled))
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, []);

  async function change(next: boolean) {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const saved = await api.saveCommunitySettings(next);
      setEnabled(saved.enabled);
      patchConfig({ community_enabled: saved.enabled });
      setMessage(
        saved.enabled
          ? "已開放社群：老師的側邊欄會出現「社群」，可以自己決定要不要分享學習單。"
          : "已關閉社群：側邊欄的「社群」和所有分享都看不到了。老師分享的設定會保留，再開啟就會回來。",
      );
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel-card admin-settings" aria-labelledby="community-settings-heading">
      <div className="card-heading">
        <h2 id="community-settings-heading">社群</h2>
        {enabled !== null && <span className="soft-tag">{enabled ? "開啟" : "關閉"}</span>}
      </div>
      <p>
        預設關閉。開啟後老師可以自己選擇分享學習單，其他老師可以瀏覽、收藏，或拿來改成自己的副本。分享的人只顯示老師自己選的研究代號或「一位老師」；每次分享、取消、瀏覽、收藏和改編都會記錄（只記編號，不記內容）。
      </p>
      {enabled !== null && (
        <label className="check-row">
          <input type="checkbox" checked={enabled} disabled={busy} onChange={(e) => void change(e.target.checked)} />
          開放老師在社群分享、瀏覽和改編學習單
        </label>
      )}
      {message && <Notice tone="success">{message}</Notice>}
      <ErrorText message={error} />
    </section>
  );
}
