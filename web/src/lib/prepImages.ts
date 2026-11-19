import type { PictureOutcome } from "../api/types";

/** What a message may carry: PNG, JPEG or WebP, up to 5 MB each, two at a time. */
export const IMAGE_TYPES = ["image/png", "image/jpeg", "image/webp"];
export const IMAGE_ACCEPT = IMAGE_TYPES.join(",");
export const IMAGE_MAX_BYTES = 5 * 1024 * 1024;
export const IMAGE_MAX_COUNT = 2;
export const IMAGE_PRIVACY_NOTE = "請不要上傳有學生臉孔或姓名的照片。";

export const IMAGE_TYPE_MESSAGE = "只能附 PNG、JPEG 或 WebP 圖片。";
export const IMAGE_COUNT_MESSAGE = `一則訊息最多附 ${IMAGE_MAX_COUNT} 張圖片。`;
export const imageSizeMessage = (name: string) => `圖片最大 5 MB，「${name}」超過了。`;

/** Why a file cannot be attached (null: it can). The count is checked by the caller. */
export function checkImageFile(file: Pick<File, "name" | "type" | "size">): string | null {
  if (!IMAGE_TYPES.includes(file.type)) return IMAGE_TYPE_MESSAGE;
  if (file.size > IMAGE_MAX_BYTES) return imageSizeMessage(file.name);
  return null;
}

/** The image files among dropped or pasted files. */
export const imageFilesOf = (files: ArrayLike<File> | null | undefined): File[] =>
  Array.from(files ?? []).filter((f) => f.type.startsWith("image/"));

/** What happened to one picture action, after 採用 or a direct revision. */
export function outcomeNote(o: PictureOutcome): string {
  if (o.status === "queued") return "已排入繪圖，畫好會自動出現";
  if (o.status === "removed") return "已移除";
  return o.message || "沒有做";
}
