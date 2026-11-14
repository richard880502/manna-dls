// Token persistence. localStorage can throw (private mode, blocked storage),
// so every access is wrapped and we fall back to an in-memory copy.

const TOKEN_KEY = "manna_dls_token";
let memoryToken: string | null = null;

export function loadToken(): string | null {
  try {
    return window.localStorage.getItem(TOKEN_KEY);
  } catch {
    return memoryToken;
  }
}

export function saveToken(token: string): void {
  memoryToken = token;
  try {
    window.localStorage.setItem(TOKEN_KEY, token);
  } catch {
    // ignore: memory copy is used instead
  }
}

export function clearToken(): void {
  memoryToken = null;
  try {
    window.localStorage.removeItem(TOKEN_KEY);
  } catch {
    // ignore
  }
}

export const TOKEN_STORAGE_KEY = TOKEN_KEY;

// 目前班級 (the class the teacher is working in): per tab, so two tabs can be in two classes.
const CURRENT_CLASS_KEY = "manna.currentClass";
let memoryClass: number | null = null;

export function loadCurrentClass(): number | null {
  try {
    const raw = window.sessionStorage.getItem(CURRENT_CLASS_KEY);
    const id = raw === null ? NaN : Number(raw);
    return Number.isInteger(id) && id > 0 ? id : null;
  } catch {
    return memoryClass;
  }
}

export function saveCurrentClass(classId: number | null): void {
  memoryClass = classId;
  try {
    if (classId === null) window.sessionStorage.removeItem(CURRENT_CLASS_KEY);
    else window.sessionStorage.setItem(CURRENT_CLASS_KEY, String(classId));
  } catch {
    // ignore: memory copy is used instead
  }
}

export const CURRENT_CLASS_STORAGE_KEY = CURRENT_CLASS_KEY;
