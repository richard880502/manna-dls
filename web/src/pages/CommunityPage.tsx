import { useId } from "react";
import { Navigate, useSearchParams } from "react-router";
import { Loading } from "../components/common";
import { BrowseTab } from "../components/community/BrowseTab";
import { COMMUNITY_TABS, type CommunityTab } from "../components/community/labels";
import { MineTab } from "../components/community/MineTab";
import { Tabs } from "../components/library/Tabs";
import { useApp } from "../context/AppContext";
import "../styles/community.css";
import { useDocumentTitle } from "../lib/useDocumentTitle";

const tabFromParam = (value: string | null): CommunityTab =>
  COMMUNITY_TABS.find(([key]) => key === value)?.[0] ?? "explore";

/**
 * 社群 (/community, only while the PI's switch is on): 探索, 我的收藏 and 我的分享 as tabs kept in
 * `?tab=`. With the switch off the page does not exist for teachers: it sends them home.
 */
export function CommunityPage() {
  useDocumentTitle("社群");
  const { config, communityEnabled } = useApp();
  const ids = useId();
  const [params, setParams] = useSearchParams();
  if (config === null) return <Loading />;
  if (!communityEnabled) return <Navigate to="/" replace />;
  const tab = tabFromParam(params.get("tab"));
  return (
    <section className="com-page" aria-labelledby={`${ids}-h`}>
      <div className="page-intro">
        <h1 id={`${ids}-h`}>社群</h1>
        <p>看看其他老師分享的學習單，收藏起來，或拿來改成自己的版本。你的學習單預設不公開，要不要分享由你決定。</p>
      </div>
      <Tabs
        label="社群的內容"
        tabs={COMMUNITY_TABS}
        current={tab}
        panelId={`${ids}-panel`}
        onChange={(next) => setParams(next === "explore" ? {} : { tab: next }, { replace: true })}
      />
      <div id={`${ids}-panel`} role="tabpanel" className="library-tabpanel">
        <h2 className="sr-only">{COMMUNITY_TABS.find(([key]) => key === tab)?.[1]}</h2>
        {tab === "explore" && <BrowseTab kind="explore" />}
        {tab === "saved" && <BrowseTab kind="saved" />}
        {tab === "mine" && <MineTab />}
      </div>
    </section>
  );
}
