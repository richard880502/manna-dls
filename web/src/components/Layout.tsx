import { useEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Link, Outlet, useLocation, useOutletContext } from "react-router";
import { useApp } from "../context/AppContext";
import { can } from "../lib/permissions";
import { DemoBanner, Loading } from "./common";

interface NavEntry {
  to: string;
  label: string;
  /** Which paths count as being inside this destination. */
  match: (pathname: string) => boolean;
}

/**
 * The whole navigation: three destinations, nothing unbuilt. The teacher's path runs through
 * 班級 (class → unit → lesson → worksheet), so units, lessons, runs and the list of all units
 * count as being inside 班級; materials have their own place under 教材.
 */
export const NAV: NavEntry[] = [
  {
    to: "/",
    label: "班級",
    match: (p) =>
      p === "/" ||
      p.startsWith("/classes") ||
      p.startsWith("/units") ||
      p.startsWith("/lessons") ||
      p.startsWith("/runs") ||
      p === "/record",
  },
  { to: "/worksheets", label: "學習單", match: (p) => p.startsWith("/worksheets") },
  // 我的學習單版型 live on the 教材 page; a template's own page counts as being there.
  { to: "/materials", label: "教材", match: (p) => p.startsWith("/materials") || p.startsWith("/templates") },
];

/** Working pages with two columns get the wider frame; the in-class console a narrower one. */
function frameClass(pathname: string): string {
  if (/^\/runs\/\d+\/?$/.test(pathname)) return " is-console";
  if (/^\/(lessons\/\d+|units\/\d+\/core|runs\/\d+\/review|admin|worksheets\/\d+)/.test(pathname)) return " is-wide";
  // 知識庫管理 is a list page: the default frame.
  if (pathname === "/worksheets" || pathname === "/preferences") return " is-narrow";
  return "";
}

interface ShellContext {
  topSlot: HTMLElement | null;
}

/**
 * Lets a page take over the top bar: what it renders here replaces the navigation and the account
 * links (the in-class console shows 「上課中」 and 「結束本節課」 instead). Outside the shell, for
 * example in a page rendered on its own, the content stays in the page.
 */
export function TopBarSlot({ children }: { children: ReactNode }) {
  const context = useOutletContext<ShellContext | null>();
  if (context == null) return <div className="top-slot-inline">{children}</div>;
  return context.topSlot ? createPortal(children, context.topSlot) : null;
}

export function Layout() {
  const { teacher, logout, preferencesStatus } = useApp();
  const { pathname } = useLocation();
  const [topSlot, setTopSlot] = useState<HTMLElement | null>(null);

  useEffect(() => {
    try {
      window.scrollTo({ top: 0 });
    } catch {
      // jsdom / old browsers
    }
  }, [pathname]);

  const current = (active: boolean) => (active ? ("page" as const) : undefined);

  return (
    <>
      <DemoBanner />
      <header className="top">
        <Link className="brand" to="/" aria-label="Manna 首頁">
          <span className="star" aria-hidden="true" />
          Manna
        </Link>
        <div className="top-slot" ref={setTopSlot} />
        <nav className="top-nav" aria-label="主要導覽">
          {NAV.map((item) => (
            <Link key={item.to} to={item.to} aria-current={current(item.match(pathname))}>
              {item.label}
            </Link>
          ))}
        </nav>
        <div className="acct">
          {can(teacher, "manage_knowledge") && (
            <Link to="/knowledge" aria-current={current(pathname === "/knowledge")}>
              知識庫管理
            </Link>
          )}
          {can(teacher, "administer") && (
            <Link to="/admin" aria-current={current(pathname === "/admin")}>
              後台管理
            </Link>
          )}
          <Link to="/preferences" aria-current={current(pathname === "/preferences")}>
            {teacher && <span className="acct-id">{teacher.research_id} · </span>}
            備課偏好
          </Link>
          <button type="button" className="acct-logout" onClick={() => void logout()}>
            登出
          </button>
        </div>
      </header>

      <main className={`workspace${frameClass(pathname)}`}>
        {preferencesStatus === "loading" ? (
          <Loading label="正在載入你的備課偏好…" />
        ) : (
          <Outlet context={{ topSlot } satisfies ShellContext} />
        )}
      </main>
    </>
  );
}
