import { MemorySettings } from "../components/admin/MemorySettings";
import { AiModelSettings } from "../components/admin/AiModelSettings";
import { useEffect, useState, type ChangeEvent, type FormEvent } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../api/client";
import type { Account, ImportResult, Role, SchoolTrack } from "../api/types";
import { PromptsEditor } from "../components/admin/PromptsEditor";
import { ErrorText, Loading, Notice } from "../components/common";
import { useApp } from "../context/AppContext";
import { ROLE_LABELS, SCHOOL_TRACK_LABELS, formatDate } from "../lib/labels";
import { can } from "../lib/permissions";

const ROLES: Role[] = ["teacher", "pi", "kb_manager", "ra", "rater", "engineer"];

export function AdminPage() {
  const { teacher } = useApp();
  const [modelRevision, setModelRevision] = useState(0);
  if (!can(teacher, "administer")) {
    return <ErrorText message="這個頁面只開放給計畫主持人。" />;
  }
  return (
    <section className="admin-page">
      <div className="page-intro">
        <h1>後台管理</h1>
        <p>
          建立邀請碼、變更帳號的角色、維護提示詞版本、匯入教師備課經驗。每次修改提示詞都會產生新版本。課綱條目與詞彙表在
          <Link to="/knowledge?tab=curriculum">知識庫</Link>。
        </p>
      </div>
      <PlatformSettings />
      <AiModelSettings onChanged={() => setModelRevision((value) => value + 1)} />
      <MemorySettings refreshVersion={modelRevision} />
      <div className="admin-grid">
        <InviteForm />
        <CsvImport
          title="匯入教師備課經驗"
          columns="case_code,school_track,grade,topic,summary,details,approved"
          inputId="cases-csv"
          upload={api.admin.importTeacherCases}
        />
      </div>
      <AccountRoles />
      <PromptsEditor />
    </section>
  );
}

/** What a 知識庫管理員 may and may not do (docs/API.md「角色與權限」), said wherever the role is given. */
const KB_MANAGER_NOTE =
  "「知識庫管理員」可以管理知識庫的教材、學習單範本、課綱條目與詞彙，但不能建立邀請碼或變更平台設定，也看不到其他教師的資料。";

/** One line on what the new role means for the account, shown before the change is confirmed. */
function roleChangeNote(from: Role, to: Role): string {
  if (to === "pi") return "主持人可以使用全部後台功能，包括建立邀請碼與變更其他帳號的角色。";
  if (from === "pi") return "對方將不能再使用後台。";
  if (to === "kb_manager") return KB_MANAGER_NOTE;
  if (from === "kb_manager") return "對方將不能再管理知識庫。";
  return "對方的備課資料不受影響。";
}

type RowResult = { id: number; tone: "success" | "error"; text: string };

