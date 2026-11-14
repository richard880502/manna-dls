import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { ApiError, api, setUnauthorizedHandler } from "../api/client";
import type { AppConfig, Preferences, Teacher } from "../api/types";
import { clearToken, loadCurrentClass, loadToken, saveCurrentClass, saveToken } from "../lib/storage";

interface AppContextValue {
  config: AppConfig | null;
  /** AI is assumed available until /api/config says otherwise; the server still answers "disabled" if not. */
  aiEnabled: boolean;
  token: string | null;
  teacher: Teacher | null;
  authChecked: boolean;
  authError: string | null;
  sessionNotice: string | null;
  /** Platform switch (主持人後台): teachers may pick a prep route. Off → fixed goal-first order. */
  routeChoiceEnabled: boolean;
  /** Latest preferences version; null = none yet (or failed to load — see preferencesStatus). */
  preferences: Preferences | null;
  /** "loaded" with preferences === null means the first-use survey has not been done. */
  preferencesStatus: "idle" | "loading" | "loaded" | "error";
  setPreferences: (prefs: Preferences) => void;
  /**
   * 目前班級: the class the teacher is working in (set by entering a class, cleared by entering
   * the library). Pages only honour it when that class uses the unit on screen.
   */
  currentClassId: number | null;
  setCurrentClassId: (classId: number | null) => void;
  patchConfig: (change: Partial<AppConfig>) => void;
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
  const [preferences, setPreferencesState] = useState<Preferences | null>(null);
  /** Teacher id the preferences were fetched for (null = not fetched yet). */
  const [prefsFor, setPrefsFor] = useState<number | null>(null);
  const [prefsError, setPrefsError] = useState(false);
  const [currentClassId, setCurrentClassState] = useState<number | null>(() => loadCurrentClass());

  const setCurrentClassId = useCallback((classId: number | null) => {
    saveCurrentClass(classId);
    setCurrentClassState(classId);
  }, []);

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
    saveCurrentClass(null);
    setCurrentClassState(null);
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

  // Load the latest preferences once per signed-in teacher.
  const teacherId = teacher?.id ?? null;
  useEffect(() => {
    if (teacherId === null) return;
    let alive = true;
    api
      .myPreferences()
      .then((p) => {
        if (!alive) return;
        setPreferencesState(p ?? null);
        setPrefsError(false);
        setPrefsFor(teacherId);
      })
      .catch(() => {
        if (!alive) return;
        // Not blocking: the app works with default (no) preferences.
        setPreferencesState(null);
        setPrefsError(true);
        setPrefsFor(teacherId);
      });
    return () => {
      alive = false;
    };
  }, [teacherId]);

  const setPreferences = useCallback(
    (prefs: Preferences) => {
      setPreferencesState(prefs);
      setPrefsError(false);
      if (teacherId !== null) setPrefsFor(teacherId);
    },
    [teacherId],
  );

  const patchConfig = useCallback((change: Partial<AppConfig>) => {
    setConfig((prev) => (prev ? { ...prev, ...change } : prev));
  }, []);

  let preferencesStatus: AppContextValue["preferencesStatus"] = "idle";
  if (teacherId !== null) {
    if (prefsFor !== teacherId) preferencesStatus = "loading";
    else preferencesStatus = prefsError ? "error" : "loaded";
  }
  const currentPreferences = teacherId !== null && prefsFor === teacherId ? preferences : null;

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
      routeChoiceEnabled: config?.route_choice_enabled === true,
      preferences: currentPreferences,
      preferencesStatus,
      setPreferences,
      currentClassId,
      setCurrentClassId,
      patchConfig,
      login,
      logout,
      retryAuth,
    }),
    [
      config,
      token,
      teacher,
      authChecked,
      authError,
      sessionNotice,
      currentPreferences,
      preferencesStatus,
      setPreferences,
      currentClassId,
      setCurrentClassId,
      patchConfig,
      login,
      logout,
      retryAuth,
    ],
  );

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}

export function useApp(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useApp must be used inside <AppProvider>");
  return ctx;
}
