import { useState } from "react";
import { Link } from "react-router";
import type { WsTemplateSummary } from "../../api/types";
import { templateMeta, templateTitle } from "../../lib/templates";
import { Loading } from "../common";
import { ATTACH_LABEL, Dialog, MaterialChip } from "../material/AttachDialog";
import { LibraryStep, type PickedMaterial } from "../material/MaterialChooser";
import { useReadyTemplates } from "./TemplateSelect";

/**
 * 照我的學習單做: the teacher's worksheet sample. A material (uploaded now or from the library) is
 * read into a template by the server; a ready template from 我的版型 is used as it is.
 */
export type Sample = { kind: "material"; material: PickedMaterial } | { kind: "template"; template: WsTemplateSummary };

export type SampleWay = "upload" | "template" | "library";

const FROM_TEMPLATES = "從我的版型選";

const SAMPLE_WAYS: Array<[SampleWay, string]> = [
  ["upload", "上傳學習單"],
  ["template", FROM_TEMPLATES],
  ["library", "從教材庫選"],
];

/** The home quick box's 附上學習單範本 menu: 附上文章's 上傳檔案 and 從教材庫選, plus 從我的版型選. */
export const SAMPLE_ATTACH: Array<[SampleWay, string]> = [
  ["upload", ATTACH_LABEL.upload],
  ["library", ATTACH_LABEL.library],
  ["template", FROM_TEMPLATES],
];

/** What the sample is called: the file's (material's) name or the template's title. */
export const sampleName = (s: Sample): string =>
  s.kind === "template" ? templateTitle(s.template) : s.material.title.trim() || "未命名教材";

/** The request fields for a sample. */
export const sampleFields = (s: Sample): { template_id: number } | { template_material_id: number } =>
  s.kind === "template" ? { template_id: s.template.id } : { template_material_id: s.material.id };

/** 從我的版型選: the ready templates, one 選用 each. */
function TemplatePicker({
  templates,
  disabled,
  onPick,
}: {
  templates: WsTemplateSummary[] | null;
  disabled: boolean;
  onPick: (t: WsTemplateSummary) => void;
}) {
  if (!templates) return <Loading />;
  if (templates.length === 0) return <p className="empty-line">還沒有版型。</p>;
  return (
    <ul className="rows material-rows" aria-label="我的版型">
      {templates.map((t) => (
        <li key={t.id} className="row">
          <div>
            <h3>{templateTitle(t)}</h3>
            <p className="meta">{templateMeta(t)}</p>
          </div>
          <p className="side">
            <button
              type="button"
              className="secondary-button small-button"
              disabled={disabled}
              aria-label={`選用「${templateTitle(t)}」`}
              onClick={() => onPick(t)}
            >
              選用
            </button>
          </p>
        </li>
      ))}
    </ul>
  );
}

/**
 * 「你的學習單範本」: 上傳學習單 (the default: the library's upload form, right here), 從我的版型選 or
 * 從教材庫選. Once chosen, the sample shows as a chip (a template with its section count and
 * 看版型內容, which opens the template in a new tab).
 */
export function SampleChooser({
  sample,
  templates,
  disabled,
  onChange,
}: {
  sample: Sample | null;
  /** The ready templates; null while loading. */
  templates: WsTemplateSummary[] | null;
  disabled: boolean;
  onChange: (next: Sample | null) => void;
}) {
  const [way, setWay] = useState<SampleWay>("upload");

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

  const label = SAMPLE_WAYS.find(([key]) => key === way)?.[1] ?? "";
  return (
    <div className="sample-chooser">
      <div className="material-ways" role="group" aria-label="範本從哪裡來">
        {SAMPLE_WAYS.map(([key, text]) => (
          <button
            key={key}
            type="button"
            className="way-button"
            aria-pressed={way === key}
            disabled={disabled}
            onClick={() => setWay(key)}
          >
            {text}
          </button>
        ))}
      </div>
      <div role="region" aria-label={label} className="sample-step">
        {way === "template" ? (
          <TemplatePicker templates={templates} disabled={disabled} onPick={(t) => onChange({ kind: "template", template: t })} />
        ) : (
          <LibraryStep key={way} way={way} onPicked={(m) => onChange({ kind: "material", material: m })} />
        )}
      </div>
    </div>
  );
}

/** 從我的版型選 in a dialog: the ready templates, asked for when it opens. */
function ReadyTemplates({ onPick }: { onPick: (t: WsTemplateSummary) => void }) {
  const templates = useReadyTemplates();
  return <TemplatePicker templates={templates} disabled={false} onPick={onPick} />;
}

/**
 * The sample from one of SAMPLE_ATTACH, in a dialog (like AttachDialog for the article): 上傳檔案
 * or 從教材庫選 end with a library material, 從我的版型選 with a ready template.
 */
export function SampleDialog({
  way,
  returnFocus,
  onChoice,
  onClose,
}: {
  way: SampleWay;
  returnFocus?: () => HTMLElement | null;
  onChoice: (sample: Sample) => void;
  onClose: () => void;
}) {
  const title = SAMPLE_ATTACH.find(([key]) => key === way)?.[1] ?? "";
  return (
    <Dialog title={title} returnFocus={returnFocus} onClose={onClose}>
      <div className="attach-step">
        {way === "template" ? (
          <ReadyTemplates onPick={(t) => onChoice({ kind: "template", template: t })} />
        ) : (
          <LibraryStep way={way} onPicked={(m) => onChoice({ kind: "material", material: m })} />
        )}
      </div>
    </Dialog>
  );
}
