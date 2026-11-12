import type { ReactNode } from "react";
import { Navigate, Route, Routes } from "react-router";
import { DemoBanner, ErrorText, Loading } from "./components/common";
import { Layout } from "./components/Layout";
import { AppProvider, useApp } from "./context/AppContext";
import { AdminPage } from "./pages/AdminPage";
import { CorePage } from "./pages/CorePage";
import { HomePage, PlanIndexPage } from "./pages/HomePage";
import { LessonPage } from "./pages/LessonPage";
import { LoginPage } from "./pages/LoginPage";
import { PreferencesPage } from "./pages/PreferencesPage";
import { CommunityPage } from "./pages/static/CommunityPage";
import { ProfilePage, RecordPage, ReflectPage, SharePage } from "./pages/static/StaticPages";
import { UnitPage } from "./pages/UnitPage";

function RequireAuth({ children }: { children: ReactNode }) {
  const { token, teacher, authChecked, authError, retryAuth, logout } = useApp();
  if (!token) return <Navigate to="/login" replace />;
  if (!teacher) {
    if (!authChecked) {
      return (
        <div className="login-shell">
          <Loading label="正在確認登入狀態…" />
        </div>
      );
    }
    return (
      <>
        <DemoBanner />
        <div className="login-shell">
          <section className="login-card panel-card">
            <ErrorText message={authError ?? "無法取得登入資訊，請稍後再試。"} />
            <div className="button-row">
              <button type="button" className="primary-button" onClick={retryAuth}>
                重試
              </button>
              <button type="button" className="ghost-button" onClick={() => void logout()}>
                重新登入
              </button>
            </div>
          </section>
        </div>
      </>
    );
  }
  return <>{children}</>;
}

export function AppRoutes() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route
        element={
          <RequireAuth>
            <Layout />
          </RequireAuth>
        }
      >
        <Route index element={<HomePage />} />
        <Route path="plan" element={<PlanIndexPage />} />
        <Route path="units/:unitId" element={<UnitPage />} />
        <Route path="units/:unitId/core" element={<CorePage />} />
        <Route path="lessons/:lessonId" element={<LessonPage />} />
        <Route path="record" element={<RecordPage />} />
        <Route path="reflect" element={<ReflectPage />} />
        <Route path="share" element={<SharePage />} />
        <Route path="profile" element={<ProfilePage />} />
        <Route path="community" element={<CommunityPage />} />
        <Route path="preferences" element={<PreferencesPage />} />
        <Route path="admin" element={<AdminPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}

export function App() {
  return (
    <AppProvider>
      <AppRoutes />
    </AppProvider>
  );
}
