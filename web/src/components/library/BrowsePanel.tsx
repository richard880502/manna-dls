import { useCallback, useEffect, useId, useMemo, useState, type ReactNode } from "react";
import { api } from "../../api/client";
import type {
  BrowseFileType,
  FacetCount,
  MaterialBrowseQuery,
  MaterialCard as MaterialCardData,
  TemplateBrowseQuery,
  TemplateCard as TemplateCardData,
  WsInstructionLanguage,
} from "../../api/types";
import { templateConfig } from "../../lib/templates";
import { ErrorText } from "../common";
import { Icon, type IconName } from "../Icon";
import { MaterialCard, SkeletonCards, TemplateCard } from "./Cards";
import {
  ALL_LABEL,
  FILTER_LABEL,
  LOAD_MORE,
  MATERIAL_FILTERS,
  NO_MATERIALS,
  NO_TEMPLATES,
  REFERENCE_CATEGORIES,
  RESET_LABEL,
  SAMPLE_CATEGORIES,
  TEMPLATE_FILTERS,
  categoryChips,
  type MaterialArea,
  type TemplateArea,
} from "./labels";
import { libraryConfig, useBrowse, useSearchText, type BrowseResult } from "./useBrowse";

const TAGS_OFFERED = 10;

// ---------------------------------------------------------------- the controls

/** The big search field. Typing searches once it rests; Enter searches at once. */
function SearchField({
  label,
  placeholder,
  search,
}: {
  label: string;
  placeholder: string;
  search: ReturnType<typeof useSearchText>;
}) {
  const ids = useId();
  return (
    <form
      className="lib-search"
      role="search"
      onSubmit={(event) => {
        event.preventDefault();
        search.submit();
      }}
    >
      <label className="sr-only" htmlFor={`${ids}-q`}>
        {label}
      </label>
      <Icon name="search" className="lib-search-icon" />
      <input
        id={`${ids}-q`}
        className="lib-search-input"
        type="search"
        enterKeyHint="search"
        autoComplete="off"
        maxLength={200}
        value={search.text}
        placeholder={placeholder}
        onChange={(e) => search.setText(e.target.value)}
      />
      {search.text !== "" && (
        <button type="button" className="lib-search-clear" aria-label="清除搜尋" onClick={search.clear}>
          <Icon name="close" />
        </button>
      )}
    </form>
  );
}

/** 「全部 120」 then each category with its count; the chosen one is filled. */
function CategoryChips({
  chips,
  allCount,
  active,
  onChange,
}: {
  chips: FacetCount[];
  allCount: number | null;
  active: string;
  onChange: (key: string) => void;
}) {
  return (
    <div className="lib-chips" role="group" aria-label="分類">
      <button type="button" className="lib-chip" aria-pressed={active === ""} onClick={() => onChange("")}>
        {ALL_LABEL}
        {allCount !== null && " "}
        {allCount !== null && <span className="lib-chip-count">{allCount}</span>}
      </button>
      {chips.map((c) => (
        <button
          key={c.key}
          type="button"
          className="lib-chip"
          aria-pressed={active === c.key}
          onClick={() => onChange(active === c.key ? "" : c.key)}
        >
          {c.key} <span className="lib-chip-count">{c.count}</span>
        </button>
      ))}
    </div>
  );
}

interface FilterChip {
  id: string;
  label: string;
  icon: IconName;
  pressed: boolean;
}

/** 「只看：」 and its chips (each with an icon). */
function FilterChips({ filters, onToggle }: { filters: FilterChip[]; onToggle: (id: string) => void }) {
  const ids = useId();
  return (
    <div className="lib-filters" role="group" aria-labelledby={`${ids}-label`}>
      <span className="lib-filters-label" id={`${ids}-label`}>
        {FILTER_LABEL}
      </span>
      {filters.map((f) => (
        <button key={f.id} type="button" className="lib-filter" aria-pressed={f.pressed} onClick={() => onToggle(f.id)}>
          <Icon name={f.icon} />
          {f.label}
        </button>
      ))}
    </div>
  );
}

