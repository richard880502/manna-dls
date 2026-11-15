import { useCallback, useEffect, useId, useState, type ChangeEvent, type FormEvent } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../api/client";
import type {
  EmbeddingStatus,
  ItemTrack,
  KnowledgeImportResult,
  KnowledgeItemRow,
  KnowledgeSummary,
  RetireDemoResult,
  VocabularyRow,
} from "../api/types";
import { ErrorText, Loading, Notice } from "../components/common";
import { useApp } from "../context/AppContext";
import { ITEM_TRACK_LABELS, formatDateTime } from "../lib/labels";
import { can } from "../lib/permissions";

const CATEGORY_LABELS: Record<string, string> = {
  curriculum: "課綱",
  grammar: "文法",
  vocab: "詞彙教學",
  siop: "SIOP",
};
const KIND_LABELS: Record<string, string> = {
  learning_performance: "學習表現",
  learning_content: "學習內容",
};
const TRACKS: ItemTrack[] = ["general", "vocational", "comprehensive", "both"];
const PAGE_SIZE = 20;

const categoryLabel = (c: string) => CATEGORY_LABELS[c] ?? c;
const count = (n: number) => n.toLocaleString("en-US");

type ImportKey = "items" | "vocabulary";

export function KnowledgePage() {
  const { teacher } = useApp();
  const [summary, setSummary] = useState<KnowledgeSummary | null>(null);
  const [summaryError, setSummaryError] = useState<string | null>(null);
  /** Only one import waits for confirmation at a time, so the page has one filled button. */
  const [pending, setPending] = useState<ImportKey | null>(null);
  /** Bumped after every change so the lists below reload. */
  const [revision, setRevision] = useState(0);
  const allowed = can(teacher, "manage_knowledge");

  const loadSummary = useCallback(() => {
    api.knowledge
      .summary()
      .then((s) => {
        setSummary(s);
        setSummaryError(null);
      })
      .catch((err: unknown) => setSummaryError(errorMessage(err)));
  }, []);

  useEffect(() => {
    if (allowed) loadSummary();
  }, [allowed, loadSummary]);

  if (!allowed) {
    return (
      <section aria-labelledby="kb-heading">
        <div className="page-head">
          <h1 id="kb-heading">知識庫管理</h1>
          <p className="lead">
            這個頁面只開放給計畫主持人與知識庫管理員。如果你需要管理課綱條目或詞彙表，請向計畫主持人申請權限。
          </p>
        </div>
        <p className="after-rows">
          <Link className="text-link" to="/">
            回到班級
          </Link>
        </p>
      </section>
    );
  }

  function onChanged() {
    setPending(null);
    setRevision((r) => r + 1);
    loadSummary();
  }

  return (
    <section aria-labelledby="kb-heading" className="kb-page">
      <div className="page-head">
        <h1 id="kb-heading">知識庫管理</h1>
        <p className="lead">
          匯入與查看 108 課綱條目和大考中心參考詞彙表。匯入前可以先預覽會變動幾筆；內容有變的條目會成為新的一版，舊的只會停用，不會刪除。
        </p>
      </div>

      <StatusSection summary={summary} error={summaryError} onChanged={onChanged} />
      <SemanticIndexSection revision={revision} />

      <ImportSection
        title="匯入課綱條目"
        inputId="kb-items-csv"
        columns="item_code, category, school_track, title, content, source_doc, source_version, source_page（另可帶 official_code, item_kind, strand, strand_name, stage, subject, advanced, recurring, source_sha256）"
        groupLabel={(k) => ITEM_TRACK_LABELS[k as ItemTrack] ?? k}
        upload={api.knowledge.importItems}
        active={pending === "items"}
        onPreview={() => setPending("items")}
        onCancel={() => setPending(null)}
        onImported={onChanged}
      />
      <ImportSection
        title="匯入詞彙表"
        inputId="kb-vocabulary-csv"
        columns="word, headword, level, pos, kind, appendix_category, list_version, source_doc, source_page, source_sha256"
        groupLabel={(k) => (k === "appendix" ? "附錄" : `第 ${k} 級`)}
        upload={api.knowledge.importVocabulary}
        active={pending === "vocabulary"}
        onPreview={() => setPending("vocabulary")}
        onCancel={() => setPending(null)}
        onImported={onChanged}
      />

      <ItemsBrowser revision={revision} />
      <VocabularyBrowser revision={revision} />
    </section>
  );
}

