import { useEffect, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { Account, Role } from "../../api/types";
import { ROLE_LABELS, SCHOOL_TRACK_LABELS, formatDate } from "../../lib/labels";
import { ROLES, roleChangeNote } from "./roles";
import { ErrorText, Loading, Notice } from "../common";

type RowResult = { id: number; tone: "success" | "error"; text: string };

export function AccountRoles() {
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
      <p className="field-hint">帳號在第一次用邀請碼登入後才會出現在這裡。你不能變更自己的角色。</p>
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
