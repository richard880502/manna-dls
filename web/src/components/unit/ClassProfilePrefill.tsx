import type { ClassProfileDefault } from "../../api/types";
import { CLASS_PROFILE_MAX_CHARS } from "../../lib/preferences";
import { PrivacyHint } from "../common";

/** Where the prefilled class profile comes from, in the teacher's words (R3). */
export function prefillSourceText(prefill: ClassProfileDefault): string {
  if (prefill.source === "previous_unit" && prefill.source_unit) {
    return `沿用上一個單元『${prefill.source_unit.title}』的班級特性，可修改`;
  }
  if (prefill.source === "preference") return "來自你的備課偏好，可修改";
  return "還沒有可以沿用的班級特性，可以留白";
}

interface ClassProfilePrefillProps {
  /** null while it loads, or when it could not be loaded (the server applies the same default). */
  prefill: ClassProfileDefault | null;
  /** null = not editing: the prefill is shown as is and not sent; the server applies it. */
  draft: string | null;
  onDraft: (value: string | null) => void;
  disabled: boolean;
}

/**
 * 建立新單元: the class profile a new unit will get, with its source, as an editable prefill.
 * It is the teacher's own earlier text (previous unit, else the preference survey), never AI.
 */
export function ClassProfilePrefill({ prefill, draft, onDraft, disabled }: ClassProfilePrefillProps) {
  if (draft !== null) {
    return (
      <div className="profile-prefill is-editing">
        <label className="field-label" htmlFor="new-unit-profile">
          班級特性（選填）
        </label>
        <textarea
          id="new-unit-profile"
          rows={3}
          value={draft}
          maxLength={CLASS_PROFILE_MAX_CHARS}
          disabled={disabled}
          aria-describedby="new-unit-profile-source"
          onChange={(e) => onDraft(e.target.value)}
        />
        <div className="profile-prefill-foot">
          <p className="field-hint" id="new-unit-profile-source">
            {prefill ? prefillSourceText(prefill) : "建立後也可以在單元頁修改"}
          </p>
          {prefill && prefill.source !== "none" && (
            <button type="button" className="ghost-button small-button" onClick={() => onDraft(null)} disabled={disabled}>
              還原沿用的內容
            </button>
          )}
        </div>
        <PrivacyHint />
      </div>
    );
  }

  if (!prefill) return null;

  return (
    <div className="profile-prefill" role="group" aria-labelledby="new-unit-profile-heading">
      <div className="profile-prefill-head">
        <div>
          <span className="field-label" id="new-unit-profile-heading">
            班級特性
          </span>
          <p className="field-hint">{prefillSourceText(prefill)}</p>
        </div>
        <button
          type="button"
          className="ghost-button small-button"
          onClick={() => onDraft(prefill.class_profile)}
          disabled={disabled}
        >
          {prefill.class_profile ? "修改" : "填寫"}
        </button>
      </div>
      {prefill.class_profile && <p className="profile-prefill-text">{prefill.class_profile}</p>}
    </div>
  );
}