// ---------------------------------------------------------------- 目前內容

function StatusSection({
  summary,
  error,
  onChanged,
}: {
  summary: KnowledgeSummary | null;
  error: string | null;
  onChanged: () => void;
}) {
  if (!summary) {
    return (
      <section className="section" aria-labelledby="kb-status-heading">
        <h2 id="kb-status-heading">目前內容</h2>
        {error ? <ErrorText message={error} /> : <Loading />}
      </section>
    );
  }
  const real = summary.knowledge.groups.filter((g) => !g.demo);
  const curriculum = real.filter((g) => g.category === "curriculum");
  const sum = (groups: { count: number }[]) => groups.reduce((n, g) => n + g.count, 0);
  const byTrack = TRACKS.map((t) => ({ track: t, n: sum(curriculum.filter((g) => g.school_track === t)) })).filter(
    (x) => x.n > 0,
  );
  const others = real.filter((g) => g.category !== "curriculum");
  const otherCategories = [...new Set(others.map((g) => g.category))];
  const levels = summary.vocabulary.groups.filter((g) => g.level !== null);
  const appendix = sum(
    summary.vocabulary.groups.filter((g) => g.level === null).map((g) => ({ count: g.forms })),
  );
  const versions = [...new Set(summary.vocabulary.groups.map((g) => g.list_version))];

  return (
    <section className="section" aria-labelledby="kb-status-heading">
      <h2 id="kb-status-heading">目前內容</h2>
      <ErrorText message={error} />
      <ul className="rows is-compact" aria-label="目前內容">
        <li className="row">
          <div>
            <h3>課綱條目</h3>
            <p className="meta">
              {curriculum.length === 0
                ? "還沒有匯入課綱條目。"
                : byTrack.map((x) => `${ITEM_TRACK_LABELS[x.track]} ${count(x.n)} 條`).join(" · ")}
            </p>
          </div>
          <p className="side">{count(sum(curriculum))} 條</p>
        </li>
        {others.length > 0 && (
          <li className="row">
            <div>
              <h3>其他條目</h3>
              <p className="meta">
                {otherCategories
                  .map((c) => `${categoryLabel(c)} ${count(sum(others.filter((g) => g.category === c)))} 條`)
                  .join(" · ")}
              </p>
            </div>
            <p className="side">{count(sum(others))} 條</p>
          </li>
        )}
        <li className="row">
          <div>
            <h3>詞彙表</h3>
            <p className="meta">
              {summary.vocabulary.active_forms === 0
                ? "還沒有匯入詞彙表；匯入後教師才能分析詞彙級別。"
                : `${versions.join("、")}：${levels
                    .map((g) => `第 ${g.level} 級 ${count(g.entries)} 詞條`)
                    .join(" · ")}${appendix > 0 ? ` · 附錄 ${count(appendix)} 詞` : ""}`}
            </p>
          </div>
          <p className="side">{count(summary.vocabulary.active_forms)} 個詞形</p>
        </li>
        <DemoRow summary={summary} onChanged={onChanged} />
      </ul>

      {summary.recent_batches.length > 0 && (
        <>
          <h3 className="kb-subhead">最近的匯入</h3>
          <ul className="rows is-compact" aria-label="最近的匯入">
            {summary.recent_batches.map((b) => (
              <li key={b.id} className="row">
                <div>
                  <p>
                    {b.kind === "vocabulary" ? "詞彙表" : "課綱與知識庫條目"}：新增 {count(b.inserted)}、更新{" "}
                    {count(b.updated)}、不變 {count(b.unchanged)}
                  </p>
                  <p className="meta">
                    {formatDateTime(b.created_at)} · {b.imported_by ?? "（帳號已移除）"} · 檔案 SHA-256{" "}
                    <code className="sha">{b.file_sha256}</code>
                  </p>
                </div>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

/** The seeded demo items: retired only when someone asks, after seeing what that leaves. */
function DemoRow({ summary, onChanged }: { summary: KnowledgeSummary; onChanged: () => void }) {
  const [preview, setPreview] = useState<RetireDemoResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<number | null>(null);
  const active = summary.knowledge.demo_active;
  if (active === 0 && done === null) return null;

  async function run(dryRun: boolean) {
    setBusy(true);
    setError(null);
    try {
      const res = await api.knowledge.retireDemo(dryRun);
      if (dryRun) {
        setPreview(res);
      } else {
        setPreview(null);
        setDone(res.retired);
        onChanged();
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <li className="row">
      <div>
        <h3>示範條目</h3>
        <p className="meta">
          {active > 0
            ? "系統內建的示範內容，不是官方來源。正式條目匯入後不會自動停用。"
            : "示範條目都已停用。"}
        </p>
      </div>
      <p className="side">
        <span>{count(active)} 條使用中</span>
        {active > 0 && !preview && (
          <button type="button" className="text-link" disabled={busy} onClick={() => void run(true)}>
            停用示範條目
          </button>
        )}
      </p>
      {preview && (
        <div className="row-more">
          <Notice tone="warn">
            將停用 {count(preview.demo_active)} 條示範條目（
            {Object.entries(preview.demo_by_category)
              .map(([c, n]) => `${categoryLabel(c)} ${n}`)
              .join("、")}
            ）；條目會保留在資料庫，只是不再被引用。
            {preview.real_active === 0 && " 目前還沒有正式條目，請先匯入正式條目再停用。"}
            {preview.categories_left_empty.length > 0 &&
              ` 停用後，${preview.categories_left_empty.map(categoryLabel).join("、")}類別將沒有任何條目，教案建議只會引用其他類別。`}
          </Notice>
          <div className="button-row">
            <button
              type="button"
              className="secondary-button"
              disabled={busy || preview.real_active === 0}
              onClick={() => void run(false)}
            >
              {busy ? "停用中…" : "確認停用"}
            </button>
            <button type="button" className="text-link is-plain" disabled={busy} onClick={() => setPreview(null)}>
              取消
            </button>
          </div>
        </div>
      )}
      {done !== null && (
        <div className="row-more">
          <Notice tone="success">已停用 {count(done)} 條示範條目。</Notice>
        </div>
      )}
      {error && (
        <div className="row-more">
          <ErrorText message={error} />
        </div>
      )}
    </li>
  );
}

// ---------------------------------------------------------------- 語意索引

/** How often the status is polled while an indexing run is in progress. */
const EMBEDDING_POLL_MS = 1500;

function SemanticIndexSection({ revision }: { revision: number }) {
  const [status, setStatus] = useState<EmbeddingStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    api.knowledge
      .embeddings()
      .then((s) => {
        setStatus(s);
        setError(null);
      })
      .catch((err: unknown) => setError(errorMessage(err)));
  }, []);

  // Reload after every import: the import starts an indexing run in the background.
  useEffect(() => load(), [load, revision]);

  const running = status?.running ?? false;
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(load, EMBEDDING_POLL_MS);
    return () => window.clearInterval(timer);
  }, [running, load]);

  async function rebuild() {
    setBusy(true);
    setError(null);
    try {
      setStatus(await api.knowledge.rebuildEmbeddings());
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  const heading = <h2 id="kb-semantic-heading">語意索引</h2>;
  if (!status) {
    return (
      <section className="section" aria-labelledby="kb-semantic-heading">
        {heading}
        {error ? <ErrorText message={error} /> : <Loading />}
      </section>
    );
  }
  const job = status.job;
  const missing = status.active_items - status.embedded;
  return (
    <section className="section" aria-labelledby="kb-semantic-heading">
      {heading}
      <p className="lead">
        語意索引讓英文的教材或目標也能找到中文的課綱條文：檢索時同時比對關鍵字與語意相似度。沒有索引的條目仍可用關鍵字找到；模型無法使用時自動改用關鍵字，不影響備課功能。
      </p>
      {!status.enabled ? (
        <Notice tone="info">尚未設定語意索引模型，目前只用關鍵字檢索。</Notice>
      ) : (
        <ul className="rows is-compact" aria-label="語意索引">
          <li className="row">
            <div>
              <h3>
                {count(status.active_items)} 條中 {count(status.embedded)} 條已建立語意索引
              </h3>
              <p className="meta">
                模型 {status.model}（{status.dims} 維）
                {status.other_model > 0 && `；另有 ${count(status.other_model)} 條是舊模型的索引，檢索不使用，重建後取代`}
              </p>
              {running && job && (
                <p className="meta" role="status">
                  建立中：{count(job.embedded)} / {count(job.total)}
                </p>
              )}
              {!running && job?.status === "failed" && (
                <p className="meta">
                  上次建立在 {count(job.embedded)} / {count(job.total)} 時中斷（{job.error ?? "原因不明"}
                  ）。已完成的部分會保留，再按一次會從缺少的條目繼續。
                </p>
              )}
              {!running && job?.status === "done" && job.finished_at && (
                <p className="meta">上次建立：{formatDateTime(job.finished_at)}</p>
              )}
            </div>
            <p className="side">
              <button
                type="button"
                className={missing > 0 ? "secondary-button" : "text-link"}
                disabled={busy || running}
                onClick={() => void rebuild()}
              >
                {running ? "建立中…" : missing > 0 ? "建立語意索引" : "重新檢查"}
              </button>
            </p>
          </li>
        </ul>
      )}
      {error && <ErrorText message={error} />}
    </section>
  );
}

// ---------------------------------------------------------------- 匯入

function ImportSection({
  title,
  inputId,
  columns,
  groupLabel,
  upload,
  active,
  onPreview,
  onCancel,
  onImported,
}: {
  title: string;
  inputId: string;
  columns: string;
  groupLabel: (key: string) => string;
  upload: (csv: string, dryRun: boolean) => Promise<KnowledgeImportResult>;
  /** This import holds the page's pending confirmation. */
  active: boolean;
  onPreview: () => void;
  onCancel: () => void;
  onImported: () => void;
}) {
  const [file, setFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<KnowledgeImportResult | null>(null);
  const [result, setResult] = useState<KnowledgeImportResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const shown = active ? preview : null;

  function onFile(e: ChangeEvent<HTMLInputElement>) {
    setFile(e.target.files?.[0] ?? null);
    setPreview(null);
    setResult(null);
    setError(null);
    if (active) onCancel();
  }

  async function run(dryRun: boolean) {
    if (!file) {
      setError("請先選擇 CSV 檔案。");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const res = await upload(await file.text(), dryRun);
      if (dryRun) {
        setResult(null);
        setPreview(res);
        onPreview();
      } else {
        setPreview(null);
        setResult(res);
        onImported();
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  const groups = (r: KnowledgeImportResult) =>
    Object.entries(r.by_track ?? r.by_level ?? {})
      .map(([k, n]) => `${groupLabel(k)} ${count(n)}`)
      .join(" · ");
  const nothingToDo = shown !== null && shown.inserted + shown.updated === 0;

  return (
    <section className="section" aria-labelledby={`${inputId}-heading`}>
      <h2 id={`${inputId}-heading`}>{title}</h2>
      <p className="quiet-note">
        CSV（UTF-8），欄位：<code className="kb-columns">{columns}</code>
      </p>
      <label className="field-label" htmlFor={inputId}>
        選擇 CSV 檔案
      </label>
      <input id={inputId} type="file" accept=".csv,text/csv" onChange={onFile} disabled={busy} />
      {!shown && (
        <div className="button-row">
          <button type="button" className="secondary-button" onClick={() => void run(true)} disabled={busy}>
            {busy ? "檢查中…" : "預覽"}
          </button>
        </div>
      )}
      {shown && (
        <>
          <Notice tone="info">
            預覽（尚未寫入）：檔案共 {count(shown.rows)} 列（{groups(shown)}）。新增 {count(shown.inserted)}、更新{" "}
            {count(shown.updated)}、不變 {count(shown.unchanged)}。
            {nothingToDo && " 檔案內容和目前資料相同，不需要匯入。"}
            <br />
            檔案 SHA-256：<code className="sha">{shown.sha256}</code>
          </Notice>
          <div className="button-row">
            {!nothingToDo && (
              <button type="button" className="primary-button" onClick={() => void run(false)} disabled={busy}>
                {busy ? "匯入中…" : "確認匯入"}
              </button>
            )}
            <button
              type="button"
              className="text-link is-plain"
              disabled={busy}
              onClick={() => {
                setPreview(null);
                onCancel();
              }}
            >
              {nothingToDo ? "關閉預覽" : "取消"}
            </button>
          </div>
        </>
      )}
      {result && (
        <Notice tone="success">
          已匯入：新增 {count(result.inserted)}、更新 {count(result.updated)}、不變 {count(result.unchanged)}。
          <br />
          檔案 SHA-256：<code className="sha">{result.sha256}</code>
        </Notice>
      )}
      <ErrorText message={error} />
    </section>
  );
}

// ---------------------------------------------------------------- 查看條目

interface Listing<T> {
  total: number;
  items: T[];
}

/** Loads a paged list; `key` changing (filters, a new import) starts over from the first page. */
function usePaged<T>(key: string, fetchPage: (offset: number) => Promise<Listing<T>>) {
  const [state, setState] = useState<{ key: string; list: Listing<T> } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(
    (offset: number) => {
      let alive = true;
      setBusy(true);
      fetchPage(offset)
        .then((page) => {
          if (!alive) return;
          setState((prev) => ({
            key,
            list:
              offset > 0 && prev?.key === key
                ? { total: page.total, items: [...prev.list.items, ...page.items] }
                : page,
          }));
          setError(null);
        })
        .catch((err: unknown) => alive && setError(errorMessage(err)))
        .finally(() => alive && setBusy(false));
      return () => {
        alive = false;
      };
    },
    // fetchPage is rebuilt from `key` by the caller
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key],
  );

  useEffect(() => load(0), [load]);

  const list = state?.key === key ? state.list : null;
  return { list, error, busy, more: () => void load(list?.items.length ?? 0) };
}

function ListFooter({
  list,
  busy,
  more,
  unit,
}: {
  list: Listing<unknown>;
  busy: boolean;
  more: () => void;
  unit: string;
}) {
  return (
    <p className="after-rows kb-more">
      <span className="quiet-note">
        顯示 {count(list.items.length)}／{count(list.total)} {unit}
      </span>
      {list.items.length < list.total && (
        <button type="button" className="text-link" disabled={busy} onClick={more}>
          {busy ? "載入中…" : "再看 20 筆"}
        </button>
      )}
    </p>
  );
}

function ItemsBrowser({ revision }: { revision: number }) {
  const ids = useId();
  const [draft, setDraft] = useState("");
  const [q, setQ] = useState("");
  const [track, setTrack] = useState("");
  const [category, setCategory] = useState("");
  const [kind, setKind] = useState("");
  const [status, setStatus] = useState<"active" | "retired" | "all">("active");
  const key = JSON.stringify([q, track, category, kind, status, revision]);
  const { list, error, busy, more } = usePaged<KnowledgeItemRow>(key, (offset) =>
    api.knowledge.items({
      q: q || undefined,
      track: track || undefined,
      category: category || undefined,
      kind: kind || undefined,
      status,
      limit: PAGE_SIZE,
      offset,
    }),
  );

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setQ(draft.trim());
  }

  return (
    <section className="section" aria-labelledby={`${ids}-heading`}>
      <h2 id={`${ids}-heading`}>查看條目</h2>
      <p className="quiet-note">只能查看。要修改條文，請更新 CSV 後重新匯入。</p>
      <form className="kb-filters" onSubmit={onSubmit} role="search" aria-label="搜尋條目">
        <div className="kb-filter-wide">
          <label className="field-label" htmlFor={`${ids}-q`}>
            代碼或文字
          </label>
          <div className="inline-form">
            <input
              id={`${ids}-q`}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder="例如：3-Ⅴ-7 或 閱讀"
            />
            <button type="submit" className="secondary-button">
              搜尋
            </button>
          </div>
        </div>
        <div>
          <label className="field-label" htmlFor={`${ids}-track`}>
            學校類型
          </label>
          <select id={`${ids}-track`} value={track} onChange={(e) => setTrack(e.target.value)}>
            <option value="">全部</option>
            {TRACKS.map((t) => (
              <option key={t} value={t}>
                {ITEM_TRACK_LABELS[t]}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="field-label" htmlFor={`${ids}-category`}>
            類別
          </label>
          <select id={`${ids}-category`} value={category} onChange={(e) => setCategory(e.target.value)}>
            <option value="">全部</option>
            {Object.entries(CATEGORY_LABELS).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="field-label" htmlFor={`${ids}-kind`}>
            學習重點
          </label>
          <select id={`${ids}-kind`} value={kind} onChange={(e) => setKind(e.target.value)}>
            <option value="">全部</option>
            {Object.entries(KIND_LABELS).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="field-label" htmlFor={`${ids}-status`}>
            狀態
          </label>
          <select
            id={`${ids}-status`}
            value={status}
            onChange={(e) => setStatus(e.target.value as "active" | "retired" | "all")}
          >
            <option value="active">使用中</option>
            <option value="retired">已停用</option>
            <option value="all">全部</option>
          </select>
        </div>
      </form>
      <ErrorText message={error} />
      {!list && !error && <Loading />}
      {list && list.items.length === 0 && <p className="empty-line">沒有符合的條目。</p>}
      {list && list.items.length > 0 && (
        <>
          <ul className="rows is-compact" aria-label="條目">
            {list.items.map((item) => (
              <li key={`${item.item_code}-${item.version}`} className="row">
                <div>
                  <h3>{item.title}</h3>
                  <p className="kb-text">{item.content}</p>
                  <p className="meta">
                    <code>{item.item_code}</code> · {ITEM_TRACK_LABELS[item.school_track] ?? item.school_track} ·{" "}
                    {categoryLabel(item.category)}
                    {item.source_doc && ` · ${item.source_doc}`}
                    {item.source_version && `（${item.source_version}）`}
                    {item.source_page && ` 第 ${item.source_page} 頁`} · 第 {item.version} 版
                  </p>
                </div>
                {item.retired_at && <p className="side">已停用 {formatDateTime(item.retired_at)}</p>}
              </li>
            ))}
          </ul>
          <ListFooter list={list} busy={busy} more={more} unit="條" />
        </>
      )}
    </section>
  );
}

function VocabularyBrowser({ revision }: { revision: number }) {
  const ids = useId();
  const [draft, setDraft] = useState("");
  const [q, setQ] = useState("");
  const [level, setLevel] = useState("");
  const key = JSON.stringify([q, level, revision]);
  const { list, error, busy, more } = usePaged<VocabularyRow>(key, (offset) =>
    api.knowledge.vocabulary({ q: q || undefined, level: level || undefined, limit: PAGE_SIZE, offset }),
  );

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setQ(draft.trim());
  }

  return (
    <section className="section" aria-labelledby={`${ids}-heading`}>
      <h2 id={`${ids}-heading`}>查看詞彙表</h2>
      <form className="kb-filters" onSubmit={onSubmit} role="search" aria-label="搜尋詞彙表">
        <div className="kb-filter-wide">
          <label className="field-label" htmlFor={`${ids}-q`}>
            單字開頭
          </label>
          <div className="inline-form">
            <input
              id={`${ids}-q`}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder="例如：conserv"
              autoCapitalize="none"
              spellCheck={false}
            />
            <button type="submit" className="secondary-button">
              搜尋
            </button>
          </div>
        </div>
        <div>
          <label className="field-label" htmlFor={`${ids}-level`}>
            級別
          </label>
          <select id={`${ids}-level`} value={level} onChange={(e) => setLevel(e.target.value)}>
            <option value="">全部</option>
            {[1, 2, 3, 4, 5, 6].map((n) => (
              <option key={n} value={String(n)}>
                第 {n} 級
              </option>
            ))}
            <option value="appendix">附錄（未分級）</option>
          </select>
        </div>
      </form>
      <ErrorText message={error} />
      {!list && !error && <Loading />}
      {list && list.items.length === 0 && <p className="empty-line">沒有符合的詞。</p>}
      {list && list.items.length > 0 && (
        <>
          <ul className="rows is-compact" aria-label="詞彙">
            {list.items.map((w) => (
              <li key={`${w.word}-${w.headword}-${w.pos}-${w.kind}-${w.appendix_category}`} className="row">
                <div>
                  <h3 className="en">{w.word}</h3>
                  <p className="meta">
                    {w.kind === "appendix"
                      ? `附錄：${w.appendix_category}`
                      : `詞條 ${w.headword}${w.pos ? ` · ${w.pos}` : ""}`}
                    {w.source_page && ` · 第 ${w.source_page} 頁`} · {w.list_version}
                  </p>
                </div>
                <p className="side">{w.level === null ? "未分級" : `第 ${w.level} 級`}</p>
              </li>
            ))}
          </ul>
          <ListFooter list={list} busy={busy} more={more} unit="個詞形" />
        </>
      )}
    </section>
  );
}
