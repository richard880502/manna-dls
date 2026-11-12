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
