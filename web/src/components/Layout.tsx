import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Link, Outlet, useLocation, useOutletContext } from "react-router";
import { useApp } from "../context/AppContext";
import { loadSidebarCollapsed, saveSidebarCollapsed } from "../lib/storage";
import { DemoBanner, Loading } from "./common";
import { Icon } from "./Icon";
import { Sidebar } from "./shell/Sidebar";

const SIDEBAR_ID = "shell-sidebar";

/** Working pages with two columns get the wider frame; the in-class console a narrower one. */
function frameClass(pathname: string): string {
  if (/^\/runs\/\d+\/?$/.test(pathname)) return " is-console";
  if (/^\/(lessons\/\d+|units\/\d+\/core|runs\/\d+\/review|admin|worksheets\/\d+)/.test(pathname)) return " is-wide";
  // 知識庫管理 is a list page: the default frame.
  if (pathname === "/worksheets" || pathname === "/preferences" || pathname === "/prep") return " is-narrow";
  if (pathname === "/") return " is-home";
  if (pathname.startsWith("/create/")) return " is-create";
  if (/^\/prep\/\d+/.test(pathname)) return " is-prep";
  return "";
}

interface ShellContext {
  topSlot: HTMLElement | null;
}

/**
 * Lets a page take over the top bar: what it renders here replaces the navigation (the in-class
 * console shows 「上課中」 and 「結束本節課」, and the sidebar and the menu button step aside, so
 * nothing invites a stray tap during class). Outside the shell, for example in a page rendered on
 * its own, the content stays in the page.
 */
export function TopBarSlot({ children }: { children: ReactNode }) {
  const context = useOutletContext<ShellContext | null>();
  if (context == null) return <div className="top-slot-inline">{children}</div>;
  return context.topSlot ? createPortal(children, context.topSlot) : null;
}

/**
 * A new page starts at the top; an address with #anchor (我的版型 → /materials#templates) scrolls
 * to that section, and keeps it in view while the page above it is still loading, until the
 * teacher scrolls herself.
 */
function useScrollOnNavigate(pathname: string, hash: string) {
  useEffect(() => {
    const id = decodeURIComponent(hash.replace(/^#/, ""));
    if (!id) {
      try {
        window.scrollTo({ top: 0 });
      } catch {
        // jsdom / old browsers
      }
      return undefined;
    }
    let stopped = false;
    const go = () => {
      if (!stopped) document.getElementById(id)?.scrollIntoView?.({ block: "start" });
    };
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(go) : null;
    const stop = () => {
      stopped = true;
      observer?.disconnect();
      window.clearTimeout(timer);
      for (const type of ["wheel", "touchstart", "keydown"] as const) window.removeEventListener(type, stop);
    };
    const timer = window.setTimeout(stop, 2000);
    for (const type of ["wheel", "touchstart", "keydown"] as const) window.addEventListener(type, stop, { passive: true });
    observer?.observe(document.body);
    go();
    return stop;
  }, [pathname, hash]);
}

/**
 * The app shell: a left sidebar (wide screens: 248px, foldable to icons; narrow screens: a drawer
 * opened from ☰ in the top bar) and the page beside it. Every signed-in page renders here.
 */
export function Layout() {
  const { preferencesStatus } = useApp();
  const location = useLocation();
  const { pathname, hash } = location;
  const [topSlot, setTopSlot] = useState<HTMLElement | null>(null);
  const [collapsed, setCollapsed] = useState(loadSidebarCollapsed);
  // The drawer belongs to the location it was opened at: any navigation closes it.
  const [drawerAt, setDrawerAt] = useState<string | null>(null);
  const drawerOpen = drawerAt === location.key;
  const menuRef = useRef<HTMLButtonElement>(null);
  const sideRef = useRef<HTMLDivElement>(null);

  useScrollOnNavigate(pathname, hash);

  // The open drawer: Escape or a tap outside closes it. It does not hold focus in.
  useEffect(() => {
    if (!drawerOpen) return undefined;
    sideRef.current?.querySelector<HTMLElement>("a[href]")?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setDrawerAt(null);
      menuRef.current?.focus();
    };
    const onPointer = (event: Event) => {
      const target = event.target instanceof Node ? event.target : null;
      if (target && (sideRef.current?.contains(target) || menuRef.current?.contains(target))) return;
      setDrawerAt(null);
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onPointer);
    document.addEventListener("touchstart", onPointer);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onPointer);
      document.removeEventListener("touchstart", onPointer);
    };
  }, [drawerOpen]);

  const toggleCollapsed = () => {
    setCollapsed((c) => {
      saveSidebarCollapsed(!c);
      return !c;
    });
  };

  return (
    <>
      <DemoBanner />
      <div className={`shell${collapsed ? " is-collapsed" : ""}${drawerOpen ? " is-drawer-open" : ""}`}>
        <Sidebar
          id={SIDEBAR_ID}
          ref={sideRef}
          collapsed={collapsed}
          onToggleCollapsed={toggleCollapsed}
          onCloseDrawer={() => {
            setDrawerAt(null);
            menuRef.current?.focus();
          }}
        />
        <div className="side-scrim" aria-hidden="true" />

        <div className="shell-main">
          <header className="shell-top">
            <button
              ref={menuRef}
              type="button"
              className="side-icon-button shell-menu"
              aria-label="選單"
              aria-expanded={drawerOpen}
              aria-controls={SIDEBAR_ID}
              onClick={() => setDrawerAt(drawerOpen ? null : location.key)}
            >
              <Icon name="menu" />
            </button>
            <Link className="brand" to="/" aria-label="Manna 首頁">
              <span className="star" aria-hidden="true" />
              Manna
            </Link>
            <div className="top-slot" ref={setTopSlot} />
          </header>

          <main className={`workspace${frameClass(pathname)}`}>
            {preferencesStatus === "loading" ? (
              <Loading label="正在載入你的備課偏好…" />
            ) : (
              <Outlet context={{ topSlot } satisfies ShellContext} />
            )}
          </main>
        </div>
      </div>
    </>
  );
}
