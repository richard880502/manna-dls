import type { Capability, Role, Teacher } from "../api/types";

/**
 * Which roles hold which capability. Mirrors `role_can` in api/src/auth.rs (docs/API.md
 * 「角色與權限」); the server enforces it, this only decides what the interface offers.
 * A new capability is one entry here and one check where it is used.
 */
const ROLE_CAPABILITIES: Partial<Record<Role, Capability[]>> = {
  pi: ["administer", "manage_knowledge", "use_materials"],
  kb_manager: ["manage_knowledge", "use_materials"],
  teacher: ["use_materials"],
  ra: ["use_materials"],
  rater: ["use_materials"],
  engineer: ["use_materials"],
};

/** Whether the signed-in account may do this. Prefers what the server reported for the account. */
export function can(teacher: Teacher | null | undefined, capability: Capability): boolean {
  if (!teacher) return false;
  const held = teacher.capabilities ?? ROLE_CAPABILITIES[teacher.role] ?? [];
  return held.includes(capability);
}
