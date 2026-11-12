import { useEffect, useState } from "react";
import { NavLink, Outlet, useLocation } from "react-router";
import { useApp } from "../context/AppContext";
import { ROLE_LABELS, SCHOOL_TRACK_LABELS } from "../lib/labels";
import { DemoBanner, Loading } from "./common";
import { OnboardingWizard } from "./prefs/OnboardingWizard";

interface NavEntry {
  to: string;
  label: string;
  symbol: string;
  end?: boolean;
}

const NAV: NavEntry[] = [
  { to: "/", label: "工作檯", symbol: "⌂", end: true },
  { to: "/plan", label: "Study & Plan", symbol: "▤" },
  { to: "/record", label: "Teach & Record", symbol: "▣" },
  { to: "/reflect", label: "Reflect & Progress", symbol: "⌁" },
  { to: "/share", label: "Share & Connect", symbol: "♧" },
  { to: "/profile", label: "履歷頁", symbol: "▥" },
  { to: "/community", label: "社群動能地圖", symbol: "⌖" },
  { to: "/preferences", label: "我的備課偏好", symbol: "✎" },
];

function viewTitle(pathname: string): { eyebrow: string; title: string } {
  if (pathname === "/") return { eyebrow: "Manna DLS · Phase 1 AI 備課", title: "教師工作檯" };
  if (pathname === "/plan") return { eyebrow: "01 · Study & Plan", title: "Study & Plan｜單元與課例" };
  if (/^\/units\/\d+\/core/.test(pathname)) return { eyebrow: "01 · Study & Plan", title: "單元前 CoRe" };
  if (/^\/units\/\d+/.test(pathname)) return { eyebrow: "01 · Study & Plan", title: "單元與課例" };
  if (/^\/lessons\/\d+/.test(pathname)) return { eyebrow: "01 · Study & Plan", title: "課例備課" };
  if (pathname === "/record") return { eyebrow: "02 · Teach & Record", title: "Teach & Record｜課堂事件紀錄" };
  if (pathname === "/reflect") return { eyebrow: "03 · Reflect & Progress", title: "Reflect & Progress｜精彩片段回顧" };
  if (pathname === "/share") return { eyebrow: "04 · Share & Connect", title: "Share & Connect｜我不孤單" };
  if (pathname === "/profile") return { eyebrow: "Professional Profile", title: "教師專業履歷" };
  if (pathname === "/community") return { eyebrow: "Community pulse", title: "社群動能地圖" };
  if (pathname === "/admin") return { eyebrow: "主持人後台", title: "後台管理" };
  if (pathname === "/preferences") return { eyebrow: "Settings", title: "我的備課偏好" };
  return { eyebrow: "Manna DLS", title: "Manna DLS" };
}

export function Layout() {
  const { teacher, logout, config, preferences, preferencesStatus } = useApp();
  const location = useLocation();
  const [menuOpen, setMenuOpen] = useState(false);
  const [onboardingDismissed, setOnboardingDismissed] = useState(false);
  const needsOnboarding = preferencesStatus === "loaded" && preferences === null && !onboardingDismissed;
  const { eyebrow, title } = needsOnboarding
    ? { eyebrow: "Manna DLS · 第一次使用", title: "初次使用設定" }
    : viewTitle(location.pathname);

  useEffect(() => {
    try {
      window.scrollTo({ top: 0 });
    } catch {
      // jsdom / old browsers
    }
  }, [location.pathname]);

  const nav = teacher?.role === "pi" ? [...NAV, { to: "/admin", label: "後台管理", symbol: "⚙" }] : NAV;

  return (
    <>
      <DemoBanner />
      <div className="app-shell">
        <aside className={`sidebar${menuOpen ? " is-open" : ""}`} aria-label="主要導覽">
          <div className="sidebar-top">
            <div className="brand">
              <div className="brand-mark star-mascot" aria-label="Manna 星星娃娃">
                ★
              </div>
              <div>
                <div className="brand-name">Manna DLS</div>
                <div className="brand-sub">AI 賦能英文教師專業學習</div>
              </div>
            </div>
            <button
              type="button"
              className="menu-toggle"
              aria-expanded={menuOpen}
              aria-controls="main-nav"
              onClick={() => setMenuOpen((v) => !v)}
            >
              {menuOpen ? "收合選單" : "選單"}
            </button>
          </div>

          {teacher && (
            <div className="teacher-card">
              <div className="avatar" aria-hidden="true">
                {teacher.research_id.slice(0, 1).toUpperCase()}
              </div>
              <div>
                <strong>{teacher.research_id}</strong>
                <span>
                  {SCHOOL_TRACK_LABELS[teacher.school_track]}英文{ROLE_LABELS[teacher.role]}
                </span>
              </div>
              <span className="privacy-dot" title="資料治理已啟用">
                ●
              </span>
            </div>
          )}

          <nav className="nav-list" id="main-nav" aria-label="DLS 工作流程">
            {nav.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                end={item.end}
                onClick={() => setMenuOpen(false)}
                className={({ isActive }) => `nav-item${isActive ? " is-active" : ""}`}
              >
                <span className="nav-symbol" aria-hidden="true">
                  {item.symbol}
                </span>
                {item.label}
              </NavLink>
            ))}
          </nav>

          <section className="side-panel" aria-label="Phase 1 範圍">
            <div className="panel-label">本輪平台範圍</div>
            <div className="cycle-title">Phase 1：AI 備課</div>
            <div className="stage-mini">
              <span className="stage-item done">
                <b>●</b>
                <span>單元前 CoRe</span>
              </span>
              <span className="stage-item done">
                <b>●</b>
                <span>課例備課（RAG、SIOP）</span>
              </span>
              <span className="stage-item">
                <b>—</b>
                <span>課中書籤、反思（Phase 2）</span>
              </span>
            </div>
            {config && !config.ai_enabled && <p className="side-note">AI 功能目前未開放</p>}
          </section>

          <button type="button" className="logout-button" onClick={() => void logout()}>
            登出
          </button>
        </aside>

        <main className="workspace">
          <header className="topbar">
            <div>
              <div className="eyebrow">{eyebrow}</div>
              <h1>{title}</h1>
            </div>
          </header>
          {preferencesStatus === "loading" ? (
            <Loading label="正在載入你的備課偏好…" />
          ) : needsOnboarding ? (
            <OnboardingWizard onDismiss={() => setOnboardingDismissed(true)} />
          ) : (
            <Outlet />
          )}
        </main>
      </div>
    </>
  );
}
