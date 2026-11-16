import { useCallback, useEffect, useId, useState, type FormEvent, type ReactNode } from "react";
import { errorMessage } from "../../api/client";
import type { AdminLibraryList, AdminLibraryQuery, AdminLibraryUpdate } from "../../api/types";
import { ErrorText, Loading } from "../common";
import { AUTO_CATEGORY_LABEL, parseTags, tagsProblem, tagsText, titleProblem } from "./prefill";

export interface ListItem {
  id: number;
  title: string;
  category: string;
  tags: string[];
}

export interface ListFilters {
  q: string;
  category: string;
}

const titleOf = (item: { title: string }) => item.title.trim() || "未命名";

/**
 * One admin list (GET /api/admin/library?kind=…) for the given filters, reloaded when `revision`
 * changes. `patch` changes items in place (an edit, a deletion, a poll's news).
 */
export function useAdminList<T extends { id: number }>(
  load: (query: AdminLibraryQuery) => Promise<AdminLibraryList<T>>,
  filters: ListFilters,
  revision: number,
) {
  const { q, category } = filters;
  const key = JSON.stringify([q, category, revision]);
  const [state, setState] = useState<{ key: string; items: T[]; next: string | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    load({ q: q || undefined, category: category || undefined })
      .then((res) => {
        if (!alive) return;
        setState({ key, items: res.items, next: res.next_cursor ?? null });
        setError(null);
      })
      .catch((err: unknown) => alive && setError(errorMessage(err)));
    return () => {
      alive = false;
    };
  }, [load, q, category, key]);

  // While another search loads, the rows of the last one stay (no flash of 載入中).
  const current = state?.key === key ? state : null;
  const next = current?.next ?? null;

  const more = useCallback(() => {
    if (!next) return;
    setBusy(true);
    load({ q: q || undefined, category: category || undefined, cursor: next })
      .then((res) => {
        setState((s) =>
          s && s.key === key ? { key, items: [...s.items, ...res.items], next: res.next_cursor ?? null } : s,
        );
        setError(null);
      })
      .catch((err: unknown) => setError(errorMessage(err)))
      .finally(() => setBusy(false));
  }, [load, q, category, key, next]);

  /** Maps every item; null takes it out. */
  const patch = useCallback((fn: (item: T) => T | null) => {
    setState((s) => {
      if (!s) return s;
      let changed = false;
      const items: T[] = [];
      for (const item of s.items) {
        const out = fn(item);
        if (out !== item) changed = true;
        if (out !== null) items.push(out);
      }
      return changed ? { ...s, items } : s;
    });
  }, []);

  return { items: state?.items ?? null, loading: current === null, error, busy, hasMore: next !== null, more, patch };
}

interface Draft {
  id: number;
  title: string;
  category: string;
  tags: string;
}

/**
 * The admin list under a tab's upload: search and a category filter, and per row 編輯 (title,
 * category, tags) and 刪除 after an in-page confirmation.
 */
