import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Link, Navigate, useLocation, useNavigate, useParams, useSearchParams } from "react-router";
import { api, errorMessage } from "../api/client";
import type { CreatePrepSessionRequest } from "../api/types";
import { BloomPicker } from "../components/bloom/BloomPicker";
import {
  DEFAULT_PRESET,
  TOTAL_MAX,
  TOTAL_MIN,
  countsFor,
  countsLine,
  countsTotal,
  minutesLine,
  nonZeroCounts,
  type BloomCounts,
  type PresetId,
} from "../components/bloom/presets";
import { ErrorText } from "../components/common";
import { Icon } from "../components/Icon";
import { PICK_MATERIAL } from "../components/library/labels";
import { LibraryPicker, pickedOf } from "../components/library/LibraryPicker";
import { AttachDialog, MaterialChip, PrivacyReminder, choiceTitle } from "../components/material/AttachDialog";
import { NO_TEXT_LINE, materialFields, type MaterialChoice } from "../components/material/MaterialChooser";
import { WorksheetNotice, type MimicState } from "../components/material/WorksheetNotice";
import { OncePerPage } from "../components/OncePerPage";
import { usePopover } from "../components/Popover";
import { NO_CLASS_LABEL, useActiveClasses } from "../components/prep/ContextChips";
import { Stepper } from "../components/Stepper";
import {
  SampleChooser,
  sampleFields,
  sampleName,
  templateSample,
  type Sample,
} from "../components/template/SampleChooser";
import { TemplateSelect, useReadyTemplates } from "../components/template/TemplateSelect";
import { useApp } from "../context/AppContext";
import {
  EXEMPLAR_TEXT,
  FORMAT_CHOICES,
  LESSON_TEXT_HEADING,
  MATERIAL_REQUIRED,
  MIMIC_LINK,
  NEED_LESSON_TEXT,
  NEED_SAMPLE,
  OBJECTIVE_MAX,
  SAMPLE_HEADING,
  SAMPLE_LINE,
  TEXT_MAX,
  TOOLS,
  TOOL_BLURB,
  TOOL_CHIP,
  TOOL_FIELDS,
  TOOL_NAME,
  TOOL_SUBTITLE,
  asCreateRequest,
  defaultCount,
  isTool,
  kindOf,
  mimicPath,
  styleLine,
  type Tool,
} from "../lib/create";

export const OBJECTIVE_PLACEHOLDER = "例如：學生能分析作者如何用例子支持論點";

/** 「8 題 · 記憶 1・理解 2・應用 2・分析 2・評鑑 1 · 約 20 分鐘」 */
export function summaryLine(total: number, counts: BloomCounts): string {
  return [`${total} 題`, countsLine(counts), minutesLine(total)].filter(Boolean).join(" · ");
}

/**
 * The generator for one tool (like Eduaide's): 「來做一份 <tool ▾>」, a 內容 card (learning objective,
 * the article or topic, a file or a library material instead), a 設定 card with what this tool
 * takes, and a sticky bar with the summary and 產生. Switching tools keeps what was filled in.
 *
 * 照我的學習單做 (mimic) has its own cards: 你的學習單範本 (the sample), 這節課的課文 (the same
 * article field) and 設定 (班級／程度 and 學習目標); 產生 waits for both a sample and a text.
 */
