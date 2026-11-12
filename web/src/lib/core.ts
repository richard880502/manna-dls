import {
  QUESTION_IDS,
  type AssistLevel,
  type ContentSource,
  type CoreDraftRequest,
  type CoreDraftResponse,
  type CoreFocus,
  type CoreQuestion,
  type CoreResponse,
  type QuestionId,
} from "../api/types";

export const CORE_PRE_QUESTION =
  "學完這個單元後，你最希望學生真正理解、能運用或能表達的 2–3 個重要學習重點是什麼？可以包含語言知識、溝通功能、文本理解、思考能力或文化內容。";

export const MAX_FOCUSES = 3;

/** Used only if GET /api/core/questions fails; the server copy is authoritative (versioned in the back office). */
export const FALLBACK_QUESTIONS: CoreQuestion[] = [
  { id: "Q1", prompt: "你希望學生學會什麼？", help: "學生最後要真正學會、能運用或能表達什麼" },
  { id: "Q2", prompt: "為什麼要學這個重點？", help: "與本單元、後續學習或真實溝通有什麼關係" },
  { id: "Q3", prompt: "還有哪些相關內容這次先不教？", help: "你知道、但這個單元不打算教或暫不展開的內容" },
  { id: "Q4", prompt: "這個重點可能哪裡難教？", help: "可能遇到的困難或限制" },
  {
    id: "Q5",
    prompt: "學生可能怎麼想或誤解？",
    help: "學生原本可能的理解、誤解或回應；影響教法的經驗、語言能力或常見困難",
  },
  { id: "Q6", prompt: "還有哪些因素會影響教學？", help: "班級差異、時間、課綱、考試、教材或設備" },
  { id: "Q7", prompt: "你準備怎麼教？為什麼？", help: "活動、方法或材料，以及這樣安排的理由" },
  { id: "Q8", prompt: "你怎麼知道學生懂了？", help: "如何知道學生已理解、仍混淆或能運用；預期的反應或作品" },
];

export const ASSIST_LEVEL_LABELS: Record<AssistLevel, string> = {
  1: "重新說明題意",
  2: "提供思考面向",
  3: "根據你的內容提出一個追問",
};

export interface CellState {
  answer: string;
  /** Text kept locally while 「目前沒有想法」 is on; never sent to the server. */
  stash: string;
  noIdea: boolean;
  contentSource: ContentSource;
  /** Teacher has seen AI help for this cell (enables the content-source selector). */
  sawAi: boolean;
}

export interface FocusState {
  key: string;
  id?: number;
  big_idea: string;
  cells: Record<QuestionId, CellState>;
}

let keyCounter = 0;
export function newKey(): string {
  keyCounter += 1;
  return `f${Date.now().toString(36)}${keyCounter}`;
}

export function emptyCell(): CellState {
  return { answer: "", stash: "", noIdea: false, contentSource: "self", sawAi: false };
}

function allCells(make: (q: QuestionId) => CellState): Record<QuestionId, CellState> {
  const out = {} as Record<QuestionId, CellState>;
  for (const q of QUESTION_IDS) out[q] = make(q);
  return out;
}

export function emptyFocus(): FocusState {
  return { key: newKey(), big_idea: "", cells: allCells(() => emptyCell()) };
}

export function cellFromServer(r: CoreResponse | undefined): CellState {
  if (!r) return emptyCell();
  const noIdea = r.status === "no_idea";
  return {
    answer: noIdea ? "" : (r.answer ?? ""),
    stash: "",
    noIdea,
    contentSource: r.content_source ?? "self",
    sawAi: (r.content_source ?? "self") !== "self",
  };
}

export function focusFromServer(f: CoreFocus): FocusState {
  return {
    key: `id${f.id}`,
    id: f.id,
    big_idea: f.big_idea ?? "",
    cells: allCells((q) => cellFromServer(f.responses?.[q])),
  };
}

export function cellToDraft(c: CellState): CoreDraftResponse {
  if (c.noIdea) return { answer: "", status: "no_idea", content_source: c.contentSource };
  return {
    answer: c.answer,
    status: c.answer.trim() ? "filled" : "blank",
    content_source: c.contentSource,
  };
}

export function buildDraft(focuses: FocusState[]): CoreDraftRequest {
  return {
    focuses: focuses.map((f, index) => {
      const responses: Partial<Record<QuestionId, CoreDraftResponse>> = {};
      for (const q of QUESTION_IDS) responses[q] = cellToDraft(f.cells[q]);
      return {
        ...(f.id !== undefined ? { id: f.id } : {}),
        position: index + 1,
        big_idea: f.big_idea,
        responses,
      };
    }),
  };
}
