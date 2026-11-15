import type { ReactNode } from "react";
import { Navigate, Route, Routes } from "react-router";
import { DemoBanner, ErrorText, Loading } from "./components/common";
import { Layout } from "./components/Layout";
import { AppProvider, useApp } from "./context/AppContext";
import { AdminPage } from "./pages/AdminPage";
import { ClassPage } from "./pages/ClassPage";
import { CorePage } from "./pages/CorePage";
import { HomePage } from "./pages/HomePage";
import { KnowledgePage } from "./pages/KnowledgePage";
import { LessonPage } from "./pages/LessonPage";
import { LibraryPage } from "./pages/LibraryPage";
import { LoginPage } from "./pages/LoginPage";
import { PreferencesPage } from "./pages/PreferencesPage";
import { RecordIndexPage } from "./pages/RecordIndexPage";
import { RemotePage } from "./pages/RemotePage";
import { RunConsolePage } from "./pages/RunConsolePage";
import { RunReviewPage } from "./pages/RunReviewPage";
import { UnitPage } from "./pages/UnitPage";
import { WorksheetCheckPage } from "./pages/WorksheetCheckPage";
import { WorksheetPage } from "./pages/WorksheetPage";
import { WorksheetsPage } from "./pages/WorksheetsPage";

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
          <section className="login-card">
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
      {/* Phone remote opened from the QR code: no teacher login, run-scoped token only. */}
      <Route path="/remote" element={<RemotePage />} />
      <Route
        element={
          <RequireAuth>
            <Layout />
          </RequireAuth>
        }
      >
        <Route index element={<HomePage />} />
        <Route path="classes/:classId" element={<ClassPage />} />
        <Route path="library" element={<LibraryPage />} />
        {/* Old links keep working: /plan was the library; removed pages fall through to home. */}
        <Route path="plan" element={<LibraryPage />} />
        <Route path="units/:unitId" element={<UnitPage />} />
        <Route path="units/:unitId/core" element={<CorePage />} />
        <Route path="lessons/:lessonId" element={<LessonPage />} />
        <Route path="record" element={<RecordIndexPage />} />
        <Route path="runs/:runId" element={<RunConsolePage />} />
        <Route path="runs/:runId/review" element={<RunReviewPage />} />
        <Route path="worksheets" element={<WorksheetsPage />} />
        <Route path="worksheets/:worksheetId" element={<WorksheetPage />} />
        <Route path="worksheets/:worksheetId/check" element={<WorksheetCheckPage />} />
        <Route path="preferences" element={<PreferencesPage />} />
        <Route path="admin" element={<AdminPage />} />
        <Route path="knowledge" element={<KnowledgePage />} />
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
