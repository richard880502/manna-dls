import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router";
import { api, errorMessage } from "../../api/client";
import type { CommunityMine, CommunityShareState } from "../../api/types";
import { formatDate } from "../../lib/labels";
import { ErrorText, Loading } from "../common";
import { Counts } from "./Counts";
import { Cover } from "./Cover";
import { ANONYMOUS_NAME, SHARE_NAME_LABEL, outputLabel } from "./labels";
import { ShareDialog } from "./ShareDialog";

/** 我的分享: every worksheet of the teacher's with a plain 不公開／已分享 switch. */
export function MineTab() {
  const [items, setItems] = useState<CommunityMine[] | null>(null);
  const [total, setTotal] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [dialog, setDialog] = useState<CommunityMine | null>(null);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);

  useEffect(() => {
    let alive = true;
    api.community
      .mine()
      .then((page) => {
        if (!alive) return;
        setItems(page.items);
        setTotal(page.total);
      })
      .catch((err: unknown) => {
        if (!alive) return;
        setItems([]);
        setError(errorMessage(err));
      });
    return () => {
      alive = false;
    };
  }, []);

  const loadMore = useCallback(async () => {
    if (items === null) return;
    setLoadingMore(true);
    try {
      const page = await api.community.mine({ offset: items.length });
      setItems((list) => [...(list ?? []), ...page.items]);
      setTotal(page.total);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoadingMore(false);
    }
  }, [items]);

  const applied = (state: CommunityShareState) =>
    setItems((list) =>
      (list ?? []).map((i) =>
        i.id === state.id
          ? { ...i, shared: state.shared, shared_at: state.shared_at, name: state.name, allow_remix: state.allow_remix }
          : i,
      ),
    );

  async function unshare(item: CommunityMine) {
    setBusyId(item.id);
    setError(null);
    try {
      applied(await api.community.share(item.id, { shared: false }));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div className="com-mine">
      <p className="lead">
        分享是你自己決定的，預設都不公開。打開開關前會先告訴你別人看得到什麼，也隨時可以撤回。
      </p>
      <ErrorText message={error} />
      {items === null && <Loading />}
      {items !== null && items.length === 0 && !error && (
        <div className="com-empty">
          <p>
            還沒有可以分享的學習單。先<Link to="/">做一份學習單</Link>，產生好之後再回來分享。
          </p>
        </div>
      )}
      {items !== null && items.length > 0 && (
        <ul className="com-rows" aria-label="我的學習單">
          {items.map((item) => {
            const kind = outputLabel(item.output);
            return (
              <li key={item.id} className="com-row">
                <div className="com-row-cover">
                  <Cover sheet={item.cover} tint={item.id} />
                </div>
                <div className="com-row-main">
                  <h3 className="com-row-title">
                    <Link to={`/worksheets/${item.id}`}>{item.title || "未命名的學習單"}</Link>
                  </h3>
                  <p className="com-row-meta">
                    {kind && <span className="soft-tag">{kind}</span>} 修改於 {formatDate(item.updated_at)}
                  </p>
                  {item.shared && (
                    <>
                      <p className="com-row-meta">
                        名稱顯示為「{item.name === "code" ? SHARE_NAME_LABEL.code : ANONYMOUS_NAME}」，
                        {item.allow_remix ? "允許改編" : "不開放改編"}
                      </p>
                      <Counts saves={item.saves} remixes={item.remixes} />
                    </>
                  )}
                </div>
                <div className="com-row-switch">
                  <button
                    type="button"
                    role="switch"
                    className="com-switch"
                    aria-checked={item.shared}
                    aria-label={`分享：${item.title || "未命名的學習單"}`}
                    disabled={busyId === item.id}
                    onClick={() => (item.shared ? void unshare(item) : setDialog(item))}
                  >
                    <span className="com-switch-track" aria-hidden="true" />
                    <span className="com-switch-text">{item.shared ? "已分享" : "不公開"}</span>
                  </button>
                  {item.shared && (
                    <button type="button" className="text-link is-plain" onClick={() => setDialog(item)}>
                      分享設定
                    </button>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}
      {items !== null && items.length < total && (
        <p className="lib-more">
          <button type="button" className="secondary-button" disabled={loadingMore} onClick={() => void loadMore()}>
            {loadingMore ? "載入中…" : "顯示更多"}
          </button>
        </p>
      )}
      {dialog && (
        <ShareDialog
          item={dialog}
          onClose={() => setDialog(null)}
          onShared={(state) => {
            applied(state);
            setDialog(null);
          }}
        />
      )}
    </div>
  );
}
