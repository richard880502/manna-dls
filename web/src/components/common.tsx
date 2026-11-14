import type { ReactNode } from "react";
import type { AiStatus } from "../api/types";
import { useApp } from "../context/AppContext";
import { AI_STATUS_FALLBACK } from "../lib/labels";

export function DemoBanner() {
  const { config } = useApp();
  if (!config?.demo_mode) return null;
  return (
    <div className="demo-banner" role="status">
      <span className="demo-banner-icon" aria-hidden="true">!</span>
      示範環境：請勿輸入真實教師或學生資料
    </div>
  );
}


export function ErrorText({ message }: { message: string | null | undefined }) {
  if (!message) return null;
  return (
    <p className="error-text" role="alert">
      {message}
    </p>
  );
}

export function Notice({ children, tone = "info" }: { children: ReactNode; tone?: "info" | "success" | "warn" }) {
  return <div className={`notice notice-${tone}`}>{children}</div>;
}

/** Message for an AI result that was not shown (blocked / timeout / error / disabled). */
export function AiStatusMessage({ status, message }: { status: AiStatus; message: string | null }) {
  if (status === "shown") return null;
  return (
    <div className={`ai-status ai-status-${status}`} role="status">
      {message || AI_STATUS_FALLBACK[status]}
    </div>
  );
}

export function AiUnavailableNote() {
  return <p className="ai-unavailable">AI 功能目前未開放，你可以繼續手動完成所有步驟。</p>;
}

export function Loading({ label = "載入中…" }: { label?: string }) {
  return (
    <p className="loading-text" aria-live="polite">
      {label}
    </p>
  );
}

export function PrivacyHint() {
  return <p className="field-hint">請不要輸入學生姓名或其他個人資料。</p>;
}
