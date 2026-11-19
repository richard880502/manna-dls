import { useEffect, useState, type ReactNode, type Ref } from "react";
import { Link, useLocation } from "react-router";
import { api } from "../../api/client";
import type { PrepSessionSummary } from "../../api/types";
import { useApp } from "../../context/AppContext";
import { can } from "../../lib/permissions";
import { PREP_SESSIONS_CHANGED, sessionTitle } from "../../lib/prep";
import { Icon, type IconName } from "../Icon";

/** How many of the latest prep sessions the sidebar lists. */
export const RECENT_IN_SIDEBAR = 3;

/**
 * Where a link stands relative to the page: "page" on the page itself, "true" on a page inside it
 * (a session inside 我的作品, a unit inside 班級與課例), otherwise nothing.
 */
type Current = "page" | "true" | undefined;

const inside = (exact: boolean, within: boolean): Current => (exact ? "page" : within ? "true" : undefined);

/** The pages inside 更多 (班級與課例, 上課紀錄). */
const inMore = (p: string) =>
  p.startsWith("/classes") ||
  p.startsWith("/units") ||
  p.startsWith("/lessons") ||
  p === "/record" ||
  p.startsWith("/runs/");

/**
 * The teacher's latest prep sessions. Read again when a prep page opens (one may be new) and
 * when one is deleted (PREP_SESSIONS_CHANGED: 我的作品 deletes without leaving its page).
 */
function useRecentSessions(pathname: string): PrepSessionSummary[] | null {
  const [sessions, setSessions] = useState<PrepSessionSummary[] | null>(null);
  const [changes, setChanges] = useState(0);
  const key = pathname.startsWith("/prep") ? pathname : "elsewhere";
  useEffect(() => {
    const changed = () => setChanges((n) => n + 1);
    window.addEventListener(PREP_SESSIONS_CHANGED, changed);
    return () => window.removeEventListener(PREP_SESSIONS_CHANGED, changed);
  }, []);
  useEffect(() => {
    let alive = true;
    api
      .listPrepSessions()
      .then((res) => alive && setSessions(res.sessions.slice(0, RECENT_IN_SIDEBAR)))
      // The list is a convenience: without it the shell still works.
      .catch(() => alive && setSessions((s) => s ?? []));
    return () => {
      alive = false;
    };
  }, [key, changes]);
  return sessions;
}

function SideLink({
  to,
  icon,
  label,
  current,
}: {
  to: string;
  icon: IconName;
  label: string;
  current: Current;
}) {
  return (
    <li>
      <Link className="side-link" to={to} aria-current={current} title={label}>
        <Icon name={icon} />
        <span className="side-label">{label}</span>
      </Link>
    </li>
  );
}

/** A group that folds: 更多 (folded unless the page is inside it). */
function SideGroup({
  id,
  icon,
  label,
  open,
  onToggle,
  children,
}: {
  id: string;
  icon: IconName;
  label: string;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <li className="side-group">
      <button
        type="button"
        className="side-link side-group-toggle"
        aria-expanded={open}
        aria-controls={id}
        title={label}
        onClick={onToggle}
      >
        <Icon name={icon} />
        <span className="side-label">{label}</span>
        <Icon name="chevron" className="side-chevron" />
      </button>
      <ul id={id} className="side-sublist" hidden={!open}>
        {children}
      </ul>
    </li>
  );
}

