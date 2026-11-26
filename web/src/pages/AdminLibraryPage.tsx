import { useCallback, useEffect, useState, type KeyboardEvent } from "react";
import { Navigate, useSearchParams } from "react-router";
import { MaterialsTab } from "../components/adminLibrary/MaterialsTab";
import { TemplatesTab } from "../components/adminLibrary/TemplatesTab";
import { LeaveConfirm, useLeaveGuard } from "../components/LeaveGuard";
import { useApp } from "../context/AppContext";
import { can } from "../lib/permissions";
import "../components/adminLibrary/adminLibrary.css";
import { useDocumentTitle } from "../lib/useDocumentTitle";

type Tab = "materials" | "templates";

const TABS: Array<{ key: Tab; label: string }> = [
  { key: "materials", label: "知識庫教材" },
  { key: "templates", label: "平台範本" },
];

/**
 * 教材與範本管理 (/admin/library): admins seed 平台教材 (past 學測／統測 papers, the curriculum, the
 * word list, exam guides) and the platform 學習單範本. pi and kb_manager only; others go home.
 */
export function AdminLibraryPage() {
  useDocumentTitle("教材與範本管理");
  const { teacher } = useApp();
  const [params] = useSearchParams();
  // ?tab=templates opens 平台範本. Switching is not a navigation, so it never asks to leave.
  const [tab, setTab] = useState<Tab>(() => (params.get("tab") === "templates" ? "templates" : "materials"));
  // A tab once opened stays mounted (hidden), so an upload in it keeps going when the other is shown.
  const [opened, setOpened] = useState<Set<Tab>>(() => new Set([tab]));
  // While files are on their way, leaving the page (which would stop the queue) asks first.
  const [busy, setBusy] = useState<Record<Tab, boolean>>({ materials: false, templates: false });
  const onMaterialsBusy = useCallback(
    (b: boolean) => setBusy((s) => (s.materials === b ? s : { ...s, materials: b })),
    [],
  );
  const onTemplatesBusy = useCallback(
    (b: boolean) => setBusy((s) => (s.templates === b ? s : { ...s, templates: b })),
    [],
  );
  const guard = useLeaveGuard(busy.materials || busy.templates);
  // A file dropped beside the drop zone would make the browser open it, leaving the page.
  useEffect(() => {
    const hold = (e: globalThis.DragEvent) => {
      if (e.dataTransfer?.types.includes("Files")) e.preventDefault();
    };
    window.addEventListener("dragover", hold);
    window.addEventListener("drop", hold);
    return () => {
      window.removeEventListener("dragover", hold);
      window.removeEventListener("drop", hold);
    };
  }, []);

  if (!can(teacher, "manage_knowledge")) return <Navigate to="/" replace />;

  function choose(next: Tab) {
    setTab(next);
    if (!opened.has(next)) setOpened(new Set([...opened, next]));
  }

  function onKeyDown(e: KeyboardEvent<HTMLButtonElement>) {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    e.preventDefault();
    const next = tab === "materials" ? "templates" : "materials";
    choose(next);
    document.getElementById(`adlib-tab-${next}`)?.focus();
  }

  return (
    <section aria-labelledby="adlib-heading" className="adlib-page">
      <div className="page-head">
        <h1 id="adlib-heading">教材與範本管理</h1>
        <p className="lead">
          放進平台給所有老師參考的教材（歷屆學測、統測英文試題、課綱、詞彙表、考試說明）與學習單範本。老師都看得到、可以選用；只有計畫主持人與知識庫管理員能修改或刪除。
        </p>
        <p className="quiet-note">請不要上傳學生作業、學生姓名或成績；上傳前請確認這份內容可以提供給所有老師使用（著作權）。</p>
      </div>

      <div className="adlib-tabs" role="tablist" aria-label="教材與範本">
        {TABS.map((t) => (
          <button
            key={t.key}
            id={`adlib-tab-${t.key}`}
            type="button"
            role="tab"
            aria-selected={tab === t.key}
            aria-controls={`adlib-panel-${t.key}`}
            tabIndex={tab === t.key ? 0 : -1}
            className={tab === t.key ? "is-current" : ""}
            onClick={() => choose(t.key)}
            onKeyDown={onKeyDown}
          >
            {t.label}
          </button>
        ))}
      </div>

      {TABS.map((t) => (
        <div
          key={t.key}
          id={`adlib-panel-${t.key}`}
          role="tabpanel"
          aria-labelledby={`adlib-tab-${t.key}`}
          hidden={tab !== t.key}
          className="adlib-panel"
        >
          {opened.has(t.key) &&
            (t.key === "materials" ? (
              <MaterialsTab onBusy={onMaterialsBusy} />
            ) : (
              <TemplatesTab onBusy={onTemplatesBusy} />
            ))}
        </div>
      ))}
      <LeaveConfirm guard={guard} message="還有檔案正在上傳或排隊。離開這頁，排隊中的檔案就不會上傳。" />
    </section>
  );
}
