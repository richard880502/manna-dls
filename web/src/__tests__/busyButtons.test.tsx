// A busy button uses aria-disabled (not disabled): it keeps focus and ignores clicks meanwhile.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { CommunityCard } from "../api/types";
import { Card } from "../components/community/Card";

const card = {
  id: 1,
  title: "T",
  owner_name: "王老師",
  is_mine: false,
  saved: false,
  allow_remix: true,
  saves: 0,
  remixes: 0,
  output: "worksheet",
  cover: null,
} as unknown as CommunityCard;

describe("busy buttons", () => {
  it("a busy 收藏 stays focusable, says aria-disabled and ignores clicks", async () => {
    const onToggleSave = vi.fn();
    const user = userEvent.setup();
    const props = { card, onOpen: () => undefined, onToggleSave, onRemix: () => undefined };
    const { rerender } = render(<ul><Card {...props} busy={false} /></ul>);
    await user.click(screen.getByRole("button", { name: "收藏" }));
    expect(onToggleSave).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "收藏" })).toHaveFocus();
    rerender(<ul><Card {...props} busy /></ul>);
    const btn = screen.getByRole("button", { name: "收藏" });
    expect(btn).toHaveAttribute("aria-disabled", "true");
    expect(btn).not.toBeDisabled();
    expect(btn).toHaveFocus();
    await user.click(btn);
    expect(onToggleSave).toHaveBeenCalledTimes(1);
  });
});
