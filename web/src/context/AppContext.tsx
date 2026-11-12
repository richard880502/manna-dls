import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { ApiError, api, setUnauthorizedHandler } from "../api/client";
import type { AppConfig, Teacher } from "../api/types";
import { clearToken, loadToken, saveToken } from "../lib/storage";

interface AppContextValue {
  config: AppConfig | null;
  /** AI is assumed available until /api/config says otherwise; the server still answers "disabled" if not. */
  aiEnabled: boolean;
  token: string | null;
  teacher: Teacher | null;
  authChecked: boolean;
  authError: string | null;
  sessionNotice: string | null;
  login: (code: string) => Promise<void>;
  logout: () => Promise<void>;
  retryAuth: () => void;
}

const AppContext = createContext<AppContextValue | null>(null);

export function AppProvider({ children }: { children: ReactNode }) {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [token, setToken] = useState<string | null>(() => loadToken());
  const [teacher, setTeacher] = useState<Teacher | null>(null);
  const [authChecked, setAuthChecked] = useState<boolean>(() => !loadToken());
  const [authError, setAuthError] = useState<string | null>(null);
  const [sessionNotice, setSessionNotice] = useState<string | null>(null);
  const [authAttempt, setAuthAttempt] = useState(0);

  useEffect(() => {
    let alive = true;
    api
      .config()
      .then((c) => {
        if (alive) setConfig(c);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  const dropSession = useCallback((notice: string | null) => {
    clearToken();
    setToken(null);
    setTeacher(null);
    setAuthChecked(true);
    setSessionNotice(notice);
  }, []);

  useEffect(() => {
    setUnauthorizedHandler(() => dropSession("登入已過期，請重新登入。"));
    return () => setUnauthorizedHandler(null);
  }, [dropSession]);

  // Restore the teacher for a stored token.
  useEffect(() => {
    if (!token || teacher) return;
    let alive = true;
    api
      .me()
      .then((t) => {
        if (!alive) return;
        setTeacher(t);
        setAuthChecked(true);
      })
      .catch((err: unknown) => {
        if (!alive) return;
        if (err instanceof ApiError && err.status === 401) {
          dropSession("登入已過期，請重新登入。");
        } else {
          setAuthError(err instanceof ApiError ? err.message : "無法取得登入資訊，請稍後再試。");
          setAuthChecked(true);
        }
      });
    return () => {
      alive = false;
    };
  }, [token, teacher, authAttempt, dropSession]);

  const login = useCallback(async (code: string) => {
    const res = await api.login(code);
    saveToken(res.token);
    setSessionNotice(null);
    setAuthError(null);
    setTeacher(res.teacher);
    setToken(res.token);
    setAuthChecked(true);
  }, []);

  const logout = useCallback(async () => {
    try {
      await api.logout();
    } catch {
      // logging out locally is enough
    }
    dropSession(null);
  }, [dropSession]);

  const retryAuth = useCallback(() => {
    setAuthError(null);
    setAuthChecked(false);
    setAuthAttempt((n) => n + 1);
  }, []);

  const value = useMemo<AppContextValue>(
    () => ({
      config,
      aiEnabled: config ? config.ai_enabled : true,
      token,
      teacher,
      authChecked,
      authError,
      sessionNotice,
      login,
      logout,
      retryAuth,
    }),
    [config, token, teacher, authChecked, authError, sessionNotice, login, logout, retryAuth],
  );

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}

export function useApp(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useApp must be used inside <AppProvider>");
  return ctx;
}
