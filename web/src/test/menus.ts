import { screen, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

type Scope = ReturnType<typeof within>;

/** Opens a block's 「更多…」 menu (if it is closed) and returns the menu to look in. */
export async function openBlockMore(user: UserEvent, region: Scope): Promise<Scope> {
  const button = region.getByRole("button", { name: /^更多（/ });
  if (button.getAttribute("aria-expanded") !== "true") await user.click(button);
  return within(region.getByRole("group", { name: /的更多工具$/ }));
}

/** Clicks an item of a block's 「更多…」 menu. */
export async function clickBlockMore(user: UserEvent, region: Scope, name: string | RegExp): Promise<void> {
  const menu = await openBlockMore(user, region);
  await user.click(menu.getByRole("button", { name }));
}

/** Opens one of the workspace toolbar's popovers (顯示, 更多…, ⓘ) and returns the popover to look in. */
export async function openBarPopover(user: UserEvent, button: string | RegExp): Promise<Scope> {
  const trigger = screen.getByRole("button", { name: button });
  if (trigger.getAttribute("aria-expanded") !== "true") await user.click(trigger);
  const panelId = trigger.getAttribute("aria-controls");
  const panel = panelId ? document.getElementById(panelId) : null;
  if (!panel) throw new Error(`no popover for ${String(button)}`);
  return within(panel);
}
