import { useId, useState } from "react";
import type { Material, MaterialDetail, MaterialPurpose } from "../../api/types";
import { ocrBusyLine, ocrFailed, ocrFailedLine, ocrOpen } from "../../lib/ocr";
import { withTextOnly } from "../../lib/templates";
import { ErrorText, Loading } from "../common";
import { MaterialAddForm, MaterialPicker, useMaterialLibrary } from "./MaterialLibrary";
import { OcrFollow } from "./Ocr";

export type MaterialWay = "upload" | "library" | "paste";

export const MATERIAL_WAYS: Array<{ key: MaterialWay; label: string }> = [
  { key: "upload", label: "上傳檔案" },
  { key: "library", label: "從教材庫選" },
  { key: "paste", label: "貼上文字" },
];

export const NO_TEXT_LINE =
  "這份檔案讀不到文字（可能是掃描檔），Manna 沒辦法照它出題。請換一份，或改用貼上文字。";

/** A chosen library material; `looks_like_worksheet` comes along when the server said so. */
export type PickedMaterial = Pick<Material, "id" | "title" | "looks_like_worksheet"> & {
  /** How many paragraphs it has (the long-material notice); older picks may not know. */
  paragraph_count?: number;
};

/** Where the material is coming from, and what has been chosen so far. */
export interface MaterialChoice {
  way: MaterialWay;
  picked: PickedMaterial | null;
  title: string;
  text: string;
}

export const emptyMaterialChoice = (): MaterialChoice => ({ way: "upload", picked: null, title: "", text: "" });

/** The request fields for a choice: a library id, or pasted text; null while nothing is chosen. */
export function materialFields(
  choice: MaterialChoice,
): { material_id: number } | { material: { title: string; text: string } } | null {
  if (choice.way === "paste") {
    return choice.text.trim() ? { material: { title: choice.title.trim(), text: choice.text } } : null;
  }
  return choice.picked ? { material_id: choice.picked.id } : null;
}

/** What to say when the teacher goes on without a material. */
export function missingMaterialLine(choice: MaterialChoice): string {
  if (choice.way === "upload") return "請先上傳教材檔案。";
  if (choice.way === "library") return "請先從教材庫選一份教材。";
  return "請貼上教材文字。";
}

/**
 * 上傳檔案 and 從教材庫選: both end with one library material that has text (also the home page's
 * attach dialogs). A scanned upload is read first (Manna's progress shows here) and taken once
 * its text is there; a worksheet sample is taken at once, since the server waits for it.
 */
export function LibraryStep({
  way,
  purpose,
  onPicked,
}: {
  way: "upload" | "library";
  /** An upload for a worksheet sample (`worksheet_sample`) stays out of 我的教材. */
  purpose?: MaterialPurpose;
  onPicked: (m: PickedMaterial) => void;
}) {
  const { library, error, reload } = useMaterialLibrary();
  const [problem, setProblem] = useState<string | null>(null);
  /** An upload whose text Manna is still reading (OCR). */
  const [reading, setReading] = useState<MaterialDetail | null>(null);

  function take(m: Pick<Material, "id" | "title" | "text_status" | "looks_like_worksheet" | "ocr"> & { paragraph_count?: number }) {
    // 照我的學習單做 waits for its sample's OCR on the server side.
    const waitsOnServer = purpose === "worksheet_sample" && ocrOpen(m.ocr);
    if (m.ocr && ocrOpen(m.ocr) && !waitsOnServer) {
      setProblem(ocrBusyLine(m.ocr));
      return;
    }
    if (m.text_status !== "ready" && !waitsOnServer) {
      setProblem(ocrFailed(m.ocr) && m.ocr ? ocrFailedLine(m.ocr) : NO_TEXT_LINE);
      return;
    }
    setProblem(null);
    onPicked({
      id: m.id,
      title: m.title,
      ...(m.looks_like_worksheet ? { looks_like_worksheet: true } : {}),
      ...(typeof m.paragraph_count === "number" ? { paragraph_count: m.paragraph_count } : {}),
    });
  }

  function added(m: MaterialDetail) {
    if ((ocrOpen(m.ocr) || ocrFailed(m.ocr)) && purpose !== "worksheet_sample") {
      setProblem(null);
      setReading(m);
      return;
    }
    take(m);
  }

  if (error) {
    return (
      <div>
        <ErrorText message={error} />
        <button type="button" className="text-link is-plain" onClick={reload}>
          再試一次
        </button>
      </div>
    );
  }
  if (!library) return <Loading />;
  if (reading) {
    return (
      <div className="ocr-upload">
        <p className="quiet-note">已上傳「{reading.title.trim() || "未命名教材"}」。文字辨識好就會放進來。</p>
        <OcrFollow
          material={reading}
          onDone={(m) => {
            setReading(null);
            take(m);
          }}
        />
        <button type="button" className="text-link is-plain" onClick={() => setReading(null)}>
          換一份
        </button>
      </div>
    );
  }
  return (
    <>
      {way === "upload" ? (
        <MaterialAddForm way="upload" library={library} purpose={purpose} onAdded={added} />
      ) : (
        <MaterialPicker library={withTextOnly(library)} currentId={null} busy={false} onPick={take} />
      )}
      <ErrorText message={problem} />
    </>
  );
}

