import { useId, useState, type FormEvent } from "react";
import { errorMessage } from "../../api/client";
import { ErrorText } from "../common";
import { Dialog, useDialogGuard } from "../Dialog";

/** The name field and its buttons; lives inside the dialog so 取消 and Esc ask before typed input is lost. */
function RenameForm({
  label,
  initial,
  emptyMessage,
  onSave,
  onCancel,
}: {
  label: string;
  initial: string;
  emptyMessage: string;
  onSave: (title: string) => Promise<void>;
  onCancel: () => void;
}) {
  const ids = useId();
  const [title, setTitle] = useState(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestClose = useDialogGuard({ busy: saving, dirty: title.trim() !== initial.trim() });

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    const trimmed = title.trim();
    if (!trimmed) {
      setError(emptyMessage);
      return;
    }
    // The same name: nothing to send.
    if (trimmed === initial.trim()) {
      onCancel();
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await onSave(trimmed);
    } catch (err) {
      setError(errorMessage(err));
      setSaving(false);
    }
  }

  return (
    <form className="rename-form" onSubmit={onSubmit}>
      <label className="field-label" htmlFor={`${ids}-title`}>
        {label}
      </label>
      <input id={`${ids}-title`} value={title} onChange={(e) => setTitle(e.target.value)} disabled={saving} />
      <ErrorText message={error} />
      <div className="button-row">
        <button className="primary-button" type="submit" disabled={saving}>
          {saving ? "儲存中…" : "儲存"}
        </button>
        <button type="button" className="text-link is-plain" disabled={saving} onClick={requestClose ?? onCancel}>
          取消
        </button>
      </div>
    </form>
  );
}

/**
 * 改名 for a unit or a lesson (備課庫, 單元頁): the current name, ready to edit. A blank name is
 * refused here as the server would; the same name closes without a request. `onSave` sends it and
 * closes the dialog; a failure is said in the dialog and the name stays editable.
 */
export function RenameDialog({
  title,
  label,
  initial,
  emptyMessage,
  returnFocus,
  onSave,
  onClose,
}: {
  /** The dialog's heading, e.g. 改單元名稱. */
  title: string;
  /** The field's label, e.g. 單元名稱. */
  label: string;
  initial: string;
  /** Said when the field is left blank, e.g. 請輸入單元名稱。 */
  emptyMessage: string;
  returnFocus: () => HTMLElement | null;
  onSave: (title: string) => Promise<void>;
  onClose: () => void;
}) {
  return (
    <Dialog title={title} returnFocus={returnFocus} onClose={onClose}>
      <RenameForm label={label} initial={initial} emptyMessage={emptyMessage} onSave={onSave} onCancel={onClose} />
    </Dialog>
  );
}
