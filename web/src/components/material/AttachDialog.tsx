import { useId, useState } from "react";
import type { MaterialPurpose } from "../../api/types";
import { UPLOAD_WARNING } from "../../lib/materials";
import { Dialog } from "../Dialog";
import { Icon, type IconName } from "../Icon";
import { useFirstOnPage } from "../OncePerPage";
import { LibraryStep, type MaterialChoice, type PickedMaterial } from "./MaterialChooser";

export type AttachWay = "upload" | "library" | "paste";

/** 「從教材挑選」 opens the library picker (平台教材 | 我的教材) instead of a dialog of its own. */
export const ATTACH_LABEL: Record<AttachWay, string> = {
  upload: "上傳檔案",
  library: "從教材挑選",
  paste: "貼上文字",
};

/** The privacy reminder, said once on the page (an upload form in a dialog may say it instead). */
export function PrivacyReminder() {
  const show = useFirstOnPage();
  if (!show) return null;
  return (
    <p className="material-warning attach-privacy" role="note">
      {UPLOAD_WARNING}
    </p>
  );
}

/** What a choice shows on its chip. */
export function choiceTitle(choice: MaterialChoice): string {
  if (choice.way === "paste") return choice.title.trim() || "貼上的文字";
  return choice.picked?.title.trim() || "未命名教材";
}

/**
 * The attached material inside a prompt card: which slot it fills when there are two (e.g.
 * 「範本：」), its title (and a short fact, e.g. 「3 區」) and ×.
 */
export function MaterialChip({
  label,
  title,
  meta,
  icon = "file",
  disabled,
  onRemove,
}: {
  label?: string;
  title: string;
  meta?: string;
  icon?: IconName;
  disabled?: boolean;
  onRemove: () => void;
}) {
  return (
    <span className="attach-chip" role="status">
      <Icon name={icon} />
      {label && <span className="attach-chip-label">{label}：</span>}
      <span className="attach-chip-title">{title}</span>
      {meta && <span className="attach-chip-meta">· {meta}</span>}
      <button
        type="button"
        className="attach-chip-remove"
        aria-label={`移除${label ?? ""}「${title}」`}
        disabled={disabled}
        onClick={onRemove}
      >
        <Icon name="close" />
      </button>
    </span>
  );
}

/** 貼上文字: a title (optional) and the text, attached when the teacher says so. */
function PasteForm({
  initial,
  onDone,
  onCancel,
}: {
  initial: { title: string; text: string };
  onDone: (title: string, text: string) => void;
  onCancel: () => void;
}) {
  const ids = useId();
  const [title, setTitle] = useState(initial.title);
  const [text, setText] = useState(initial.text);
  return (
    <form
      className="attach-paste"
      onSubmit={(event) => {
        event.preventDefault();
        if (text.trim()) onDone(title, text);
      }}
    >
      <PrivacyReminder />
      <label className="field-label" htmlFor={`${ids}-title`}>
        教材標題 <span className="optional-label">可以不填</span>
      </label>
      <input id={`${ids}-title`} type="text" maxLength={200} value={title} onChange={(e) => setTitle(e.target.value)} />
      <label className="field-label" htmlFor={`${ids}-text`}>
        貼上教材文字
      </label>
      <textarea
        id={`${ids}-text`}
        className="print-input"
        rows={10}
        lang="en"
        maxLength={50000}
        value={text}
        aria-describedby={`${ids}-hint`}
        onChange={(e) => setText(e.target.value)}
      />
      <p className="field-hint" id={`${ids}-hint`}>
        段落之間空一行。請不要貼上學生姓名或其他個人資料。
      </p>
      <div className="button-row">
        <button type="submit" className="primary-button" disabled={text.trim() === ""}>
          附上
        </button>
        <button type="button" className="text-link is-plain" onClick={onCancel}>
          取消
        </button>
      </div>
    </form>
  );
}

/**
 * 上傳檔案 or 貼上文字, in a dialog: MaterialChooser's steps without its big always-open form.
 * Ends with a material choice, or nothing when closed. (從教材挑選 is the LibraryPicker.)
 */
export function AttachDialog({
  way,
  purpose,
  pasted = { title: "", text: "" },
  returnFocus,
  onChoice,
  onClose,
}: {
  way: "upload" | "paste";
  /** An upload for a worksheet sample says so (`worksheet_sample`): it stays out of 我的教材. */
  purpose?: MaterialPurpose;
  /** What was pasted before, to edit it. */
  pasted?: { title: string; text: string };
  returnFocus?: () => HTMLElement | null;
  onChoice: (choice: MaterialChoice) => void;
  onClose: () => void;
}) {
  const take = (m: PickedMaterial) => onChoice({ way, picked: m, title: "", text: "" });
  return (
    <Dialog title={ATTACH_LABEL[way]} returnFocus={returnFocus} onClose={onClose}>
      {way === "paste" ? (
        <PasteForm
          initial={pasted}
          onCancel={onClose}
          onDone={(title, text) => onChoice({ way: "paste", picked: null, title, text })}
        />
      ) : (
        <div className="attach-step">
          <LibraryStep way="upload" purpose={purpose} onPicked={take} />
        </div>
      )}
    </Dialog>
  );
}
