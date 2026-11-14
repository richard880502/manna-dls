import { useEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { logScaffold } from "../../lib/scaffold";
import { ErrorText } from "../common";

/** Document Picture-in-Picture (Chrome / Edge only). */
export interface DocumentPipApi {
  requestWindow: (options?: { width?: number; height?: number }) => Promise<Window>;
}

export function getDocumentPip(): DocumentPipApi | null {
  if (typeof window === "undefined") return null;
  const api = (window as Window & { documentPictureInPicture?: DocumentPipApi }).documentPictureInPicture;
  return api && typeof api.requestWindow === "function" ? api : null;
}

/** Copies the page's styles into the PiP document so the mini console looks the same. */
function copyStyles(target: Document): void {
  for (const sheet of Array.from(document.styleSheets)) {
    try {
      const css = Array.from(sheet.cssRules, (rule) => rule.cssText).join("\n");
      const style = target.createElement("style");
      style.textContent = css;
      target.head.appendChild(style);
    } catch {
      if (sheet.href) {
        const link = target.createElement("link");
        link.rel = "stylesheet";
        link.href = sheet.href;
        target.head.appendChild(link);
      }
    }
  }
  target.body.classList.add("pip-body");
}

/**
 * 「子母畫面小視窗」: a small always-on-top window with the two bookmark buttons. Only shown
 * when the browser supports Document PiP; warns that a mirrored projector shows it to students.
 */
export function PipControl({ children }: { children: ReactNode }) {
  const pip = getDocumentPip();
  const [confirming, setConfirming] = useState(false);
  const [pipWindow, setPipWindow] = useState<Window | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!pipWindow) return;
    const onClose = () => setPipWindow(null);
    pipWindow.addEventListener("pagehide", onClose);
    return () => pipWindow.removeEventListener("pagehide", onClose);
  }, [pipWindow]);

  // Close the small window when the console goes away.
  useEffect(() => {
    if (!pipWindow) return;
    return () => pipWindow.close();
  }, [pipWindow]);

  if (!pip) return null;

  async function open() {
    if (!pip) return;
    setConfirming(false);
    setError(null);
    try {
      const win = await pip.requestWindow({ width: 360, height: 300 });
      copyStyles(win.document);
      setPipWindow(win);
      logScaffold("SC-RECORD", "expand", { trigger: "pip" });
    } catch {
      setError("無法開啟子母畫面小視窗，請改用這個頁面或手機遙控。");
    }
  }

  return (
    <div className="pip-control">
      {pipWindow ? (
        <button type="button" className="text-link is-plain" onClick={() => pipWindow.close()}>
          關閉子母畫面小視窗
        </button>
      ) : (
        <button type="button" className="text-link is-plain" onClick={() => setConfirming(true)} disabled={confirming}>
          子母畫面小視窗
        </button>
      )}
      {confirming && (
        <div className="pip-warning" role="alertdialog" aria-labelledby="pip-warning-title">
          <strong id="pip-warning-title">鏡像投影會被學生看到</strong>
          <p>如果電腦畫面是「鏡像」投影到教室螢幕，小視窗也會一起出現。建議改用「延伸」螢幕，或用手機遙控。</p>
          <div className="button-row">
            <button type="button" className="secondary-button" onClick={() => void open()}>
              仍要開啟
            </button>
            <button type="button" className="ghost-button" onClick={() => setConfirming(false)}>
              取消
            </button>
          </div>
        </div>
      )}
      <ErrorText message={error} />
      {pipWindow && createPortal(<div className="pip-console">{children}</div>, pipWindow.document.body)}
    </div>
  );
}
