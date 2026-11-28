import { useCallback, useRef, useState } from "react";
import { Link } from "react-router";
import type { TemplateCard, WsTemplateSummary } from "../../api/types";
import { templateTitle } from "../../lib/templates";
import { Icon } from "../Icon";
import { LibraryPicker } from "../library/LibraryPicker";
import { PICK_TEMPLATE } from "../library/labels";
import { ATTACH_LABEL, MaterialChip } from "../material/AttachDialog";
import { LibraryStep, type PickedMaterial } from "../material/MaterialChooser";

/** A ready template chosen as the sample (平台範本, 我的範本 or one made from 我的收藏). */
export type PickedTemplate = Pick<WsTemplateSummary, "id" | "title" | "section_count">;

/**
 * 照我的學習單做: the teacher's worksheet sample. A material (uploaded now) is read into a template
 * by the server; a ready template (平台範本, 我的範本 or one made from 我的收藏) is used as it is.
 */
export type Sample = { kind: "material"; material: PickedMaterial } | { kind: "template"; template: PickedTemplate };

export type SampleWay = "upload" | "template";

/** The home quick box's 附上學習單範本 menu: 上傳檔案 and 從範本挑選. */
export const SAMPLE_ATTACH: Array<[SampleWay, string]> = [
  ["upload", ATTACH_LABEL.upload],
  ["template", PICK_TEMPLATE],
];

/** A template card (or detail) as the sample. */
export const templateSample = (t: Pick<TemplateCard, "id" | "title" | "section_count">): Sample => ({
  kind: "template",
  template: { id: t.id, title: t.title, section_count: t.section_count },
});

/** What the sample is called: the file's (material's) name or the template's title. */
export const sampleName = (s: Sample): string =>
  s.kind === "template" ? templateTitle(s.template) : s.material.title.trim() || "未命名教材";

/** The request fields for a sample. */
export const sampleFields = (s: Sample): { template_id: number } | { template_material_id: number } =>
  s.kind === "template" ? { template_id: s.template.id } : { template_material_id: s.material.id };

/**
 * 「你的學習單範本」: 上傳學習單 (the default: the upload form, right here; the file is a worksheet
 * sample, so it stays out of 我的教材) or 從範本挑選 (平台範本 | 我的範本 | 我的收藏). Once chosen,
 * the sample shows as a chip (a template with its section count and 看版型內容, which opens the
 * template in a new tab).
 */
export function SampleChooser({
  sample,
  disabled,
  onChange,
}: {
  sample: Sample | null;
  disabled: boolean;
  onChange: (next: Sample | null) => void;
}) {
  const [picking, setPicking] = useState(false);
  const pickRef = useRef<HTMLButtonElement>(null);
  const backToPick = useCallback(() => pickRef.current, []);

  if (sample) {
    return (
      <div className="prompt-chips sample-chips">
        <MaterialChip
          title={sampleName(sample)}
          meta={sample.kind === "template" ? `${sample.template.section_count} 區` : undefined}
          icon={sample.kind === "template" ? "template" : "file"}
          disabled={disabled}
          onRemove={() => onChange(null)}
        />
        {sample.kind === "template" && (
          <Link className="text-link" to={`/templates/${sample.template.id}`} target="_blank" rel="noopener noreferrer">
            看版型內容
          </Link>
        )}
      </div>
    );
  }

  return (
    <div className="sample-chooser">
      <div className="sample-pick-row">
        <button
          ref={pickRef}
          type="button"
          className="secondary-button"
          aria-haspopup="dialog"
          disabled={disabled}
          onClick={() => setPicking(true)}
        >
          <Icon name="template" />
          {PICK_TEMPLATE}
        </button>
        <span className="quiet-note">平台範本、你自己的範本，或收藏的學習單</span>
      </div>
      <p className="sample-or">或上傳一份你做過的學習單</p>
      <div role="region" aria-label="上傳學習單" className="sample-step">
        <LibraryStep way="upload" purpose="worksheet_sample" onPicked={(m) => onChange({ kind: "material", material: m })} />
      </div>
      {picking && (
        <LibraryPicker
          kind="template"
          returnFocus={backToPick}
          onClose={() => setPicking(false)}
          onPickTemplate={(card) => {
            setPicking(false);
            onChange(templateSample(card));
          }}
        />
      )}
    </div>
  );
}