function Generator({ tool }: { tool: Tool }) {
  const ids = useId();
  const navigate = useNavigate();
  const location = useLocation();
  const [params, setParams] = useSearchParams();
  const { currentClassId } = useApp();
  const classes = useActiveClasses();
  const templates = useReadyTemplates();
  const { open: switchOpen, toggle: toggleSwitch, close: closeSwitch, rootRef: switchRoot, triggerRef: switchTrigger, onBlur: switchBlur } =
    usePopover();
  const textRef = useRef<HTMLTextAreaElement>(null);
  const pickRef = useRef<HTMLButtonElement>(null);
  const backToPick = useCallback(() => pickRef.current, []);

  const [objective, setObjective] = useState("");
  const [text, setText] = useState("");
  const [picked, setPicked] = useState<MaterialChoice | null>(null);
  const [dialog, setDialog] = useState<"upload" | "library" | null>(null);
  /** undefined: follow the current class (if it is one of the teacher's). */
  const [classPick, setClassPick] = useState<number | null | undefined>(undefined);
  const [preset, setPreset] = useState<PresetId>(DEFAULT_PRESET);
  const [custom, setCustom] = useState<BloomCounts | null>(null);
  /** null: the tool's default (8, or 12 for 分層練習). */
  const [count, setCount] = useState<number | null>(null);
  const [formats, setFormats] = useState<string[]>([]);
  const [templateId, setTemplateId] = useState<number | null>(null);
  const [sample, setSample] = useState<Sample | null>(null);
  const [sampleError, setSampleError] = useState<string | null>(null);
  /** Why ?material=<id> could not fill the article slot. */
  const [materialError, setMaterialError] = useState<string | null>(null);
  /** Materials the teacher said to keep as the article (no worksheet notice for them). */
  const [keptAsText, setKeptAsText] = useState<ReadonlySet<number>>(() => new Set());
  const [needMaterial, setNeedMaterial] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fields = TOOL_FIELDS[tool];
  const defaultClass = classes?.some((c) => c.id === currentClassId) ? currentClassId : null;
  const classId = classPick === undefined ? defaultClass : classPick;
  const chosenTotal = count ?? defaultCount(tool);
  const counts = countsFor(preset, chosenTotal, custom);
  const total = preset === "custom" ? countsTotal(counts) : chosenTotal;
  const totalOff = fields.bloom && (total < TOTAL_MIN || total > TOTAL_MAX);

  const material = picked
    ? materialFields(picked)
    : materialFields({ way: "paste", picked: null, title: "", text });
  const missing = needMaterial && material === null;
  const mimic = tool === "mimic";
  /** 照我的學習單做 waits for a sample, then for the lesson text. */
  const needed = !mimic ? null : sample === null ? NEED_SAMPLE : material === null ? NEED_LESSON_TEXT : null;
  const pickedMaterial = picked?.picked ?? null;
  const worksheetLike =
    pickedMaterial?.looks_like_worksheet === true && !keptAsText.has(pickedMaterial.id) ? pickedMaterial : null;

  /** Takes a handled parameter out of the address (the rest stays). */
  const setParamsRef = useRef(setParams);
  useEffect(() => {
    setParamsRef.current = setParams;
  }, [setParams]);
  const drop = useCallback((name: string) => {
    setParamsRef.current(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.delete(name);
        return next;
      },
      { replace: true },
    );
  }, []);

  // 照這份的樣式做 lands here with ?sample=<material id>: that material becomes the sample (and
  // leaves the article slot), then the parameter leaves the address.
  const idParam = (name: string) => {
    const value = Number(params.get(name));
    return Number.isInteger(value) && value > 0 ? value : null;
  };
  const sampleParam = mimic ? idParam("sample") : null;
  const sampleState = (location.state as MimicState | null)?.sample;
  useEffect(() => {
    if (sampleParam === null) return undefined;
    let alive = true;
    const take = (m: { id: number; title: string }) => {
      setSample({ kind: "material", material: { id: m.id, title: m.title } });
      setSampleError(null);
      setPicked((p) => (p?.picked?.id === m.id ? null : p));
    };
    if (sampleState?.id === sampleParam) {
      take(sampleState);
      drop("sample");
      return undefined;
    }
    api
      .getLibraryMaterial(sampleParam)
      .then((m) => {
        if (!alive) return;
        if (m.text_status === "ready") take(m);
        else setSampleError(NO_TEXT_LINE);
        drop("sample");
      })
      .catch((err: unknown) => {
        if (!alive) return;
        setSampleError(errorMessage(err));
        drop("sample");
      });
    return () => {
      alive = false;
    };
  }, [sampleParam, sampleState, drop]);

  // 照這個範本做 (學習單範本) lands here with ?template=<id>: that template is the sample.
  const templateParam = mimic ? idParam("template") : null;
  useEffect(() => {
    if (templateParam === null) return undefined;
    let alive = true;
    api
      .getWorksheetTemplate(templateParam)
      .then((t) => {
        if (!alive) return;
        if (t.status === "ready") {
          setSample(templateSample(t));
          setSampleError(null);
        } else {
          setSampleError("這個範本還沒準備好（Manna 還在讀，或沒有讀成功），請換一份。");
        }
        drop("template");
      })
      .catch((err: unknown) => {
        if (!alive) return;
        setSampleError(errorMessage(err));
        drop("template");
      });
    return () => {
      alive = false;
    };
  }, [templateParam, drop]);

  // 用這篇出題 (教材參考, 我的教材) lands here with ?material=<id>: that material in the article slot.
  const materialParam = idParam("material");
  useEffect(() => {
    if (materialParam === null) return undefined;
    let alive = true;
    api
      .getLibraryMaterial(materialParam)
      .then((m) => {
        if (!alive) return;
        if (m.text_status === "ready") {
          setPicked({ way: "library", picked: pickedOf(m), title: "", text: "" });
          setNeedMaterial(false);
          setMaterialError(null);
        } else {
          setMaterialError(NO_TEXT_LINE);
        }
        drop("material");
      })
      .catch((err: unknown) => {
        if (!alive) return;
        setMaterialError(errorMessage(err));
        drop("material");
      });
    return () => {
      alive = false;
    };
  }, [materialParam, drop]);

  function chooseSample(next: Sample | null) {
    setSample(next);
    setSampleError(null);
    // The same material cannot be both the sample and the lesson text.
    if (next?.kind === "material") setPicked((p) => (p?.picked?.id === next.material.id ? null : p));
  }

  function choosePreset(next: PresetId) {
    // 自訂 starts from what the chosen preset gives now, so the teacher adjusts from there.
    if (next === "custom" && preset !== "custom") setCustom(counts);
    setPreset(next);
  }

  async function generate() {
    if (busy || totalOff || needed !== null) return;
    if (material === null) {
      setNeedMaterial(true);
      textRef.current?.focus();
      return;
    }
    const options: NonNullable<CreatePrepSessionRequest["options"]> = {};
    if (fields.bloom) {
      options.bloom_counts = nonZeroCounts(counts);
      options.question_count = total;
    }
    if (fields.formats && formats.length > 0) options.answer_formats = FORMAT_CHOICES.filter((f) => formats.includes(f));
    if (objective.trim()) options.objective = objective.trim();

    setBusy(true);
    setError(null);
    try {
      const session = await api.createPrepSession(
        asCreateRequest({
          kind: kindOf(tool),
          request: objective.trim() || TOOL_CHIP[kindOf(tool)],
          ...material,
          ...(classId !== null ? { class_id: classId } : {}),
          ...(fields.template && templateId !== null ? { template_id: templateId } : {}),
          ...(mimic && sample ? sampleFields(sample) : {}),
          ...(Object.keys(options).length > 0 ? { options } : {}),
        }),
      );
      navigate(`/prep/${session.id}`);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  const summary = mimic
    ? [sample ? styleLine(sampleName(sample)) : null, needed].filter(Boolean).join(" · ")
    : fields.bloom
      ? summaryLine(total, counts)
      : `${TOOL_NAME[tool]} · ${TOOL_BLURB[tool]}`;

  const objectiveField = (
    <>
      <label className="field-label" htmlFor={`${ids}-objective`}>
        學習目標（選填）
      </label>
      <input
        id={`${ids}-objective`}
        type="text"
        maxLength={OBJECTIVE_MAX}
        value={objective}
        placeholder={OBJECTIVE_PLACEHOLDER}
        disabled={busy}
        onChange={(e) => setObjective(e.target.value)}
      />
    </>
  );

  const exemplarButton = (
    <button
      type="button"
      className="text-link is-plain exemplar-button"
      disabled={busy}
      onClick={() => {
        setPicked(null);
        setText(EXEMPLAR_TEXT);
        setNeedMaterial(false);
      }}
    >
      用範例試試
    </button>
  );

  // The article (or topic): pasted text, or a file or library material as a chip instead.
  const articleField = (
    <>
      <div className={`compose-card is-text${missing ? " is-missing" : ""}`}>
        {picked ? (
          <div className="prompt-chips">
            <MaterialChip title={choiceTitle(picked)} disabled={busy} onRemove={() => setPicked(null)} />
          </div>
        ) : (
          <textarea
            id={`${ids}-text`}
            ref={textRef}
            className="prompt-input is-article"
            rows={8}
            lang="en"
            maxLength={TEXT_MAX}
            value={text}
            disabled={busy}
            placeholder="貼上課文或文章；段落之間空一行。"
            aria-labelledby={mimic ? `${ids}-lesson` : undefined}
            aria-describedby={`${ids}-count${missing ? ` ${ids}-missing` : ""}`}
            aria-invalid={missing || undefined}
            onChange={(e) => setText(e.target.value)}
          />
        )}
        <div className="prompt-toolbar">
          <button type="button" className="tool-button" disabled={busy} onClick={() => setDialog("upload")}>
            <Icon name="upload" />
            上傳檔案
          </button>
          <button
            ref={pickRef}
            type="button"
            className="tool-button"
            aria-haspopup="dialog"
            disabled={busy}
            onClick={() => setDialog("library")}
          >
            <Icon name="library" />
            {PICK_MATERIAL}
          </button>
          {!picked && (
            <span className="prompt-counter" id={`${ids}-count`}>
              {text.length}/{TEXT_MAX}
            </span>
          )}
        </div>
      </div>
      {missing && (
        <p className="error-text" role="alert" id={`${ids}-missing`}>
          {MATERIAL_REQUIRED}
        </p>
      )}
      <ErrorText message={materialError} />
      {worksheetLike && (
        <WorksheetNotice
          disabled={busy}
          onUse={() => {
            const state: MimicState = { sample: { id: worksheetLike.id, title: worksheetLike.title } };
            navigate(mimicPath(worksheetLike.id), { state });
          }}
          onKeep={() => setKeptAsText((kept) => new Set(kept).add(worksheetLike.id))}
        />
      )}
      {/* On 照我的學習單做 the reminder sits with the sample, where the upload is. */}
      {dialog === null && !mimic && <PrivacyReminder />}
    </>
  );

  const classField = (
    <div className="create-field">
      <label className="field-label" htmlFor={`${ids}-class`}>
        班級／程度
      </label>
      <select
        id={`${ids}-class`}
        className="create-select"
        value={classId === null ? "" : String(classId)}
        disabled={busy}
        onChange={(e) => setClassPick(e.target.value === "" ? null : Number(e.target.value))}
      >
        <option value="">{NO_CLASS_LABEL}</option>
        {(classes ?? []).map((c) => (
          <option key={c.id} value={c.id}>
            {c.name}
          </option>
        ))}
      </select>
    </div>
  );

  return (
    <OncePerPage>
      <div className="create-head">
        <div className="create-title popover-root" ref={switchRoot} onBlur={switchBlur}>
          <h1>
            {!mimic && "來做一份 "}
            <button
              ref={switchTrigger}
              type="button"
              className="tool-switch"
              aria-expanded={switchOpen}
              aria-controls={`${ids}-tools`}
              onClick={toggleSwitch}
            >
              {TOOL_NAME[tool]}
              <Icon name="chevron" />
            </button>
          </h1>
          {switchOpen && (
            <div className="popover is-tools" id={`${ids}-tools`} role="group" aria-label="換一種">
              {TOOLS.map((t) => (
                <Link
                  key={t}
                  className="popover-item"
                  to={`/create/${t}`}
                  aria-current={t === tool ? "page" : undefined}
                  onClick={closeSwitch}
                >
                  <Icon name={t} />
                  {TOOL_NAME[t]}
                </Link>
              ))}
            </div>
          )}
        </div>
        <p className="lead">{TOOL_SUBTITLE[tool]}</p>
      </div>

      {mimic ? (
        <>
          <section className="create-card" aria-labelledby={`${ids}-sample`}>
            <h2 id={`${ids}-sample`}>{SAMPLE_HEADING}</h2>
            <SampleChooser sample={sample} disabled={busy} onChange={chooseSample} />
            <ErrorText message={sampleError} />
            <p className="field-hint sample-line">{SAMPLE_LINE}</p>
            {dialog === null && <PrivacyReminder />}
          </section>

          <section className="create-card" aria-labelledby={`${ids}-lesson`}>
            <div className="create-card-head">
              <h2 id={`${ids}-lesson`}>{LESSON_TEXT_HEADING}</h2>
              {exemplarButton}
            </div>
            <div className="create-field">{articleField}</div>
          </section>
        </>
      ) : (
        <section className="create-card" aria-labelledby={`${ids}-content`}>
          <div className="create-card-head">
            <h2 id={`${ids}-content`}>內容</h2>
            {exemplarButton}
          </div>

          {objectiveField}

          <label className="field-label" htmlFor={`${ids}-text`}>
            文章或主題
          </label>
          {articleField}
        </section>
      )}

      <section className="create-card" aria-labelledby={`${ids}-settings`}>
        <h2 id={`${ids}-settings`}>設定</h2>

        {classField}

        {mimic && <div className="create-field">{objectiveField}</div>}

        {fields.bloom && (
          <div className="create-field">
            <span className="field-label" id={`${ids}-count-label`}>
              題數
            </span>
            {preset === "custom" ? (
              <p className="field-hint">依下面每個層次的題數合計：{total} 題</p>
            ) : (
              <Stepper label="題數" value={chosenTotal} min={TOTAL_MIN} max={TOTAL_MAX} disabled={busy} onChange={setCount} />
            )}
          </div>
        )}

        {fields.bloom && (
          <BloomPicker
            preset={preset}
            custom={counts}
            disabled={busy}
            onPreset={choosePreset}
            onCustom={(next) => setCustom(next)}
          />
        )}

        {fields.formats && (
          <fieldset className="create-field" disabled={busy}>
            <legend className="field-label">題型</legend>
            <div className="format-chips">
              {FORMAT_CHOICES.map((f) => (
                <button
                  key={f}
                  type="button"
                  className="output-chip is-small"
                  aria-pressed={formats.includes(f)}
                  onClick={() => setFormats((list) => (list.includes(f) ? list.filter((x) => x !== f) : [...list, f]))}
                >
                  {f}
                </button>
              ))}
            </div>
            <p className="field-hint">{formats.length === 0 ? "沒有選：自動（依每個層次適合的題型）" : `已選 ${formats.length} 種`}</p>
          </fieldset>
        )}

        {fields.template && (
          <div className="create-field">
            {templates && templates.length > 0 ? (
              <>
                <label className="field-label" htmlFor={`${ids}-template`}>
                  版型
                </label>
                <TemplateSelect
                  id={`${ids}-template`}
                  templates={templates}
                  value={templateId}
                  disabled={busy}
                  onChange={setTemplateId}
                />
              </>
            ) : (
              <p className="field-hint">
                還沒有版型。可以在 <Link to="/templates?tab=mine">學習單範本</Link> 上傳一份你的學習單。
              </p>
            )}
            <p className="field-hint mimic-link">
              <Link to="/create/mimic">{MIMIC_LINK}</Link>
            </p>
          </div>
        )}
      </section>

      <ErrorText message={error} />

      <div className="create-bar">
        <p className="create-summary" id={`${ids}-summary`} aria-live="polite">
          {summary}
        </p>
        <button
          type="button"
          className="primary-button"
          disabled={busy || totalOff || needed !== null}
          aria-describedby={needed ? `${ids}-summary` : undefined}
          onClick={() => void generate()}
        >
          {busy ? "產生中…" : "產生"}
        </button>
      </div>

      {dialog === "upload" && (
        <AttachDialog
          way="upload"
          onClose={() => setDialog(null)}
          onChoice={(next) => {
            setPicked(next);
            setNeedMaterial(false);
            setMaterialError(null);
            setDialog(null);
          }}
        />
      )}
      {dialog === "library" && (
        <LibraryPicker
          kind="material"
          returnFocus={backToPick}
          onClose={() => setDialog(null)}
          onPickMaterial={(card) => {
            setPicked({ way: "library", picked: pickedOf(card), title: "", text: "" });
            setNeedMaterial(false);
            setMaterialError(null);
            setDialog(null);
          }}
        />
      )}
    </OncePerPage>
  );
}

/** /create/:tool. An unknown tool opens 閱讀理解題. */
export function CreatePage() {
  const { tool } = useParams();
  if (!isTool(tool)) return <Navigate to="/create/comprehension" replace />;
  return <Generator tool={tool} />;
}
