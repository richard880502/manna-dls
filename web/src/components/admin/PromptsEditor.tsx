import { useEffect, useId, useMemo, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { PromptInfo, PromptList, PromptVersion, TryResponse } from "../../api/types";
import { formatDateTime } from "../../lib/labels";
import { checkVariables, lineDiff } from "../../lib/promptCheck";
import { ErrorText, Loading, Notice } from "../common";
import { Icon } from "../Icon";

const UNREGISTERED = "未登記";
const NOTE_MAX = 200;
/** The old unit-and-lesson flow: still editable, but not what the PI reaches for first. */
const LEGACY_GROUP = "單元與課例（舊流程）";

function matches(info: PromptInfo | undefined, p: PromptVersion, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const hay = [p.prompt_key, info?.name_zh, info?.description, info?.group].join("\n").toLowerCase();
  return q.split(/\s+/).every((word) => hay.includes(word));
}

export function PromptsEditor() {
  const idBase = useId();
  const [list, setList] = useState<PromptList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [newKey, setNewKey] = useState("");
  const [closed, setClosed] = useState<ReadonlySet<string>>(() => new Set([LEGACY_GROUP]));

  useEffect(() => {
    let alive = true;
    api.admin
      .listPrompts()
      .then((l) => alive && setList(l))
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, []);

  function onSaved(saved: PromptVersion) {
    setList((prev) =>
      prev
        ? {
            ...prev,
            prompts: [...prev.prompts.filter((p) => p.prompt_key !== saved.prompt_key), saved].sort((a, b) =>
              a.prompt_key.localeCompare(b.prompt_key),
            ),
          }
        : prev,
    );
  }

  const byKey = useMemo(() => new Map((list?.registry ?? []).map((i) => [i.key, i])), [list]);
  const sections = useMemo(() => {
    if (!list) return [];
    const order = [...list.groups, UNREGISTERED];
    return order
      .map((group) => ({
        group,
        items: list.prompts
          .filter((p) => (byKey.get(p.prompt_key)?.group ?? UNREGISTERED) === group)
          .filter((p) => matches(byKey.get(p.prompt_key), p, query))
          .sort((a, b) => {
            const names = (p: PromptVersion) => byKey.get(p.prompt_key)?.name_zh ?? p.prompt_key;
            return list.registry.findIndex((i) => i.key === a.prompt_key) -
              list.registry.findIndex((i) => i.key === b.prompt_key) || names(a).localeCompare(names(b));
          }),
      }))
      .filter((s) => s.items.length > 0);
  }, [list, byKey, query]);
  const shown = sections.reduce((n, s) => n + s.items.length, 0);
  const searching = query.trim() !== "";

  function setGroupOpen(group: string, open: boolean) {
    setClosed((prev) => {
      if (prev.has(group) === !open) return prev;
      const next = new Set(prev);
      if (open) next.delete(group);
      else next.add(group);
      return next;
    });
  }

  function jumpTo(group: string, index: number) {
    setGroupOpen(group, true);
    requestAnimationFrame(() => document.getElementById(`${idBase}-group-${index}`)?.scrollIntoView?.({ block: "start" }));
  }

  return (
    <section className="panel-card prompts-panel" aria-labelledby="prompts-heading">
      <div className="card-heading">
        <h2 id="prompts-heading">提示詞版本</h2>
        <span className="soft-tag">{list ? `${list.prompts.length} 個提示詞 · 每個顯示最新版` : "每個提示詞的最新版"}</span>
      </div>
      <p className="field-hint">
        按下提示詞展開來編輯。每次儲存都會新增一個版本，舊版本都留著，可以查看、比較或還原。
      </p>
      <ErrorText message={error} />
      {!list && !error && <Loading />}
      {list && (
        <>
          <div className="prompt-toolbar">
            <div className="prompt-search">
              <label className="field-label" htmlFor="prompt-search">
                搜尋提示詞
              </label>
              <input
                id="prompt-search"
                type="search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="名稱、說明或 key，例如：暖身"
              />
              {searching && <span className="soft-tag">找到 {shown} 個</span>}
            </div>
            {sections.length > 0 && (
              <ul className="prompt-chips" aria-label="跳到分組">
                {sections.map((s, i) => (
                  <li key={s.group}>
                    <button type="button" className="prompt-chip" onClick={() => jumpTo(s.group, i)}>
                      {s.group}
                      <span className="prompt-chip-count">{s.items.length}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
          {sections.map((s, i) => (
            <section key={s.group} id={`${idBase}-group-${i}`} className="prompt-group" aria-label={s.group}>
              <details
                className="prompt-group-body"
                open={searching || !closed.has(s.group)}
                onToggle={(e) => {
                  if (!searching) setGroupOpen(s.group, (e.currentTarget as HTMLDetailsElement).open);
                }}
              >
              <summary>
                <h3>
                  {s.group}
                  <span className="soft-tag">{s.items.length}</span>
                </h3>
              </summary>
              {s.group === UNREGISTERED && (
                <p className="field-hint">這些 key 還沒有登記名稱與變數，程式也可能已經不用了；儲存時不會檢查變數。</p>
              )}
              {s.items.map((p) => (
                <PromptItem
                  key={p.prompt_key}
                  prompt={p}
                  info={byKey.get(p.prompt_key)}
                  canTry={list.try_keys.includes(p.prompt_key)}
                  open={searching && shown <= 3}
                  onSaved={onSaved}
                />
              ))}
              </details>
            </section>
          ))}
          {shown === 0 && <p className="field-hint">沒有符合的提示詞。</p>}
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
                info={byKey.get(newKey.trim())}
                onSaved={(p) => {
                  onSaved(p);
                  setNewKey("");
                }}
              />
            )}
          </div>
        </>
      )}
    </section>
  );
}

/** One prompt, collapsed to its name; the editor mounts only when it is opened. */
function PromptItem({
  prompt,
  info,
  canTry,
  open: openByDefault,
  onSaved,
}: {
  prompt: PromptVersion;
  info: PromptInfo | undefined;
  canTry: boolean;
  open: boolean;
  onSaved: (p: PromptVersion) => void;
}) {
  const [open, setOpen] = useState(openByDefault);
  return (
    <details
      className="prompt-item"
      open={open}
      onToggle={(e) => setOpen((e.currentTarget as HTMLDetailsElement).open)}
    >
      <summary>
        <span className="prompt-item-name">{info?.name_zh ?? "（未登記的提示詞）"}</span>
        <code className="prompt-item-key">{prompt.prompt_key}</code>
        <span className="soft-tag">v{prompt.version_no}</span>
        {info && <span className="prompt-item-desc">{info.description}</span>}
      </summary>
      {open && <PromptCard prompt={prompt} info={info} canTry={canTry} onSaved={onSaved} />}
    </details>
  );
}

function VariableList({ info }: { info: PromptInfo }) {
  if (info.vars.length === 0) {
    return <p className="field-hint">這個提示詞沒有變數：程式直接使用它的文字。</p>;
  }
  return (
    <details className="prompt-vars">
      <summary>程式會填入的變數（{info.vars.length}）</summary>
      <ul>
        {info.vars.map((v) => (
          <li key={v.name}>
            <code>{`{{${v.name}}}`}</code> {v.label}
            {v.required && <span className="var-tag var-required">必要</span>}
            {v.auto && <span className="var-tag var-auto">沒寫也會自動補上</span>}
          </li>
        ))}
      </ul>
    </details>
  );
}

function PromptCard({
  prompt,
  info,
  canTry = false,
  onSaved,
}: {
  prompt: PromptVersion;
  info: PromptInfo | undefined;
  canTry?: boolean;
  onSaved: (p: PromptVersion) => void;
}) {
  const [systemText, setSystemText] = useState(prompt.system_text);
  const [userTemplate, setUserTemplate] = useState(prompt.user_template);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  // A new version (saved here, or restored from the history) becomes what the editor shows.
  const [shownVersion, setShownVersion] = useState(prompt.version_no);
  if (shownVersion !== prompt.version_no) {
    setShownVersion(prompt.version_no);
    setSystemText(prompt.system_text);
    setUserTemplate(prompt.user_template);
  }
  const changed = systemText !== prompt.system_text || userTemplate !== prompt.user_template;
  const idBase = `prompt-${prompt.prompt_key}`;
  const check = checkVariables(info, systemText, userTemplate);
  const userOptional = info ? !info.uses_user_template : false;
  const blank = !systemText.trim() || (!userOptional && !userTemplate.trim());

  async function save() {
    setBusy(true);
    setError(null);
    setSaved(null);
    try {
      const result = await api.admin.createPrompt({
        prompt_key: prompt.prompt_key,
        system_text: systemText,
        user_template: userTemplate,
        ...(note.trim() ? { note: note.trim() } : {}),
      });
      setSaved(`已儲存為 v${result.version_no}，之後的 AI 呼叫會立刻使用這一版。`);
      setNote("");
      onSaved(result);
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
      {info ? (
        <>
          <p className="prompt-description">{info.description}</p>
          <VariableList info={info} />
        </>
      ) : (
        <p className="field-hint">這個 key 還沒有登記，儲存時不會檢查變數。</p>
      )}
      <label className="field-label" htmlFor={`${idBase}-system`}>
        系統提示詞（system_text）
      </label>
      <textarea id={`${idBase}-system`} rows={8} value={systemText} onChange={(e) => setSystemText(e.target.value)} />
      <label className="field-label" htmlFor={`${idBase}-user`}>
        使用者範本（user_template）{userOptional && "：這個提示詞不使用，可以留空"}
      </label>
      <textarea id={`${idBase}-user`} rows={6} value={userTemplate} onChange={(e) => setUserTemplate(e.target.value)} />
      {check.missing.length > 0 && (
        <div className="prompt-check prompt-check-error" role="alert">
          <strong>少了程式一定會填、不能沒有的變數，這樣不能儲存：</strong>
          <ul>
            {check.missing.map((v) => (
              <li key={v.name}>
                <code>{`{{${v.name}}}`}</code> {v.label}
              </li>
            ))}
          </ul>
        </div>
      )}
      {check.unknown.length > 0 && (
        <div className="prompt-check prompt-check-warn" role="status">
          <strong>程式不會填這些變數，它們會原樣送給 AI（可以儲存）：</strong>{" "}
          {check.unknown.map((n) => (
            <code key={n}>{`{{${n}}}`}</code>
          ))}
        </div>
      )}
      <label className="field-label" htmlFor={`${idBase}-note`}>
        這次改了什麼（選填，最多 {NOTE_MAX} 字）
      </label>
      <input
        id={`${idBase}-note`}
        value={note}
        maxLength={NOTE_MAX}
        onChange={(e) => setNote(e.target.value)}
        placeholder="例如：暖身改成 3 個問題"
      />
      <div className="button-row admin-actions">
        <button
          type="button"
          className="primary-button"
          onClick={() => void save()}
          disabled={busy || !changed || blank || check.missing.length > 0}
        >
          {busy ? "儲存中…" : "儲存為新版本"}
        </button>
      </div>
      <ErrorText message={error} />
      {saved && <Notice tone="success">{saved}</Notice>}
      {canTry && (
        <TryRun
          promptKey={prompt.prompt_key}
          systemText={systemText}
          userTemplate={userTemplate}
          blocked={blank || check.missing.length > 0}
        />
      )}
      {prompt.version_no > 0 && <History prompt={prompt} onRestored={onSaved} />}
    </article>
  );
}

function History({ prompt, onRestored }: { prompt: PromptVersion; onRestored: (p: PromptVersion) => void }) {
  const [versions, setVersions] = useState<PromptVersion[] | null>(null);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string | null>(null);

  // Opens (and reloads when a new version was saved from the editor).
  useEffect(() => {
    if (!open) return;
    let alive = true;
    api.admin
      .promptVersions(prompt.prompt_key)
      .then((v) => alive && setVersions(v))
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [open, prompt.prompt_key, prompt.version_no]);

  const current = versions?.find((v) => v.version_no === prompt.version_no) ?? prompt;
  const old = versions?.find((v) => v.version_no === selected) ?? null;

  async function restore(from: PromptVersion) {
    setBusy(true);
    setError(null);
    try {
      const saved = await api.admin.createPrompt({
        prompt_key: from.prompt_key,
        system_text: from.system_text,
        user_template: from.user_template,
        note: `還原自 v${from.version_no}`,
        restored_from: from.version_no,
      });
      setDone(`已用 v${from.version_no} 的文字建立 v${saved.version_no}。`);
      setConfirming(false);
      setSelected(null);
      onRestored(saved);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="prompt-history">
      <button type="button" className="secondary-button" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        {open ? "收起版本紀錄" : `版本紀錄（共 ${prompt.version_no} 版）`}
      </button>
      {open && (
        <div className="prompt-history-body">
          <ErrorText message={error} />
          {!versions && !error && <Loading />}
          {done && <Notice tone="success">{done}</Notice>}
          {versions && (
            <ol className="version-list" aria-label="版本紀錄">
              {versions.map((v) => (
                <li key={v.version_no} className={v.version_no === selected ? "is-selected" : undefined}>
                  <div className="version-line">
                    <strong>v{v.version_no}</strong>
                    {v.version_no === prompt.version_no && <span className="soft-tag">使用中</span>}
                    <span className="version-meta">
                      {formatDateTime(v.created_at)} ·{" "}
                      {v.by_me ? "你" : v.created_by_research_id ? v.created_by_research_id : "初始版本"}
                      {v.restored_from ? ` · 還原自 v${v.restored_from}` : ""}
                    </span>
                    {v.version_no !== prompt.version_no && (
                      <button
                        type="button"
                        className="link-button"
                        aria-label={`查看 v${v.version_no} 並和目前版本比較`}
                        aria-pressed={v.version_no === selected}
                        onClick={() => {
                          setSelected(v.version_no === selected ? null : v.version_no);
                          setConfirming(false);
                          setDone(null);
                        }}
                      >
                        {v.version_no === selected ? "收起" : "查看與比較"}
                      </button>
                    )}
                  </div>
                  {v.note && <p className="version-note">{v.note}</p>}
                </li>
              ))}
            </ol>
          )}
          {old && (
            <div className="version-compare">
              <h4>v{old.version_no} 和目前的 v{current.version_no} 比較</h4>
              <p className="field-hint">
                紅色：目前有、v{old.version_no} 沒有的行（用 v{old.version_no} 建立新版本後會消失）。綠色：v{old.version_no}
                有、目前沒有的行（會回來）。
              </p>
              <DiffBlock title="系統提示詞" from={current.system_text} to={old.system_text} />
              <DiffBlock title="使用者範本" from={current.user_template} to={old.user_template} />
              <div className="button-row">
                {!confirming ? (
                  <button type="button" className="primary-button" onClick={() => setConfirming(true)}>
                    用這一版建立新版本
                  </button>
                ) : (
                  <>
                    <span className="field-hint">會新增 v{prompt.version_no + 1}（內容和 v{old.version_no} 相同），並立刻成為使用中的版本。</span>
                    <button type="button" className="primary-button" disabled={busy} onClick={() => void restore(old)}>
                      {busy ? "建立中…" : `確定建立 v${prompt.version_no + 1}`}
                    </button>
                    <button type="button" className="secondary-button" onClick={() => setConfirming(false)}>
                      取消
                    </button>
                  </>
                )}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function DiffBlock({ title, from, to }: { title: string; from: string; to: string }) {
  const lines = useMemo(() => lineDiff(from, to), [from, to]);
  const changes = lines.filter((l) => l.kind !== "same").length;
  return (
    <div className="diff-block">
      <strong>{title}</strong>
      {changes === 0 ? (
        <p className="field-hint">和目前的一樣。</p>
      ) : (
        <pre className="diff" aria-label={`${title}的差異`}>
          {lines.map((l, i) => (
            <div key={i} className={`diff-line diff-${l.kind}`}>
              <span className="diff-mark" aria-hidden="true">
                {l.kind === "add" ? "+" : l.kind === "del" ? "−" : " "}
              </span>
              <span>{l.text || " "}</span>
            </div>
          ))}
        </pre>
      )}
    </div>
  );
}

const SAMPLE_PASSAGE = `Honeybees communicate by dancing. When a forager finds a good patch of flowers, she returns to the hive and moves in a pattern called the waggle dance.

The angle of the dance shows the direction of the flowers, and its length shows the distance. Other bees watch the dance and fly to the same place.

Scientists have studied this behaviour for decades. They believe it is one of the most complex forms of communication among insects.`;

const STATUS_TEXT: Record<TryResponse["status"], string> = {
  ok: "模型有回答",
  disabled: "這個環境沒有開啟 AI",
  timeout: "AI 沒有在時間內回應",
  refused: "模型拒絕回答",
  error: "這次沒有拿到可用的輸出",
};

/** 試產生：跑一次還沒儲存的草稿，看原始輸出和平台檢查的結果。只是預覽。 */
function TryRun({
  promptKey,
  systemText,
  userTemplate,
  blocked,
}: {
  promptKey: string;
  systemText: string;
  userTemplate: string;
  blocked: boolean;
}) {
  const [passage, setPassage] = useState("");
  const [question, setQuestion] = useState("");
  const [profile, setProfile] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<TryResponse | null>(null);
  const idBase = `try-${promptKey}`;
  const needsQuestion = promptKey === "prep_ask";
  const ready = passage.trim() !== "" && (!needsQuestion || question.trim() !== "");

  async function run() {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      setResult(
        await api.admin.tryPrompt(promptKey, {
          system_text: systemText,
          user_template: userTemplate,
          sample: {
            passage,
            ...(needsQuestion ? { question } : {}),
            ...(profile.trim() ? { class_profile: profile } : {}),
          },
        }),
      );
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <details className="try-run">
      <summary>試產生：先試試看再儲存</summary>
      <Notice>
        這是預覽。用的是上面<strong>還沒儲存</strong>的文字，只跑一次；結果不會存成版本、不會出現在任何老師的畫面，也不會進研究資料。
        用你貼的短文試，不會讀任何老師的資料。
      </Notice>
      <label className="field-label" htmlFor={`${idBase}-passage`}>
        範例教材（英文，段落之間空一行）
      </label>
      <textarea id={`${idBase}-passage`} rows={6} value={passage} onChange={(e) => setPassage(e.target.value)} />
      <div className="button-row">
        <button type="button" className="secondary-button" onClick={() => setPassage(SAMPLE_PASSAGE)}>
          放入範例短文
        </button>
      </div>
      {needsQuestion && (
        <>
          <label className="field-label" htmlFor={`${idBase}-question`}>
            老師的問題
          </label>
          <input id={`${idBase}-question`} value={question} onChange={(e) => setQuestion(e.target.value)} />
        </>
      )}
      <label className="field-label" htmlFor={`${idBase}-profile`}>
        班級描述（選填）
      </label>
      <input id={`${idBase}-profile`} value={profile} onChange={(e) => setProfile(e.target.value)} />
      <div className="button-row">
        <button
          type="button"
          className="primary-button"
          disabled={busy || !ready || blocked}
          onClick={() => void run()}
        >
          {busy ? "試產生中…" : "試產生"}
        </button>
        {blocked && <span className="field-hint">上面的文字還有問題（空白或少了必要的變數），先處理才能試。</span>}
      </div>
      <ErrorText message={error} />
      {result && (
        <div className="try-result" aria-label="試產生的結果">
          <h4>
            {STATUS_TEXT[result.status]} <span className="soft-tag">預覽</span>
          </h4>
          {result.message && <p>{result.message}</p>}
          {result.checks.length > 0 && (
            <ul className="try-checks" aria-label="平台檢查">
              {result.checks.map((c, i) => (
                <li key={`${c.name}-${i}`} className={c.passed ? "try-pass" : "try-fail"}>
                  <Icon name={c.passed ? "check" : "close"} className="is-glyph" /> {c.passed ? "通過" : "沒通過"}：<code>{c.name}</code>
                  {c.detail && <span className="try-detail"> {c.detail}</span>}
                </li>
              ))}
            </ul>
          )}
          {result.notes.length > 0 && (
            <p className="field-hint">平台會自動修正：{result.notes.join("；")}</p>
          )}
          {result.output !== null && result.output !== undefined && (
            <>
              <strong>模型的原始輸出（還沒經過任何修補）</strong>
              <pre className="try-output">{JSON.stringify(result.output, null, 2)}</pre>
            </>
          )}
          <details>
            <summary>實際送給模型的訊息</summary>
            <pre className="try-output">{`【系統】\n${result.system}\n\n【使用者】\n${result.user}`}</pre>
          </details>
        </div>
      )}
    </details>
  );
}
