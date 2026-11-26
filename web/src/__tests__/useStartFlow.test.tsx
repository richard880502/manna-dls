// useStartFlow: what the home quick box and /create share. The pages' own tests cover the whole
// flows; these pin the hook's rules: busy ends only on failure, and the OCR 409 is said (keyed to
// what is attached) instead of being shown as an error.
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ApiError } from "../api/client";
import { useStartFlow } from "../components/start/useStartFlow";

const READING = "這份檔案正在辨識文字（第 2／5 頁），完成後再試一次。";

describe("useStartFlow", () => {
  it("a failed send shows its message and frees the page; a good one stays busy (the page is left)", async () => {
    const { result } = renderHook(() => useStartFlow("a"));
    await act(() => result.current.submit(async () => undefined));
    expect(result.current.busy).toBe(true);
    expect(result.current.error).toBeNull();

    await act(() =>
      result.current.submit(async () => {
        throw new ApiError(500, "boom", "壞了");
      }),
    );
    expect(result.current.busy).toBe(false);
    expect(result.current.error).toBe("壞了");
  });

  it("an OCR 409 is said beside the button for the same attachment only", async () => {
    const { result, rerender } = renderHook(({ key }) => useStartFlow(key), { initialProps: { key: "a" } });
    await act(() =>
      result.current.submit(async () => {
        throw new ApiError(409, "conflict", READING);
      }),
    );
    expect(result.current.ocrBusy).toBe(READING);
    expect(result.current.error).toBeNull();
    expect(result.current.busy).toBe(false);

    rerender({ key: "b" });
    expect(result.current.ocrBusy).toBeNull();
    rerender({ key: "a" });
    expect(result.current.ocrBusy).toBe(READING);

    // Trying again clears it.
    await act(() => result.current.submit(async () => undefined));
    expect(result.current.ocrBusy).toBeNull();
  });
});
