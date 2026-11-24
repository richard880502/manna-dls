import { useId, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { api } from "../api/client";
import { AccountRoles } from "../components/admin/AccountRoles";
import { AdminOverview } from "../components/admin/AdminOverview";
import { AiModelSettings } from "../components/admin/AiModelSettings";
import { CommunitySettings } from "../components/admin/CommunitySettings";
import { CsvImport } from "../components/admin/CsvImport";
import { InviteForm } from "../components/admin/InviteForm";
import { MemorySettings } from "../components/admin/MemorySettings";
import { PlatformSettings } from "../components/admin/PlatformSettings";
import { PromptsEditor } from "../components/admin/PromptsEditor";
import { KB_MANAGER_NOTE } from "../components/admin/roles";
import { ErrorText } from "../components/common";
import { Tabs } from "../components/library/Tabs";
import { useApp } from "../context/AppContext";
import { can } from "../lib/permissions";

type AdminTab = "overview" | "accounts" | "settings" | "prompts" | "import";
const ADMIN_TABS: ReadonlyArray<readonly [AdminTab, string]> = [
  ["overview", "總覽"],
  ["accounts", "帳號"],
  ["settings", "模型與研究設定"],
  ["prompts", "提示詞"],
  ["import", "匯入"],
];

function tabFromParam(value: string | null): AdminTab {
  return ADMIN_TABS.find(([key]) => key === value)?.[0] ?? "overview";
}

/** 後台管理 (/admin, pi): tabs kept in `?tab=` — 總覽 (no param), accounts, settings, prompts, import. */
export function AdminPage() {
  const { teacher } = useApp();
  const ids = useId();
  const [params, setParams] = useSearchParams();
  const [modelRevision, setModelRevision] = useState(0);
  if (!can(teacher, "administer")) {
    return <ErrorText message="這個頁面只開放給計畫主持人。" />;
  }
  const tab = tabFromParam(params.get("tab"));
  return (
    <section className="admin-page">
      <div className="page-intro">
        <h1>後台管理</h1>
        <p>
          管理帳號、模型與研究設定、提示詞和備課經驗。課綱條目與詞彙表在
          <Link to="/knowledge?tab=curriculum">知識庫</Link>。
        </p>
      </div>
      <Tabs
        label="後台管理的內容"
        tabs={ADMIN_TABS}
        current={tab}
        panelId={`${ids}-panel`}
        onChange={(next) => setParams(next === "overview" ? {} : { tab: next }, { replace: true })}
      />
      <div id={`${ids}-panel`} role="tabpanel" className="library-tabpanel">
        {tab === "overview" && <AdminOverview />}
        {tab === "accounts" && (
          <>
            <details className="admin-roles-help">
              <summary>各角色能做什麼</summary>
              <p>
                「主持人」可以使用全部後台功能。{KB_MANAGER_NOTE}
              </p>
              <p>角色變更立即生效，對方不必重新登入。</p>
            </details>
            <AccountRoles />
            <InviteForm />
          </>
        )}
        {tab === "settings" && (
          <>
            <AiModelSettings onChanged={() => setModelRevision((value) => value + 1)} />
            <MemorySettings refreshVersion={modelRevision} />
            <CommunitySettings />
            <PlatformSettings />
          </>
        )}
        {tab === "prompts" && <PromptsEditor />}
        {tab === "import" && (
          <CsvImport
            title="匯入教師備課經驗"
            columns="case_code,school_track,grade,topic,summary,details,approved"
            inputId="cases-csv"
            upload={api.admin.importTeacherCases}
          />
        )}
      </div>
    </section>
  );
}
