import { useId, useState } from "react";
import type { CommunityCard, CommunityMeta, CommunitySort } from "../../api/types";
import { LEVEL_LABEL } from "../../lib/worksheet";
import { ErrorText, Loading } from "../common";
import { SearchField } from "../library/BrowsePanel";
import { useSearchText } from "../library/useBrowse";
import { Card } from "./Card";
import { OUTPUT_LABEL } from "./labels";
import { PreviewOverlay } from "./Preview";
import { useCardActions } from "./useCardActions";
import { useCommunityList } from "./useCommunityList";

/**
 * 探索 (every shared worksheet, with search, sort and the filters the worksheet data has) and
 * 我的收藏 (what the teacher saved; no sort, saved newest first). Same tiles and preview.
 */
export function BrowseTab({ kind }: { kind: "explore" | "saved" }) {
  const ids = useId();
  const search = useSearchText();
  const [sort, setSort] = useState<CommunitySort>("newest");
  const [output, setOutput] = useState("");
  const [level, setLevel] = useState("");
  const list = useCommunityList(kind, {
    q: search.q || undefined,
    sort: kind === "explore" ? sort : undefined,
    output: output || undefined,
    level: level || undefined,
  });
  const [open, setOpen] = useState<CommunityMeta | null>(null);
  const actions = useCardActions((id, change) => {
    list.patch(id, change);
    setOpen((o) => (o === null || o.id !== id ? o : change === null ? null : { ...o, ...change }));
  });
  const narrowed = search.q !== "" || output !== "" || level !== "";
  const items = list.items;

  return (
    <div className="com-browse">
      <div className="com-bar">
        <SearchField
          label={kind === "explore" ? "搜尋分享的學習單" : "搜尋我的收藏"}
          placeholder="用標題搜尋"
          search={search}
        />
        <div className="com-filters">
          {kind === "explore" && (
            <label className="lib-sort" htmlFor={`${ids}-sort`}>
              排序
              <select id={`${ids}-sort`} value={sort} onChange={(e) => setSort(e.target.value as CommunitySort)}>
                <option value="newest">最新</option>
                <option value="remixed">最多人改編</option>
                <option value="saved">最多人收藏</option>
              </select>
            </label>
          )}
          <label className="lib-sort" htmlFor={`${ids}-output`}>
            類型
            <select id={`${ids}-output`} value={output} onChange={(e) => setOutput(e.target.value)}>
              <option value="">全部</option>
              {Object.entries(OUTPUT_LABEL).map(([key, label]) => (
                <option key={key} value={key}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label className="lib-sort" htmlFor={`${ids}-level`}>
            版本
            <select id={`${ids}-level`} value={level} onChange={(e) => setLevel(e.target.value)}>
              <option value="">全部</option>
              {Object.entries(LEVEL_LABEL).map(([key, label]) => (
                <option key={key} value={key}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        </div>
      </div>

      <ErrorText message={actions.error} />
      <ErrorText message={list.error} />
      {list.error && (
        <button type="button" className="text-link is-plain" onClick={list.retry}>
          再試一次
        </button>
      )}
      {items === null && <Loading />}
      {items !== null && (
        <p className="lib-count" role="status">
          {items.length === 0 ? "" : `共 ${list.total} 份`}
        </p>
      )}
      {items !== null && items.length === 0 && !list.error && (
        <div className="com-empty">
          {narrowed ? (
            <p>沒有符合的學習單。換個搜尋字或篩選看看。</p>
          ) : kind === "explore" ? (
            <p>還沒有人分享學習單。到「我的分享」把你的學習單分享出來，就會出現在這裡。</p>
          ) : (
            <p>還沒有收藏。在「探索」看到喜歡的學習單，按「收藏」就會放在這裡。</p>
          )}
        </div>
      )}
      {kind === "saved" && list.unavailable > 0 && (
        <p className="field-hint">有 {list.unavailable} 份收藏的學習單已經取消分享，暫時看不到。</p>
      )}
      {items !== null && items.length > 0 && (
        <ul className="com-grid" aria-label={kind === "explore" ? "分享的學習單" : "我的收藏"}>
          {items.map((card: CommunityCard) => (
            <Card
              key={card.id}
              card={card}
              busy={actions.busyId === card.id}
              onOpen={setOpen}
              onToggleSave={(c) => void actions.toggleSave(c)}
              onRemix={(c) => void actions.remix(c)}
            />
          ))}
        </ul>
      )}
      {items !== null && items.length < list.total && (
        <p className="lib-more">
          <button type="button" className="secondary-button" disabled={list.loadingMore} onClick={() => void list.loadMore()}>
            {list.loadingMore ? "載入中…" : "顯示更多"}
          </button>
        </p>
      )}
      {open && (
        <PreviewOverlay
          card={open}
          busy={actions.busyId === open.id}
          onClose={() => setOpen(null)}
          onToggleSave={(c) => void actions.toggleSave(c)}
          onRemix={(c) => void actions.remix(c)}
        />
      )}
    </div>
  );
}
