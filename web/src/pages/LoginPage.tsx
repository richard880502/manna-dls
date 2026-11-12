import { useState, type FormEvent } from "react";
import { Navigate, useNavigate } from "react-router";
import { errorMessage } from "../api/client";
import { DemoBanner, ErrorText } from "../components/common";
import { useApp } from "../context/AppContext";

export function LoginPage() {
  const { login, teacher, token, sessionNotice } = useApp();
  const navigate = useNavigate();
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (token && teacher) return <Navigate to="/" replace />;

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    const trimmed = code.trim();
    if (!trimmed) {
      setError("請輸入邀請碼。");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await login(trimmed);
      navigate("/", { replace: true });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <DemoBanner />
      <div className="login-shell">
        <section className="login-card panel-card" aria-labelledby="login-title">
          <div className="brand login-brand">
            <div className="brand-mark star-mascot" aria-hidden="true">
              ★
            </div>
            <div>
              <div className="brand-name">Manna DLS</div>
              <div className="brand-sub">AI 賦能英文教師專業學習</div>
            </div>
          </div>
          <h1 id="login-title">教師登入</h1>
          <p>請輸入研究團隊提供的邀請碼。平台只以研究編號識別你，不會顯示真實姓名。</p>
          {sessionNotice && <div className="notice notice-warn">{sessionNotice}</div>}
          <form onSubmit={onSubmit} noValidate>
            <label className="field-label" htmlFor="invite-code">
              邀請碼
            </label>
            <input
              id="invite-code"
              name="code"
              autoComplete="one-time-code"
              autoCapitalize="off"
              spellCheck={false}
              value={code}
              onChange={(e) => setCode(e.target.value)}
              disabled={busy}
            />
            <ErrorText message={error} />
            <button className="primary-button wide login-submit" type="submit" disabled={busy}>
              {busy ? "登入中…" : "登入"}
            </button>
          </form>
        </section>
      </div>
    </>
  );
}