function AccountRoles() {
  const [accounts, setAccounts] = useState<Account[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** The change waiting for confirmation: the select already shows the new role. */
  const [pending, setPending] = useState<{ id: number; role: Role } | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<RowResult | null>(null);

  useEffect(() => {
    let alive = true;
    api.admin
      .listAccounts()
      .then((list) => alive && setAccounts(list))
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, []);

  function choose(account: Account, role: Role) {
    setResult(null);
    setPending(role === account.role ? null : { id: account.id, role });
  }

  async function confirm(account: Account, role: Role) {
    setBusy(true);
    try {
      const saved = await api.admin.updateAccountRole(account.id, role);
      setAccounts((prev) => (prev ?? []).map((a) => (a.id === saved.id ? saved : a)));
      setResult({
        id: saved.id,
        tone: "success",
        text: `已將 ${saved.research_id} 的角色改為「${ROLE_LABELS[saved.role]}」。對方不必重新登入，重新整理網頁後畫面就會更新。`,
      });
    } catch (err) {
      setResult({ id: account.id, tone: "error", text: errorMessage(err) });
    } finally {
      setPending(null);
      setBusy(false);
    }
  }

  return (
    <section className="panel-card admin-accounts" aria-labelledby="accounts-heading">
      <div className="card-heading">
        <h2 id="accounts-heading">帳號與角色</h2>
        {accounts && <span className="soft-tag">{accounts.length} 個帳號</span>}
      </div>
      <p className="field-hint">{KB_MANAGER_NOTE}</p>
      <p className="field-hint">
        帳號在第一次用邀請碼登入後才會出現在這裡。角色變更立即生效，對方不必重新登入。你不能變更自己的角色。
      </p>
      <ErrorText message={error} />
      {!accounts && !error && <Loading />}
      {accounts && accounts.length > 0 && (
        <ul className="rows is-compact" aria-label="帳號">
          {accounts.map((account) => {
            const chosen = pending?.id === account.id ? pending.role : account.role;
            const rowResult = result?.id === account.id ? result : null;
            return (
              <li key={account.id} className="row">
                <div>
                  <h3>
                    {account.research_id}
                    {account.is_self && <span className="soft-tag admin-self-tag">你</span>}
                  </h3>
                  <p className="meta">
                    {SCHOOL_TRACK_LABELS[account.school_track] ?? account.school_track} ·{" "}
                    {formatDate(account.created_at)} 加入
                  </p>
                </div>
                <div className="side">
                  {account.is_self ? (
                    <span>{ROLE_LABELS[account.role] ?? account.role}</span>
                  ) : (
                    <select
                      className="admin-role-select"
                      aria-label={`${account.research_id} 的角色`}
                      value={chosen}
                      disabled={busy}
                      onChange={(e) => choose(account, e.target.value as Role)}
                    >
                      {ROLES.map((r) => (
                        <option key={r} value={r}>
                          {ROLE_LABELS[r]}
                        </option>
                      ))}
                    </select>
                  )}
                </div>
                {pending?.id === account.id && (
                  <div
                    className="row-more admin-role-confirm"
                    role="alertdialog"
                    aria-label={`確認變更 ${account.research_id} 的角色`}
                  >
                    <p>
                      <strong>
                        把 {account.research_id} 的角色從「{ROLE_LABELS[account.role]}」改為「
                        {ROLE_LABELS[pending.role]}」？
                      </strong>
                    </p>
                    <p>{roleChangeNote(account.role, pending.role)}</p>
                    <div className="button-row">
                      <button
                        type="button"
                        className="primary-button"
                        disabled={busy}
                        onClick={() => void confirm(account, pending.role)}
                      >
                        {busy ? "變更中…" : "確定變更"}
                      </button>
                      <button
                        type="button"
                        className="text-link is-plain"
                        disabled={busy}
                        onClick={() => setPending(null)}
                      >
                        取消
                      </button>
                    </div>
                  </div>
                )}
                {rowResult && (
                  <div className="row-more">
                    {rowResult.tone === "success" ? (
                      <Notice tone="success">{rowResult.text}</Notice>
                    ) : (
                      <ErrorText message={rowResult.text} />
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

function PlatformSettings() {
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
      <p className="field-hint">
        關閉後，教師端不顯示菜單選擇，所有單元使用固定的「目標優先」順序（研究設計需要一致介入時使用）；已建立單元的資料不受影響。
      </p>
      {message && <Notice tone="success">{message}</Notice>}
      <ErrorText message={error} />
    </section>
  );
}

function InviteForm() {
  const [researchId, setResearchId] = useState("");
  const [track, setTrack] = useState<SchoolTrack>("general");
  const [role, setRole] = useState<Role>("teacher");
  const [code, setCode] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    if (!researchId.trim()) {
      setError("請輸入研究編號。");
      return;
    }
    setBusy(true);
    setError(null);
    setCode(null);
    try {
      const res = await api.admin.createInvite({ research_id: researchId.trim(), school_track: track, role });
      setCode(res.code);
      setResearchId("");
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="panel-card" onSubmit={onSubmit} aria-labelledby="invite-heading">
      <h2 id="invite-heading">建立邀請碼</h2>
      <label className="field-label" htmlFor="invite-research-id">
        研究編號
      </label>
      <input
        id="invite-research-id"
        value={researchId}
        onChange={(e) => setResearchId(e.target.value)}
        placeholder="例如：T-2026-001"
      />
      <p className="field-hint">只填研究編號，不要填教師姓名或 Email。{KB_MANAGER_NOTE}</p>
      <div className="two-fields">
        <div>
          <label className="field-label" htmlFor="invite-track">
            學校類型
          </label>
          <select id="invite-track" value={track} onChange={(e) => setTrack(e.target.value as SchoolTrack)}>
            {(Object.keys(SCHOOL_TRACK_LABELS) as SchoolTrack[]).map((t) => (
              <option key={t} value={t}>
                {SCHOOL_TRACK_LABELS[t]}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="field-label" htmlFor="invite-role">
            角色
          </label>
          <select id="invite-role" value={role} onChange={(e) => setRole(e.target.value as Role)}>
            {ROLES.map((r) => (
              <option key={r} value={r}>
                {ROLE_LABELS[r]}
              </option>
            ))}
          </select>
        </div>
      </div>
      <div className="button-row admin-actions">
        <button className="primary-button" type="submit" disabled={busy}>
          {busy ? "建立中…" : "建立邀請碼"}
        </button>
      </div>
      {code && (
        <Notice tone="success">
          邀請碼：<code className="invite-code">{code}</code>
          <br />
          明碼只會顯示這一次，請現在複製並交給教師。
        </Notice>
      )}
      <ErrorText message={error} />
    </form>
  );
}

function CsvImport({
  title,
  columns,
  inputId,
  upload,
}: {
  title: string;
  columns: string;
  inputId: string;
  upload: (csv: string) => Promise<ImportResult>;
}) {
  const [file, setFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ImportResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  function onFile(e: ChangeEvent<HTMLInputElement>) {
    setFile(e.target.files?.[0] ?? null);
    setResult(null);
    setError(null);
  }

  async function onUpload() {
    if (!file) {
      setError("請先選擇 CSV 檔案。");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const text = await file.text();
      setResult(await upload(text));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel-card" aria-labelledby={`${inputId}-heading`}>
      <h2 id={`${inputId}-heading`}>{title}</h2>
      <p className="field-hint">
        CSV（UTF-8），欄位：<code>{columns}</code>
      </p>
      <label className="field-label" htmlFor={inputId}>
        選擇 CSV 檔案
      </label>
      <input id={inputId} type="file" accept=".csv,text/csv" onChange={onFile} />
      <div className="button-row admin-actions">
        <button type="button" className="secondary-button" onClick={() => void onUpload()} disabled={busy}>
          {busy ? "匯入中…" : "匯入"}
        </button>
      </div>
      {result && (
        <Notice tone="success">
          已匯入 {result.imported} 筆。來源 SHA-256：<code className="sha">{result.sha256}</code>
        </Notice>
      )}
      <ErrorText message={error} />
    </section>
  );
}
