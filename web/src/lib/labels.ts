import type {
  AiStatus,
  ContentSource,
  DecisionReason,
  DeviceType,
  Handling,
  IcapLevel,
  Polarity,
  RecordVia,
  Role,
  SchoolTrack,
  StudentBehavior,
  Unit,
} from "../api/types";

export const SCHOOL_TRACK_LABELS: Record<SchoolTrack, string> = {
  general: "普高",
  vocational: "技高",
};

export const ROLE_LABELS: Record<Role, string> = {
  teacher: "教師",
  pi: "主持人",
  ra: "研究助理",
  rater: "評分者",
  engineer: "工程師",
};

export const CORE_STATUS_LABELS: Record<Unit["core_status"], string> = {
  none: "尚未開始",
  draft: "草稿",
  completed: "已完成",
};

export const CONTENT_SOURCE_LABELS: Record<ContentSource, string> = {
  self: "未經 AI 協助",
  self_after_ai: "看過 AI 內容後自行填寫",
  modified_ai_example: "修改 AI 例子",
  adopted_ai_example: "直接採用 AI 例子",
};

export const ICAP_LABELS: Record<IcapLevel, string> = {
  passive: "Passive 被動接收",
  active: "Active 主動操作",
  constructive: "Constructive 建構產出",
  interactive: "Interactive 互動共構",
};

export const ICAP_LEVELS: readonly IcapLevel[] = ["passive", "active", "constructive", "interactive"];

export const DECISION_REASON_LABELS: Record<DecisionReason, string> = {
  source_problem: "來源有問題",
  curriculum_mismatch: "與課綱或教學目標不符",
  not_feasible: "不可行",
  unclear: "說明不清",
  better_idea: "我有更好的做法",
  other: "其他",
};

export const DECISION_REASONS: readonly DecisionReason[] = [
  "source_problem",
  "curriculum_mismatch",
  "not_feasible",
  "unclear",
  "better_idea",
  "other",
];

export const AI_STATUS_FALLBACK: Record<Exclude<AiStatus, "shown">, string> = {
  blocked: "目前暫無建議，你可以繼續手動完成。",
  timeout: "AI 回應逾時，你可以繼續手動完成。",
  error: "AI 暫時無法使用，你可以繼續手動完成。",
  disabled: "AI 功能目前未開放，你可以繼續手動完成。",
};

export const ACTIVITY_TYPE_OPTIONS: readonly string[] = [
  "暖身",
  "複習",
  "直接教學",
  "教師示範",
  "引導練習",
  "個人練習",
  "兩兩討論",
  "小組討論",
  "聽力活動",
  "閱讀活動",
  "口說練習",
  "寫作練習",
  "形成性評量",
  "總結",
];

export const PRIVACY_HINT = "請不要輸入學生姓名或其他個人資料。";

// ---- Teach & Record ----

export const POLARITY_LABELS: Record<Polarity, string> = { good: "順利", stuck: "卡住" };

export const DEVICE_TYPE_LABELS: Record<DeviceType, string> = { phone: "手機", tablet: "平板", desktop: "電腦" };

export const VIA_LABELS: Record<RecordVia, string> = { console: "課堂紀錄頁", pip: "子母畫面", remote: "手機遙控" };

/** Canonical order (same as the server). */
export const STUDENT_BEHAVIORS: readonly StudentBehavior[] = [
  "listening",
  "note_taking",
  "answering",
  "asking",
  "explaining",
  "discussing",
  "probing",
  "co_completing",
  "other",
];

export const BEHAVIOR_LABELS: Record<StudentBehavior, string> = {
  listening: "聆聽",
  note_taking: "筆記",
  answering: "回答",
  asking: "提問",
  explaining: "解釋",
  discussing: "討論",
  probing: "相互追問",
  co_completing: "共同完成任務",
  other: "其他",
};

export const HANDLINGS: readonly Handling[] = [
  "re_explain",
  "add_example",
  "adjust_questions",
  "change_activity",
  "adjust_time",
  "defer",
];

export const HANDLING_LABELS: Record<Handling, string> = {
  re_explain: "重新解釋",
  add_example: "補充例子",
  adjust_questions: "調整提問",
  change_activity: "改變活動",
  adjust_time: "調整時間",
  defer: "稍後處理",
};

/** HH:MM:SS in local time (bookmark device timestamps). */
export function formatClock(value: string | null | undefined, withSeconds = true): string {
  if (!value) return "";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return value;
  const pad = (n: number) => String(n).padStart(2, "0");
  const hm = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  return withSeconds ? `${hm}:${pad(d.getSeconds())}` : hm;
}

/** Elapsed time as HH:MM:SS. */
export function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(Math.floor(total / 3600))}:${pad(Math.floor((total % 3600) / 60))}:${pad(total % 60)}`;
}

export function formatDateTime(value: string | null | undefined): string {
  if (!value) return "";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return value;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}/${pad(d.getMonth() + 1)}/${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function formatDate(value: string | null | undefined): string {
  if (!value) return "";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return value;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}/${pad(d.getMonth() + 1)}/${pad(d.getDate())}`;
}
