// Fixed SVG icons for the two bookmark buttons (spec §4.3: 圖示為固定圖檔，不用系統表情符號).
// Drawn here so they look the same on every device and never depend on an emoji font.

export function GoodIcon({ size = 56 }: { size?: number }) {
  return (
    <svg className="record-icon" width={size} height={size} viewBox="0 0 48 48" aria-hidden="true" focusable="false">
      <rect x="5" y="20" width="9" height="21" rx="2.5" fill="currentColor" />
      <path
        d="M17 21.5 25.2 8.6c1.5-2.4 5.6-1.6 5.6 1.9 0 .5-.1 1-.2 1.5L29 18.5h9.4c3 0 5 2.8 4.2 5.6l-3.3 12.6c-.5 2-2.2 3.3-4.2 3.3H17Z"
        fill="currentColor"
      />
    </svg>
  );
}

export function StuckIcon({ size = 56 }: { size?: number }) {
  return (
    <svg className="record-icon" width={size} height={size} viewBox="0 0 48 48" aria-hidden="true" focusable="false">
      <path
        d="M21.4 6.6c1.2-2 4-2 5.2 0l18 31.2c1.2 2-.3 4.5-2.6 4.5H6c-2.3 0-3.8-2.5-2.6-4.5Z"
        fill="currentColor"
      />
      <rect x="21.8" y="16" width="4.4" height="14" rx="2.2" fill="#fff" />
      <circle cx="24" cy="35.4" r="2.6" fill="#fff" />
    </svg>
  );
}

export function PolarityIcon({ polarity, size = 20 }: { polarity: "good" | "stuck"; size?: number }) {
  return polarity === "good" ? <GoodIcon size={size} /> : <StuckIcon size={size} />;
}