/**
 * The material slot shared by 學習單 and 備課: 上傳檔案 (default), 從教材庫選 or 貼上文字. Once a
 * library material is chosen it shows as one compact line with 換一份.
 */
export function MaterialChooser({
  choice,
  onChange,
  pasteHint = "段落之間空一行；沒有空行時，從 PDF 複製的斷行文字會接回段落。請不要貼上學生姓名或其他個人資料。",
  pasteRows = 8,
}: {
  choice: MaterialChoice;
  onChange: (next: MaterialChoice) => void;
  pasteHint?: string;
  pasteRows?: number;
}) {
  const ids = useId();
  const { way, picked } = choice;
  return (
    <div className="material-chooser">
      <div className="material-ways" role="group" aria-label="教材從哪裡來">
        {MATERIAL_WAYS.map((w) => (
          <button
            key={w.key}
            type="button"
            className="way-button"
            aria-pressed={way === w.key}
            onClick={() => onChange({ ...choice, way: w.key, picked: null })}
          >
            {w.label}
          </button>
        ))}
      </div>

      {way !== "paste" &&
        (picked ? (
          <p className="ws-picked" role="status">
            教材：「{picked.title.trim() || "未命名教材"}」
            <button type="button" className="text-link is-plain" onClick={() => onChange({ ...choice, picked: null })}>
              換一份
            </button>
          </p>
        ) : (
          <div role="region" aria-label={way === "upload" ? "上傳檔案" : "從教材庫選"}>
            <LibraryStep key={way} way={way} onPicked={(m) => onChange({ ...choice, picked: m })} />
          </div>
        ))}

      {way === "paste" && (
        <div role="region" aria-label="貼上文字" className="material-paste">
          <label className="field-label" htmlFor={`${ids}-title`}>
            教材標題 <span className="optional-label">可以不填</span>
          </label>
          <input
            id={`${ids}-title`}
            type="text"
            value={choice.title}
            onChange={(e) => onChange({ ...choice, title: e.target.value })}
          />
          <label className="field-label" htmlFor={`${ids}-text`}>
            貼上教材文字
          </label>
          <textarea
            id={`${ids}-text`}
            className="print-input"
            rows={pasteRows}
            lang="en"
            value={choice.text}
            onChange={(e) => onChange({ ...choice, text: e.target.value })}
            aria-describedby={`${ids}-text-hint`}
          />
          <p className="field-hint" id={`${ids}-text-hint`}>
            {pasteHint}
          </p>
        </div>
      )}
    </div>
  );
}