/** 「標籤：」: the most used tags (and any chosen one); every chosen tag must match. */
function TagChips({ tags, active, onToggle }: { tags: FacetCount[]; active: string[]; onToggle: (key: string) => void }) {
  const ids = useId();
  const offered = tags.filter((t) => t.key !== "").slice(0, TAGS_OFFERED);
  const keys = [...active, ...offered.map((t) => t.key).filter((k) => !active.includes(k))];
  if (keys.length === 0) return null;
  return (
    <div className="lib-filters is-tags" role="group" aria-labelledby={`${ids}-label`}>
      <span className="lib-filters-label" id={`${ids}-label`}>
        標籤：
      </span>
      {keys.map((key) => (
        <button
          key={key}
          type="button"
          className="lib-filter is-tag"
          aria-pressed={active.includes(key)}
          onClick={() => onToggle(key)}
        >
          {key}
        </button>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------- the frame

interface FrameProps<T> {
  searchLabel: string;
  searchPlaceholder: string;
  search: ReturnType<typeof useSearchText>;
  categories: FacetCount[];
  allCount: number | null;
  category: string;
  onCategory: (key: string) => void;
  filters: FilterChip[];
  onFilter: (id: string) => void;
  tags: FacetCount[];
  activeTags: string[];
  onTag: (key: string) => void;
  result: BrowseResult<T, unknown>;
  /** Something narrows the list (search, category, filters, tags). */
  narrowed: boolean;
  onReset: () => void;
  noMatch: string;
  /** What an area with nothing in it says (no search, no filter). */
  empty: ReactNode;
  listLabel: string;
  /** Beside the count (排序). */
  extra?: ReactNode;
  renderCard: (item: T) => ReactNode;
}

function BrowseFrame<T>({
  searchLabel,
  searchPlaceholder,
  search,
  categories,
  allCount,
  category,
  onCategory,
  filters,
  onFilter,
  tags,
  activeTags,
  onTag,
  result,
  narrowed,
  onReset,
  noMatch,
  empty,
  listLabel,
  extra,
  renderCard,
}: FrameProps<T>) {
  const { items, total, loading, loadingMore, hasMore, error, loadMore, retry } = result;
  return (
    <div className="lib-browse">
      <SearchField label={searchLabel} placeholder={searchPlaceholder} search={search} />
      <CategoryChips chips={categories} allCount={allCount} active={category} onChange={onCategory} />
      <FilterChips filters={filters} onToggle={onFilter} />
      <TagChips tags={tags} active={activeTags} onToggle={onTag} />

      {items !== null && (
        <div className="lib-count-row">
          <p className="lib-count" aria-live="polite">
            {loading ? "搜尋中…" : `共 ${total} 份`}
          </p>
          {extra}
        </div>
      )}

      {error && (
        <div className="lib-error">
          <ErrorText message={error} />
          <button type="button" className="text-link is-plain" onClick={retry}>
            再試一次
          </button>
        </div>
      )}

      {items === null && !error && (
        <>
          <p className="sr-only" role="status">
            載入中…
          </p>
          <SkeletonCards />
        </>
      )}

      {items !== null && items.length === 0 && !loading && !error && (
        <div className="lib-empty">
          {narrowed ? (
            <p className="empty-line">
              {noMatch}。
              <button type="button" className="text-link is-plain" onClick={onReset}>
                {RESET_LABEL}
              </button>
            </p>
          ) : (
            empty
          )}
        </div>
      )}

      {items !== null && items.length > 0 && (
        <ul className={`lib-grid${loading ? " is-busy" : ""}`} aria-label={listLabel} aria-busy={loading || undefined}>
          {items.map((item) => renderCard(item))}
        </ul>
      )}

      {items !== null && hasMore && !loading && (
        <p className="lib-more">
          <button type="button" className="secondary-button" disabled={loadingMore} onClick={loadMore}>
            {loadingMore ? "載入中…" : LOAD_MORE}
          </button>
        </p>
      )}
    </div>
  );
}

const sumOf = (list: FacetCount[] | undefined) => (list ? list.reduce((n, f) => n + f.count, 0) : null);
const toggleIn = (list: string[], key: string) => (list.includes(key) ? list.filter((k) => k !== key) : [...list, key]);

// ---------------------------------------------------------------- materials

type MaterialFacets = { categories: FacetCount[]; tags: FacetCount[]; file_types: FacetCount[] };

/**
 * 教材參考 (area=reference) or 我的教材 (area=mine) as a marketplace: search, category chips with
 * counts, 只看 chips, tags, and a grid of cards. The caller gives each card its actions.
 */
export function MaterialBrowser({
  area,
  renderActions,
  empty,
  refreshSignal = 0,
}: {
  area: MaterialArea;
  renderActions: (card: MaterialCardData) => ReactNode;
  empty: ReactNode;
  /** Bumped when the list changed elsewhere (an upload, a rename, a delete): read the first page again. */
  refreshSignal?: number;
}) {
  const search = useSearchText();
  const [category, setCategory] = useState("");
  const [tags, setTags] = useState<string[]>([]);
  const [fileType, setFileType] = useState<BrowseFileType | null>(null);
  const [fitsLesson, setFitsLesson] = useState(false);
  const [sort, setSort] = useState<"new" | "title">("new");

  const query = useMemo<MaterialBrowseQuery>(
    () => ({
      area,
      ...(search.q ? { q: search.q } : {}),
      ...(category ? { category } : {}),
      ...(tags.length > 0 ? { tag: tags } : {}),
      ...(fileType ? { file_type: fileType } : {}),
      ...(fitsLesson ? { fits_lesson: true } : {}),
      ...(sort !== "new" ? { sort } : {}),
    }),
    [area, search.q, category, tags, fileType, fitsLesson, sort],
  );
  const load = useCallback(
    (cursor: string | null, limit: number) =>
      api.browseMaterials({
        ...query,
        ...(cursor ? { cursor } : {}),
        ...(limit !== libraryConfig.pageSize ? { limit } : {}),
      }),
    [query],
  );
  const result = useBrowse<MaterialCardData, MaterialFacets>(load, JSON.stringify(query));
  const { retry } = result;
  useEffect(() => {
    if (refreshSignal > 0) retry();
  }, [refreshSignal, retry]);

  const narrowed = search.q !== "" || category !== "" || tags.length > 0 || fileType !== null || fitsLesson;
  const reset = () => {
    search.clear();
    setCategory("");
    setTags([]);
    setFileType(null);
    setFitsLesson(false);
  };
  const filters: FilterChip[] = MATERIAL_FILTERS.map((f) => ({
    id: f.id,
    label: f.label,
    icon: f.icon,
    pressed: f.fitsLesson ? fitsLesson : fileType === f.fileType,
  }));
  const toggleFilter = (id: string) => {
    const f = MATERIAL_FILTERS.find((x) => x.id === id);
    if (!f) return;
    if (f.fitsLesson) setFitsLesson((on) => !on);
    else if (f.fileType) setFileType((cur) => (cur === f.fileType ? null : (f.fileType ?? null)));
  };

  const facets = result.facets;
  const sortId = useId();
  return (
    <BrowseFrame
      searchLabel={area === "reference" ? "搜尋教材參考" : "搜尋我的教材"}
      searchPlaceholder={area === "reference" ? "搜尋標題、內容或標籤，例如：114 學測、閱讀測驗" : "搜尋我的教材的標題、內容或標籤"}
      search={search}
      categories={categoryChips(REFERENCE_CATEGORIES, facets?.categories ?? [], category)}
      allCount={sumOf(facets?.categories)}
      category={category}
      onCategory={setCategory}
      filters={filters}
      onFilter={toggleFilter}
      tags={facets?.tags ?? []}
      activeTags={tags}
      onTag={(key) => setTags((list) => toggleIn(list, key))}
      result={result}
      narrowed={narrowed}
      onReset={reset}
      noMatch={NO_MATERIALS}
      empty={empty}
      listLabel={area === "reference" ? "教材參考" : "我的教材"}
      extra={
        <label className="lib-sort" htmlFor={`${sortId}-sort`}>
          排序
          <select
            id={`${sortId}-sort`}
            value={sort}
            onChange={(e) => setSort(e.target.value === "title" ? "title" : "new")}
          >
            <option value="new">最新</option>
            <option value="title">標題</option>
          </select>
        </label>
      }
      renderCard={(card) => <MaterialCard key={card.id} card={card} actions={renderActions(card)} />}
    />
  );
}

// ---------------------------------------------------------------- templates

type TemplateFacets = { categories: FacetCount[]; tags: FacetCount[]; langs: FacetCount[] };

/**
 * 平台範本 (area=platform) or 我的範本 (area=mine) as a marketplace. 我的範本 is asked for again
 * every few seconds while one of them is still being read.
 */
export function TemplateBrowser({
  area,
  renderActions,
  empty,
  refreshSignal = 0,
}: {
  area: TemplateArea;
  renderActions: (card: TemplateCardData, update: (next: TemplateCardData) => void) => ReactNode;
  empty: ReactNode;
  refreshSignal?: number;
}) {
  const search = useSearchText();
  const [category, setCategory] = useState("");
  const [tags, setTags] = useState<string[]>([]);
  const [lang, setLang] = useState<WsInstructionLanguage | null>(null);

  const query = useMemo<TemplateBrowseQuery>(
    () => ({
      area,
      ...(search.q ? { q: search.q } : {}),
      ...(category ? { category } : {}),
      ...(tags.length > 0 ? { tag: tags } : {}),
      ...(lang ? { lang } : {}),
    }),
    [area, search.q, category, tags, lang],
  );
  const load = useCallback(
    (cursor: string | null, limit: number) =>
      api.browseTemplates({
        ...query,
        ...(cursor ? { cursor } : {}),
        ...(limit !== libraryConfig.pageSize ? { limit } : {}),
      }),
    [query],
  );
  const result = useBrowse<TemplateCardData, TemplateFacets>(load, JSON.stringify(query));
  const { retry, refresh, items, update } = result;
  useEffect(() => {
    if (refreshSignal > 0) retry();
  }, [refreshSignal, retry]);

  // While Manna reads one of my worksheets, ask again every few seconds.
  const extracting = items?.some((t) => t.status === "extracting") ?? false;
  useEffect(() => {
    if (!extracting) return undefined;
    const timer = setInterval(refresh, templateConfig.pollMs);
    return () => clearInterval(timer);
  }, [extracting, refresh]);

  const narrowed = search.q !== "" || category !== "" || tags.length > 0 || lang !== null;
  const reset = () => {
    search.clear();
    setCategory("");
    setTags([]);
    setLang(null);
  };
  const facets = result.facets;
  return (
    <BrowseFrame
      searchLabel={area === "platform" ? "搜尋平台範本" : "搜尋我的範本"}
      searchPlaceholder="搜尋範本的標題、區塊或標籤，例如：閱讀理解、三民"
      search={search}
      categories={categoryChips(SAMPLE_CATEGORIES, facets?.categories ?? [], category)}
      allCount={sumOf(facets?.categories)}
      category={category}
      onCategory={setCategory}
      filters={TEMPLATE_FILTERS.map((f) => ({ id: f.id, label: f.label, icon: f.icon, pressed: lang === f.lang }))}
      onFilter={(id) => {
        const f = TEMPLATE_FILTERS.find((x) => x.id === id);
        if (f) setLang((cur) => (cur === f.lang ? null : f.lang));
      }}
      tags={facets?.tags ?? []}
      activeTags={tags}
      onTag={(key) => setTags((list) => toggleIn(list, key))}
      result={result}
      narrowed={narrowed}
      onReset={reset}
      noMatch={NO_TEMPLATES}
      empty={empty}
      listLabel={area === "platform" ? "平台範本" : "我的範本"}
      renderCard={(card) => (
        <TemplateCard
          key={card.id}
          card={card}
          actions={renderActions(card, (next) => update((t) => t.id === next.id, next))}
        />
      )}
    />
  );
}
