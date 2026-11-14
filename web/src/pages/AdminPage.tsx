import { useEffect, useState, type ChangeEvent, type FormEvent } from "react";
import { api, errorMessage } from "../api/client";
import type { ImportResult, PromptVersion, Role, SchoolTrack } from "../api/types";
import { ErrorText, Loading, Notice } from "../components/common";
import { useApp } from "../context/AppContext";
import { ROLE_LABELS, SCHOOL_TRACK_LABELS, formatDateTime } from "../lib/labels";

const ROLES: Role[] = ["teacher", "pi", "ra", "rater", "engineer"];

export function AdminPage() {
  const { teacher } = useApp();
  if (teacher?.role !== "pi") {
    return <ErrorText message="這個頁面只開放給計畫主持人。" />;
  }
  return (
    <section className="admin-page">
      <div className="page-intro">
        <h1>後台管理</h1>
        <p>建立邀請碼、維護提示詞版本、匯入知識庫與教師備課經驗。每次修改提示詞都會產生新版本。</p>
      </div>
      <PlatformSettings />
      <div className="admin-grid">
        <InviteForm />
        <CsvImport
          title="匯入知識庫條目"
          columns="item_code,category,school_track,title,content,source_doc,source_version,source_page"
          inputId="knowledge-csv"
          upload={api.admin.importKnowledge}
        />
        <CsvImport
          title="匯入教師備課經驗"
          columns="case_code,school_track,grade,topic,summary,details,approved"
          inputId="cases-csv"
          upload={api.admin.importTeacherCases}
        />
      </div>
      <PromptsEditor />
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

function PromptsEditor() {
  const [prompts, setPrompts] = useState<PromptVersion[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [newKey, setNewKey] = useState("");

  useEffect(() => {
    let alive = true;
    api.admin
      .listPrompts()
      .then((list) => alive && setPrompts(list))
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, []);

  function onSaved(saved: PromptVersion) {
    setPrompts((prev) => {
      const others = (prev ?? []).filter((p) => p.prompt_key !== saved.prompt_key);
      return [...others, saved].sort((a, b) => a.prompt_key.localeCompare(b.prompt_key));
    });
  }

  return (
    <section className="panel-card prompts-panel" aria-labelledby="prompts-heading">
      <div className="card-heading">
        <h2 id="prompts-heading">提示詞版本</h2>
        <span className="soft-tag">每個 key 的最新版</span>
      </div>
      <ErrorText message={error} />
      {!prompts && !error && <Loading />}
      {prompts?.map((p) => (
        <PromptCard key={`${p.prompt_key}-${p.version_no}`} prompt={p} onSaved={onSaved} />
      ))}
      <div className="new-prompt">
        <label className="field-label" htmlFor="new-prompt-key">
          新增提示詞 key
        </label>
        <div className="inline-form">
          <input
            id="new-prompt-key"
            value={newKey}
            onChange={(e) => setNewKey(e.target.value)}
            placeholder="例如：core_assist"
          />
        </div>
        {newKey.trim() && (
          <PromptCard
            key={`new-${newKey.trim()}`}
            prompt={{
              id: 0,
              prompt_key: newKey.trim(),
              version_no: 0,
              system_text: "",
              user_template: "",
              created_at: "",
            }}
            onSaved={(p) => {
              onSaved(p);
              setNewKey("");
            }}
          />
        )}
      </div>
    </section>
  );
}

function PromptCard({ prompt, onSaved }: { prompt: PromptVersion; onSaved: (p: PromptVersion) => void }) {
  const [systemText, setSystemText] = useState(prompt.system_text);
  const [userTemplate, setUserTemplate] = useState(prompt.user_template);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const changed = systemText !== prompt.system_text || userTemplate !== prompt.user_template;
  const idBase = `prompt-${prompt.prompt_key}`;

  async function save() {
    setBusy(true);
    setError(null);
    try {
      const saved = await api.admin.createPrompt({
        prompt_key: prompt.prompt_key,
        system_text: systemText,
        user_template: userTemplate,
      });
      onSaved(saved);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <article className="prompt-card-admin">
      <div className="card-heading">
        <strong>{prompt.prompt_key}</strong>
        <span className="soft-tag">
          {prompt.version_no > 0 ? `v${prompt.version_no} · ${formatDateTime(prompt.created_at)}` : "新 key"}
        </span>
      </div>
      <label className="field-label" htmlFor={`${idBase}-system`}>
        system_text
      </label>
      <textarea id={`${idBase}-system`} rows={5} value={systemText} onChange={(e) => setSystemText(e.target.value)} />
      <label className="field-label" htmlFor={`${idBase}-user`}>
        user_template
      </label>
      <textarea id={`${idBase}-user`} rows={5} value={userTemplate} onChange={(e) => setUserTemplate(e.target.value)} />
      <div className="button-row admin-actions">
        <button type="button" className="primary-button" onClick={() => void save()} disabled={busy || !changed}>
          {busy ? "儲存中…" : "儲存為新版本"}
        </button>
      </div>
      <ErrorText message={error} />
    </article>
  );
}
