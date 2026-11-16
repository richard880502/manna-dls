// Line icons for the sidebar, the tool cards and the prompt boxes: one stroke weight, drawn in
// currentColor so they take the colour of the text beside them. Always decorative: the label next
// to the icon (or the button's aria-label) names it.

const PATHS = {
  home: "M3 10.5 12 3l9 7.5M5 9v11h5v-6h4v6h5V9",
  // 照我的學習單做: an old sheet, and a new one made after it.
  mimic: "M3 3h8v10H3zM5.5 6.5h3M5.5 9.5h3M13 11h8v10h-8zM15.5 14.5h3M15.5 17.5h3M7 16v1.5A2.5 2.5 0 0 0 9.5 20H11m-1.5-1.5L11 20l-1.5 1.5",
  comprehension: "M12 6.5C10 5 7 4.5 3.5 5v13c3.5-.5 6.5 0 8.5 1.5m0-13c2-1.5 5-2 8.5-1.5v13c-3.5-.5-6.5 0-8.5 1.5m0-13v13",
  tiered: "M4 20h16M4 20v-4h5v4m0 0v-8h5v8m0 0V7h5v13",
  activity: "M4 4h16v11H4zM12 15v5m-4 0h8M8 8.5h8M8 11.5h5",
  kit: "M3.5 8 12 3.5 20.5 8v8L12 20.5 3.5 16zM3.5 8 12 12.5 20.5 8M12 12.5v8",
  worksheet: "M6 3h9l4 4v14H6zM15 3v4h4M9 11h7M9 14.5h7M9 18h4",
  workspace: "M3 6.5h6l2 2h10V19H3zM3 6.5V5h6",
  sheets: "M8 3h8l4 4v11H8zM16 3v4h4M4 7v14h11",
  library: "M4 4h4v16H4zm4 0h4v16H8zm5 1 3.8-1 3.7 15.5-3.8 1z",
  template: "M4 4h16v16H4zM4 9h16M10 9v11",
  more: "M5 12h.01M12 12h.01M19 12h.01",
  classes: "M9 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zm-6 9c0-3.3 2.7-6 6-6s6 2.7 6 6m1-9a3 3 0 1 0 0-6m2 15c0-2.6-1.4-4.8-3.5-5.6",
  record: "M8 4h8v3H8zM6 5.5H4V21h16V5.5h-2M8 12h8M8 16h5",
  prefs: "M4 6h10m4 0h2M4 12h4m4 0h8M4 18h12m4 0h0M14 4v4M8 10v4M16 16v4",
  knowledge: "M4 6c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3zm0 0v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3",
  admin: "M12 3 4.5 6v5.5c0 4.5 3.2 8.2 7.5 9.5 4.3-1.3 7.5-5 7.5-9.5V6z",
  logout: "M14 4h5v16h-5M10 8l-4 4 4 4M6 12h10",
  menu: "M4 6.5h16M4 12h16M4 17.5h16",
  close: "M6 6l12 12M18 6 6 18",
  collapse: "M15 6l-6 6 6 6",
  chevron: "M6 9l6 6 6-6",
  paperclip: "M20 11.5 12.3 19a5 5 0 0 1-7.1-7.1L13 4.2a3.3 3.3 0 0 1 4.7 4.7l-7.7 7.7a1.7 1.7 0 0 1-2.4-2.4l7-7",
  upload: "M12 16V4m-5 5 5-5 5 5M4 16v4h16v-4",
  file: "M6 3h9l4 4v14H6zM15 3v4h4",
  paste: "M9 4h6v3H9zM7 5.5H5V21h14V5.5h-2",
  send: "M12 19V5m-6 6 6-6 6 6",
  sparkle: "M12 3v4m0 10v4M3 12h4m10 0h4M6 6l2.5 2.5m7 7L18 18M18 6l-2.5 2.5m-7 7L6 18",
  // 教材參考: a bound book with a bookmark (the knowledge base's shelf).
  reference: "M5 5.5A2.5 2.5 0 0 1 7.5 3H19v14.5H7.5A2.5 2.5 0 0 0 5 20zm0 14.5A1.5 1.5 0 0 0 6.5 21H19M10 3v6.5l2-1.5 2 1.5V3",
  search: "M10.5 17.5a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20.5 20.5l-5-5",
  preview: "M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12zm9.5 2.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z",
  // 可直接出題: ready to go.
  ready: "M13 3 5 13.5h6.5L10.5 21 19 10.5h-6.5z",
  // 英文指示／中文指示: what the sheet says to the student.
  speech: "M4 5h16v11H10l-4.5 3.5V16H4z",
  copy: "M9 9h11v12H9zM6 15H4V3h12v2",
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({ name, className }: { name: IconName; className?: string }) {
  return (
    <svg
      className={`ui-icon${className ? ` ${className}` : ""}`}
      viewBox="0 0 24 24"
      width="20"
      height="20"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
