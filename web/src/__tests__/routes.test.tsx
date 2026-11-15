// The prep route (備課菜單) is no longer offered on the unit page or when creating a unit; a unit
// keeps its stored route, and the 單元想法 page still marks the questions that route starts with.
import { screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { baseRoutes, coreVersion, focus, mockApi, renderApp, signIn, unitWith } from "../test/mockApi";

describe("a unit's stored prep route on the 單元想法 page", () => {
  it("highlights Q1–Q3 for goal-first and Q5/Q6 for student-first on the CoRe page", async () => {
    signIn();
    mockApi({ ...baseRoutes(), "GET /api/units/1/core": coreVersion([focus(11, 1, "重點")]) });
    renderApp("/units/1/core");
    const q1 = within(await screen.findByRole("group", { name: /你希望學生學會什麼？/ }));
    expect(q1.getByText("建議先寫")).toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: /這個重點可能哪裡難教？/ })).queryByText("建議先寫")).toBeNull();
  });

  it("student-first highlights Q5 and Q6", async () => {
    signIn();
    mockApi({
      ...baseRoutes(),
      "GET /api/units/1": unitWith({ prep_route: "student" }),
      "GET /api/units/1/core": coreVersion([focus(11, 1, "重點")]),
    });
    renderApp("/units/1/core");
    const q5 = within(await screen.findByRole("group", { name: /學生可能怎麼想或誤解？/ }));
    expect(await q5.findByText("建議先寫")).toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: /還有哪些因素會影響教學？/ })).getByText("建議先寫")).toBeInTheDocument();
    expect(within(screen.getByRole("group", { name: /你希望學生學會什麼？/ })).queryByText("建議先寫")).toBeNull();
  });
});
