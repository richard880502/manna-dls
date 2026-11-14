import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";
import { sopConfig } from "../components/sop/useSop";
import { autosaveConfig } from "../lib/useAutosave";

// jsdom does not implement scrolling.
Object.defineProperty(window, "scrollTo", { value: () => undefined, writable: true });

beforeEach(() => {
  // The quiet save waits for its timer only in tests that shorten it: otherwise a save happens
  // when a field is left, when the lesson starts or when the page is left, which tests can time.
  autosaveConfig.delayMs = 60_000;
  sopConfig.pollMs = 20;
  try {
    window.localStorage.clear();
    window.sessionStorage.clear();
  } catch {
    // ignore
  }
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
