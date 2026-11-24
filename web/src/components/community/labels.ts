import type { CommunityShareName } from "../../api/types";

/** What the kind of worksheet is called (`output` of the worksheet's settings). */
export const OUTPUT_LABEL: Record<string, string> = {
  worksheet: "學習單",
  comprehension: "閱讀理解題",
  tiered: "差異化學習單",
};

export const outputLabel = (output: string | null): string | null => (output ? (OUTPUT_LABEL[output] ?? null) : null);

/** How the owner is shown to the others. */
export const SHARE_NAME_LABEL: Record<CommunityShareName, string> = { code: "代號", anonymous: "匿名" };

/** What an anonymous owner is called (the server sends the same text). */
export const ANONYMOUS_NAME = "一位老師";

export type CommunityTab = "explore" | "saved" | "mine";
export const COMMUNITY_TABS: ReadonlyArray<readonly [CommunityTab, string]> = [
  ["explore", "探索"],
  ["saved", "我的收藏"],
  ["mine", "我的分享"],
];
