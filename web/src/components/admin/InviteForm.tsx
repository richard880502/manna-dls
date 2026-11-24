import { useState, type FormEvent } from "react";
import { api, errorMessage } from "../../api/client";
import type { Role, SchoolTrack } from "../../api/types";
import { ROLE_LABELS, SCHOOL_TRACK_LABELS } from "../../lib/labels";
import { ErrorText, Notice } from "../common";
import { ROLES } from "./roles";

export function InviteForm() {
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
      <p className="field-hint">只填研究編號，不要填教師姓名或 Email。</p>
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
