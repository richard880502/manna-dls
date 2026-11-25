// A yes/no remembered per browser (localStorage; unavailable storage just means it isn't remembered).
import { useCallback, useState } from "react";

export const CHAT_COLLAPSED_KEY = "manna.prepChatCollapsed";
export const HEAD_OPEN_KEY = "manna.prepHeadOpen";

function read(key: string, fallback: boolean): boolean {
  try {
    const v = window.localStorage.getItem(key);
    return v === "1" ? true : v === "0" ? false : fallback;
  } catch {
    return fallback;
  }
}

export function useStoredFlag(key: string, fallback: boolean): [boolean, (next: boolean) => void] {
  const [value, setValue] = useState(() => read(key, fallback));
  const set = useCallback(
    (next: boolean) => {
      setValue(next);
      try {
        window.localStorage.setItem(key, next ? "1" : "0");
      } catch {
        /* storage unavailable */
      }
    },
    [key],
  );
  return [value, set];
}