export function LibraryList<T extends ListItem>({
  label,
  categories,
  allowAuto,
  list,
  filters,
  onFilters,
  renderMeta,
  renderActions,
  save,
  remove,
  deleteNote,
}: {
  /** 知識庫教材 or 平台範本 */
  label: string;
  categories: readonly string[];
  /** A template's category may still be 自動判斷 (""). */
  allowAuto: boolean;
  list: ReturnType<typeof useAdminList<T>>;
  filters: ListFilters;
  onFilters: (f: ListFilters) => void;
  renderMeta: (item: T) => ReactNode;
  /** Actions of a row besides 編輯 and 刪除. */
  renderActions?: (item: T) => ReactNode;
  save: (id: number, body: AdminLibraryUpdate) => Promise<Partial<T>>;
  remove: (id: number) => Promise<unknown>;
  /** What deleting takes with it, said before the admin confirms. */
  deleteNote: string;
}) {
  const ids = useId();
  const [draftQ, setDraftQ] = useState(filters.q);
  const [editing, setEditing] = useState<Draft | null>(null);
  const [confirming, setConfirming] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [rowError, setRowError] = useState<{ id: number; message: string } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const { items } = list;

  function onSearch(e: FormEvent) {
    e.preventDefault();
    onFilters({ ...filters, q: draftQ.trim() });
  }

  async function onSave(e: FormEvent, item: T) {
    e.preventDefault();
    if (!editing) return;
    const tags = parseTags(editing.tags);
    const problem = titleProblem(editing.title) ?? tagsProblem(tags);
    if (problem) {
      setRowError({ id: item.id, message: problem });
      return;
    }
    const body: AdminLibraryUpdate = {};
    if (editing.title.trim() !== item.title) body.title = editing.title.trim();
    if (editing.category !== item.category && editing.category !== "") body.category = editing.category;
    if (tags.join("\u0000") !== item.tags.join("\u0000")) body.tags = tags;
    if (Object.keys(body).length === 0) {
      setEditing(null);
      return;
    }
    setBusy(true);
    setRowError(null);
    try {
      const saved = await save(item.id, body);
      list.patch((x) => (x.id === item.id ? { ...x, ...saved } : x));
      setEditing(null);
      setNotice(`已儲存「${titleOf({ title: saved.title ?? body.title ?? item.title })}」。`);
    } catch (err) {
      setRowError({ id: item.id, message: errorMessage(err) });
    } finally {
      setBusy(false);
    }
  }

  async function onDelete(item: T) {
    setBusy(true);
    setRowError(null);
    try {
      await remove(item.id);
      list.patch((x) => (x.id === item.id ? null : x));
      setConfirming(null);
      setNotice(`已刪除「${titleOf(item)}」。`);
    } catch (err) {
      setRowError({ id: item.id, message: errorMessage(err) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="section adlib-list" aria-labelledby={`${ids}-heading`}>
      <h2 id={`${ids}-heading`}>已上傳的{label}</h2>
      <form className="adlib-filters" onSubmit={onSearch} role="search" aria-label={`搜尋${label}`}>
        <div className="adlib-filter-q">
          <label className="field-label" htmlFor={`${ids}-q`}>
            標題、標籤或文字
          </label>
          <div className="inline-form">
            <input id={`${ids}-q`} value={draftQ} onChange={(e) => setDraftQ(e.target.value)} />
            <button type="submit" className="secondary-button">
              搜尋
            </button>
          </div>
        </div>
        <div>
          <label className="field-label" htmlFor={`${ids}-category`}>
            類別
          </label>
          <select
            id={`${ids}-category`}
            value={filters.category}
            onChange={(e) => onFilters({ ...filters, category: e.target.value })}
          >
            <option value="">全部</option>
            {categories.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </div>
      </form>

      <ErrorText message={list.error} />
      {notice && (
        <p className="quiet-note" role="status">
          {notice}
        </p>
      )}
      {(!items || list.loading) && !list.error && <Loading label={items ? "搜尋中…" : undefined} />}
      {items && items.length === 0 && (
        <p className="empty-line">
          {filters.q || filters.category ? `沒有符合的${label}。` : `還沒有${label}。用上面的上傳區加入。`}
        </p>
      )}
      {items && items.length > 0 && (
        <ul className="rows adlib-rows" aria-label={label}>
          {items.map((item) => (
            <li key={item.id} className="row">
              <div>
                <h3>{titleOf(item)}</h3>
                {renderMeta(item)}
                {item.tags.length > 0 && (
                  <p className="adlib-tags">
                    {item.tags.map((t) => (
                      <span key={t} className="soft-tag">
                        {t}
                      </span>
                    ))}
                  </p>
                )}
              </div>
              <p className="side">
                {renderActions?.(item)}
                <button
                  type="button"
                  className="text-link"
                  aria-label={`編輯「${titleOf(item)}」`}
                  onClick={() => {
                    setConfirming(null);
                    setRowError(null);
                    setNotice(null);
                    setEditing({ id: item.id, title: item.title, category: item.category, tags: tagsText(item.tags) });
                  }}
                >
                  編輯
                </button>
                <button
                  type="button"
                  className="text-link"
                  aria-label={`刪除「${titleOf(item)}」`}
                  onClick={() => {
                    setEditing(null);
                    setRowError(null);
                    setNotice(null);
                    setConfirming(item.id);
                  }}
                >
                  刪除
                </button>
              </p>
              {editing?.id === item.id && (
                <form className="row-more adlib-edit" onSubmit={(e) => void onSave(e, item)} aria-label={`編輯「${titleOf(item)}」`}>
                  <div className="adlib-edit-title">
                    <label className="field-label" htmlFor={`${ids}-title-${item.id}`}>
                      標題
                    </label>
                    <input
                      id={`${ids}-title-${item.id}`}
                      value={editing.title}
                      disabled={busy}
                      onChange={(e) => setEditing({ ...editing, title: e.target.value })}
                    />
                  </div>
                  <div>
                    <label className="field-label" htmlFor={`${ids}-cat-${item.id}`}>
                      類別
                    </label>
                    <select
                      id={`${ids}-cat-${item.id}`}
                      value={editing.category}
                      disabled={busy}
                      onChange={(e) => setEditing({ ...editing, category: e.target.value })}
                    >
                      {allowAuto && item.category === "" && <option value="">{AUTO_CATEGORY_LABEL}</option>}
                      {!allowAuto && !categories.includes(item.category) && (
                        <option value={item.category}>{item.category || "請選類別"}</option>
                      )}
                      {categories.map((c) => (
                        <option key={c} value={c}>
                          {c}
                        </option>
                      ))}
                    </select>
                  </div>
                  <div>
                    <label className="field-label" htmlFor={`${ids}-tags-${item.id}`}>
                      標籤
                    </label>
                    <input
                      id={`${ids}-tags-${item.id}`}
                      value={editing.tags}
                      disabled={busy}
                      placeholder="以頓號分開"
                      onChange={(e) => setEditing({ ...editing, tags: e.target.value })}
                    />
                  </div>
                  <div className="button-row adlib-edit-actions">
                    <button type="submit" className="primary-button small-button" disabled={busy}>
                      {busy ? "儲存中…" : "儲存"}
                    </button>
                    <button
                      type="button"
                      className="text-link is-plain"
                      disabled={busy}
                      onClick={() => {
                        setEditing(null);
                        setRowError(null);
                      }}
                    >
                      取消
                    </button>
                  </div>
                </form>
              )}
              {confirming === item.id && (
                <div className="row-more adlib-delete" role="alertdialog" aria-label={`確認刪除「${titleOf(item)}」`}>
                  <p>
                    <strong>刪除「{titleOf(item)}」？</strong>
                  </p>
                  <p>{deleteNote}</p>
                  <div className="button-row">
                    <button type="button" className="danger-button" disabled={busy} onClick={() => void onDelete(item)}>
                      {busy ? "刪除中…" : "確定刪除"}
                    </button>
                    <button type="button" className="text-link is-plain" disabled={busy} onClick={() => setConfirming(null)}>
                      取消
                    </button>
                  </div>
                </div>
              )}
              {rowError?.id === item.id && (
                <div className="row-more">
                  <ErrorText message={rowError.message} />
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
      {items && list.hasMore && (
        <p className="after-rows">
          <button type="button" className="text-link" disabled={list.busy} onClick={list.more}>
            {list.busy ? "載入中…" : "載入更多"}
          </button>
        </p>
      )}
    </section>
  );
}