export function Sidebar({
  id,
  ref,
  collapsed,
  onToggleCollapsed,
  onCloseDrawer,
}: {
  id: string;
  ref?: Ref<HTMLDivElement>;
  /** Folded to icons (wide screens only). */
  collapsed: boolean;
  onToggleCollapsed: () => void;
  /** Closes the drawer (narrow screens only). */
  onCloseDrawer: () => void;
}) {
  const { teacher, logout } = useApp();
  const { pathname } = useLocation();
  const sessions = useRecentSessions(pathname);
  const [moreOpen, setMoreOpen] = useState(() => inMore(pathname));
  // Entering a page inside 更多 opens it, so the current page is visible in the list.
  const [seenPath, setSeenPath] = useState(pathname);
  if (seenPath !== pathname) {
    setSeenPath(pathname);
    if (inMore(pathname) && !moreOpen) setMoreOpen(true);
  }

  return (
    <div className="sidebar" id={id} ref={ref}>
      <div className="side-head">
        <Link className="brand" to="/" aria-label="Manna 首頁">
          <span className="star" aria-hidden="true" />
          <span className="side-label">Manna</span>
        </Link>
        <button
          type="button"
          className="side-icon-button side-collapse"
          aria-label="側邊欄"
          aria-expanded={!collapsed}
          aria-controls={id}
          title={collapsed ? "展開側邊欄" : "收合側邊欄"}
          onClick={onToggleCollapsed}
        >
          <Icon name="collapse" />
        </button>
        <button type="button" className="side-icon-button side-close" aria-label="關閉選單" onClick={onCloseDrawer}>
          <Icon name="close" />
        </button>
      </div>

      <nav className="side-nav" aria-label="主要導覽">
        <ul className="side-list">
          {/* 建立 starts on the home page's tiles, so 首頁 is the current item on /create/*. */}
          <SideLink
            to="/"
            icon="home"
            label="首頁"
            current={inside(pathname === "/", pathname.startsWith("/create/"))}
          />
          <SideLink
            to="/prep"
            icon="workspace"
            label="我的作品"
            current={inside(pathname === "/prep", pathname.startsWith("/prep/") || pathname.startsWith("/worksheets"))}
          />
        </ul>

        {sessions && sessions.length > 0 && (
          <section className="side-recent" aria-labelledby={`${id}-recent`}>
            <h2 className="side-heading" id={`${id}-recent`}>
              最近
            </h2>
            <ul className="side-list">
              {sessions.map((s) => (
                <li key={s.id}>
                  <Link
                    className="side-link is-recent"
                    to={`/prep/${s.id}`}
                    aria-current={pathname === `/prep/${s.id}` ? "page" : undefined}
                    title={sessionTitle(s)}
                  >
                    <span className="side-label">{sessionTitle(s)}</span>
                  </Link>
                </li>
              ))}
            </ul>
          </section>
        )}

        {/* A small label, not a heading: the page's own headings stay the page's. */}
        <div className="side-section">
          <p className="side-heading side-label" id={`${id}-resources`}>
            教材與範本
          </p>
          <ul className="side-list" aria-labelledby={`${id}-resources`}>
            <SideLink to="/materials" icon="library" label="我的教材" current={inside(pathname === "/materials", false)} />
            <SideLink to="/reference" icon="reference" label="平台教材" current={inside(pathname === "/reference", false)} />
            <SideLink
              to="/templates"
              icon="template"
              label="學習單範本"
              current={inside(pathname === "/templates", pathname.startsWith("/templates/"))}
            />
          </ul>
        </div>

        <ul className="side-list">
          <SideGroup id={`${id}-more`} icon="more" label="更多" open={moreOpen} onToggle={() => setMoreOpen((o) => !o)}>
            <SideLink
              to="/classes"
              icon="classes"
              label="班級與課例"
              current={inside(
                pathname === "/classes",
                pathname.startsWith("/classes/") || pathname.startsWith("/units") || pathname.startsWith("/lessons"),
              )}
            />
            <SideLink
              to="/record"
              icon="record"
              label="上課紀錄"
              current={inside(pathname === "/record", pathname.startsWith("/runs/"))}
            />
          </SideGroup>
        </ul>
      </nav>

      <div className="side-foot">
        <ul className="side-list">
          <SideLink to="/preferences" icon="prefs" label="備課偏好" current={inside(pathname === "/preferences", false)} />
          {can(teacher, "manage_knowledge") && (
            <SideLink to="/knowledge" icon="knowledge" label="知識庫" current={inside(pathname === "/knowledge", false)} />
          )}
          {can(teacher, "manage_knowledge") && (
            <SideLink
              to="/admin/library"
              icon="library"
              label="教材與範本管理"
              current={inside(pathname === "/admin/library", false)}
            />
          )}
          {can(teacher, "administer") && (
            <SideLink to="/admin" icon="admin" label="後台管理" current={inside(pathname === "/admin", false)} />
          )}
        </ul>
        <div className="side-user">
          {teacher && (
            <p className="side-user-id" title={teacher.research_id}>
              <span className="side-avatar" aria-hidden="true">
                {teacher.research_id.slice(0, 1)}
              </span>
              <span className="side-label">{teacher.research_id}</span>
            </p>
          )}
          <button type="button" className="side-link side-logout" title="登出" onClick={() => void logout()}>
            <Icon name="logout" />
            <span className="side-label">登出</span>
          </button>
        </div>
      </div>
    </div>
  );
}
