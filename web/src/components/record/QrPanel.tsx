import qrcode from "qrcode-generator";
import { useEffect, useMemo, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { Pairing, RemoteDevice } from "../../api/types";
import { DEVICE_TYPE_LABELS, formatClock } from "../../lib/labels";
import { logScaffold } from "../../lib/scaffold";
import { ErrorText, Notice } from "../common";
import { MetaList } from "../MetaList";

/** Draws `value` as a QR code (SVG rects, no innerHTML). */
export function QrCode({ value, label, size = 220 }: { value: string; label: string; size?: number }) {
  const { path, count } = useMemo(() => {
    const qr = qrcode(0, "M");
    qr.addData(value);
    qr.make();
    const n = qr.getModuleCount();
    let d = "";
    for (let row = 0; row < n; row++) {
      for (let col = 0; col < n; col++) {
        if (qr.isDark(row, col)) d += `M${col + 4},${row + 4}h1v1h-1z`;
      }
    }
    return { path: d, count: n };
  }, [value]);
  const box = count + 8;
  return (
    <svg
      className="qr-code"
      role="img"
      aria-label={label}
      width={size}
      height={size}
      viewBox={`0 0 ${box} ${box}`}
      shapeRendering="crispEdges"
    >
      <rect width={box} height={box} fill="#fff" />
      <path d={path} fill="#000" />
    </svg>
  );
}

/** The URL inside the QR code. The one-time code sits after `#`, so it never reaches server logs. */
export function remoteUrl(code: string, origin: string = window.location.origin): string {
  return `${origin}/remote#pair=${code}`;
}

interface QrPanelProps {
  runId: number;
  remotes: RemoteDevice[];
  onChanged: () => void;
}

/** 手機遙控：show a single-use QR code (10 minutes), list paired phones, revoke. */
export function QrPanel({ runId, remotes, onChanged }: QrPanelProps) {
  const [pairing, setPairing] = useState<Pairing | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!pairing) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [pairing]);

  async function create() {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setPairing(await api.createPairing(runId));
      setNow(Date.now());
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function revoke() {
    setBusy(true);
    setError(null);
    try {
      const res = await api.revokePairing(runId);
      setPairing(null);
      setNotice(`已撤銷：${res.revoked_pairings} 個 QR Code、${res.revoked_remotes} 支遙控器。`);
      onChanged();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  const remainingMs = pairing ? Date.parse(pairing.expires_at) - now : 0;
  const expired = pairing !== null && remainingMs <= 0;
  const minutes = Math.floor(Math.max(0, remainingMs) / 60000);
  const seconds = Math.floor((Math.max(0, remainingMs) % 60000) / 1000);

  return (
    <section className="panel-card qr-panel" aria-labelledby="qr-heading">
      <div className="card-heading">
        <h3 id="qr-heading">用手機當遙控器</h3>
        <span className="soft-tag">已連線 {remotes.length} 支</span>
      </div>
      <p className="field-hint">
        用手機或 iPad 的相機掃描，手機上就會出現兩顆大按鈕。QR Code 10 分鐘內有效、只能配對一次；手機不需要登入，也看不到其他資料。
      </p>
      <Notice tone="warn">QR Code 不要投影給學生看。若不小心被看到，請按「撤銷配對」。</Notice>

      {pairing && !expired && (
        <div className="qr-box">
          <QrCode value={remoteUrl(pairing.code)} label="手機遙控器配對 QR Code" />
          <p className="qr-expiry" aria-live="polite">
            剩下 {minutes}:{String(seconds).padStart(2, "0")}（單次使用）
          </p>
        </div>
      )}
      {expired && <p className="field-hint">這個 QR Code 已過期，請重新產生。</p>}

      <div className="button-row qr-actions">
        <button type="button" className="secondary-button" onClick={() => void create()} disabled={busy}>
          {pairing ? "重新產生 QR Code" : "產生 QR Code"}
        </button>
        <button
          type="button"
          className="ghost-button"
          onClick={() => void revoke()}
          disabled={busy || (!pairing && remotes.length === 0)}
        >
          撤銷配對
        </button>
      </div>

      {remotes.length > 0 && (
        <ul className="remote-list">
          {remotes.map((r) => (
            <li key={r.id}>
              <MetaList
                items={[
                  DEVICE_TYPE_LABELS[r.device_type],
                  `${formatClock(r.paired_at, false)} 配對`,
                  r.last_seen_at ? `最近連線 ${formatClock(r.last_seen_at, false)}` : null,
                ]}
              />
            </li>
          ))}
        </ul>
      )}
      {notice && <Notice tone="success">{notice}</Notice>}
      <ErrorText message={error} />
    </section>
  );
}

/** Logs the scaffold the first time the panel is opened. */
export function logQrOpened(): void {
  logScaffold("SC-RECORD", "expand", { trigger: "qr" });
}
