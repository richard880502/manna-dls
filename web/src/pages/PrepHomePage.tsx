import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { Link, useLocation, useNavigate } from "react-router";
import type { CreateCarry } from "./CreatePage";
import { api, errorMessage } from "../api/client";
import type { ObjectiveDraftKind, PrepSessionSummary } from "../api/types";
import { ErrorText } from "../components/common";
import { Icon } from "../components/Icon";
import {
  ATTACH_LABEL,
  AttachDialog,
  MaterialChip,
  PrivacyReminder,
  choiceTitle,
  type AttachWay,
} from "../components/material/AttachDialog";
import { materialFields, type MaterialChoice, type PickedMaterial } from "../components/material/MaterialChooser";
import { WorksheetNotice } from "../components/material/WorksheetNotice";
import {
  LongMaterialNotice,
  MaterialFixButtons,
  MaterialTextDialog,
  ParagraphPickerDialog,
  SelectionLine,
} from "../components/material/LongMaterial";
import { type DraftOutcome } from "../components/objectives/ObjectiveDrafts";
import { StartDrafts, useStartFlow } from "../components/start/useStartFlow";
import { StepwiseDialog } from "../components/home/StepwiseDialog";
import { OncePerPage } from "../components/OncePerPage";
import { usePopover } from "../components/Popover";
import { LibraryPicker, pickedOf } from "../components/library/LibraryPicker";
import {
  SAMPLE_ATTACH,
  sampleFields,
  sampleName,
  templateSample,
  type Sample,
  type SampleWay,
} from "../components/template/SampleChooser";
import {
  ATTACH_LESSON_TEXT,
  ATTACH_SAMPLE,
  LESSON_TEXT_CHIP_LABEL,
  MATERIAL_REQUIRED,
  QUICK_NEED_LESSON_TEXT,
  QUICK_NEED_SAMPLE,
  REQUEST_MAX,
  SAMPLE_CHIP_LABEL,
  TAKE_AS_SAMPLE,
  HOME_BLURB,
  HOME_NAME,
  HOME_TOOLS,
  STEPWISE_PLACEHOLDER,
  TOOL_CHIP,
  asCreateRequest,
  isStepwise,
  kindOf,
  toolFromWords,
  type HomeTool,
  type StartKind,
  type Tool,
} from "../lib/create";
import {
  CHANGE_MATERIAL,
  KEPT_LINE,
  RETRY,
  isLongMaterial,
  loadHomeDraft,
  rememberOrigin,
  saveHomeDraft,
  selectedLine,
  type HomeDraft,
  type HomeRestore,
} from "../lib/longMaterial";
import { objectiveOptions, type DraftDecision } from "../lib/objectives";
import { KIND_LABEL, sessionTitle } from "../lib/prep";
import { useAutoGrow } from "../lib/useAutoGrow";
import { formatDateTime } from "../lib/worksheet";
import { useDocumentTitle } from "../lib/useDocumentTitle";
import { MetaList } from "../components/MetaList";

const RECENT_SHOWN = 5;

export const HOME_HEADING = "今天想準備什麼？";
export const QUICK_PLACEHOLDER = "例如：B1 程度、8 題閱讀理解，記憶到評鑑都要有";
export const MIMIC_PLACEHOLDER = "例如：照這份的樣式，題目少一點、加一題寫作";

/** The latest few conversations, quietly, under the tools. */
function RecentSessions() {
  const [sessions, setSessions] = useState<PrepSessionSummary[] | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .listPrepSessions()
      .then((res) => alive && setSessions(res.sessions))
      // The list is a convenience: without it the page still works.
      .catch(() => alive && setSessions([]));
    return () => {
      alive = false;
    };
  }, []);
  if (!sessions || sessions.length === 0) return null;
  return (
    <section className="home-recent" aria-labelledby="home-recent-heading">
      <div className="head-row">
        <h2 id="home-recent-heading">最近的備課</h2>
        <Link className="text-link is-plain" to="/prep">
          全部
        </Link>
      </div>
      <ul className="rows is-compact">
        {sessions.slice(0, RECENT_SHOWN).map((s) => (
          <li key={s.id} className="row">
            <div>
              <h3>
                <Link to={`/prep/${s.id}`}>{sessionTitle(s)}</Link>
              </h3>
              <MetaList
                className="meta"
                items={[s.last_kind ? (KIND_LABEL[s.last_kind] ?? null) : null, formatDateTime(s.updated_at)]}
              />
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

const ARTICLE_WAYS: Array<[AttachWay, string]> = [
  ["upload", ATTACH_LABEL.upload],
  ["library", ATTACH_LABEL.library],
  ["paste", ATTACH_LABEL.paste],
];

type Popover = ReturnType<typeof usePopover>;

/** A 附上… button in the prompt card with its menu of ways (each way's icon is its own name). */
function AttachMenu<W extends AttachWay | SampleWay>({
  id,
  label,
  icon,
  ways,
  menu: { open, toggle, close, rootRef, triggerRef, onBlur },
  disabled,
  onPick,
}: {
  id: string;
  label: string;
  icon: "paperclip" | "template";
  ways: ReadonlyArray<readonly [W, string]>;
  menu: Popover;
  disabled: boolean;
  onPick: (way: W) => void;
}) {
  return (
    <div className="popover-root" ref={rootRef} onBlur={onBlur}>
      <button
        ref={triggerRef}
        type="button"
        className="tool-button"
        aria-expanded={open}
        aria-controls={id}
        disabled={disabled}
        onClick={toggle}
      >
        <Icon name={icon} />
        {label}
      </button>
      {open && (
        <div className="popover" id={id} role="group" aria-label={label}>
          {ways.map(([way, text]) => (
            <button
              key={way}
              type="button"
              className="popover-item"
              onClick={() => {
                close();
                onPick(way);
              }}
            >
              <Icon name={way} />
              {text}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

type QuickDialog =
  | { slot: "article"; way: AttachWay }
  | { slot: "sample"; way: SampleWay }
  | { slot: "stepwise" }
  | { slot: "edit-text"; id: number }
  | { slot: "pick-paragraphs"; id: number };

/** The id the long-material notice's 照樣產生 is remembered under for pasted text (it has no id yet). */
const PASTED_KEY = -1;

/**
 * The quick box: one composer box (text, 附上 controls, send). With a material it makes 閱讀理解題
 * unless a tile says otherwise; without one, a question to Manna. Typed words pick a tile until the
 * teacher picks one herself.
 *
 * 照我的學習單做 (the first tile) gives the card two slots, 附上學習單範本 and 附上課文, and sends a
 * 完整學習單 in the sample's style, as /create/mimic does. A worksheet put in the article slot under
 * another tile offers 把它當範本, which moves it into the 範本 slot here.
 */
function QuickBox() {
  const ids = useId();
  const navigate = useNavigate();
  const location = useLocation();
  // Coming back (browser Back, or 回上一步 on the conversation page): the box is as it was left.
  const [initial] = useState<HomeDraft | null>(
    () => (location.state as HomeRestore | null)?.restore ?? loadHomeDraft(location.key),
  );
  const [text, setText] = useState(initial?.text ?? "");
  const [choice, setChoice] = useState<MaterialChoice | null>(initial?.choice ?? null);
  /** 照我的學習單做's sample; kept while another tile is chosen, but sent only with it. */
  const [sample, setSample] = useState<Sample | null>(initial?.sample ?? null);
  /** null: the teacher has not chosen a tile, so typed words choose. */
  const [clicked, setClicked] = useState<{ tool: Tool | HomeTool | null } | null>(initial?.clicked ?? null);
  /** Materials the teacher said to keep as the article (no worksheet notice for them). */
  const [keptAsText, setKeptAsText] = useState<ReadonlySet<number>>(() => new Set(initial?.keptAsText ?? []));
  /** 只選要用的段落: the ids used of the attached library material (null: all of it). */
  const [selection, setSelection] = useState<string[] | null>(initial?.selection ?? null);
  /** The long-material notice was answered with 照樣產生 for this material (its id; pasted: -1). */
  const [longOk, setLongOk] = useState<number | null>(initial?.longOk ?? null);
  /** How many paragraphs pasted text would become (asked of the server, which owns the rule). */
  const [counted, setCounted] = useState<{ text: string; count: number } | null>(null);
  const [dialog, setDialog] = useState<QuickDialog | null>(null);
  /** 照我的學習單做 was sent without its sample or its lesson text. */
  const [needSlots, setNeedSlots] = useState(false);
  const articleMenu = usePopover();
  const sampleMenu = usePopover();
  const { triggerRef: articleTrigger } = articleMenu;
  const { triggerRef: sampleTrigger } = sampleMenu;
  const boxRef = useRef<HTMLTextAreaElement>(null);
  const tileRefs = useRef<Array<HTMLButtonElement | null>>([]);
  useAutoGrow(boxRef, text);

  const draftNow = useCallback(
    (): HomeDraft => ({ text, choice, sample, clicked, keptAsText: [...keptAsText], selection, longOk }),
    [text, choice, sample, clicked, keptAsText, selection, longOk],
  );
  // Kept per history entry: Back finds the box as it was, even after the page was left for a session.
  useEffect(() => {
    saveHomeDraft(location.key, draftNow());
  }, [location.key, draftNow]);

  const pastedText = choice?.way === "paste" ? choice.text : null;
  useEffect(() => {
    if (pastedText === null || !pastedText.trim()) return undefined;
    let alive = true;
    api
      .splitPreview(pastedText)
      .then((res) => alive && setCounted({ text: pastedText, count: res.paragraph_count }))
      // The notice is a convenience: without a count nothing is said.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [pastedText]);
  const pasteCount = pastedText !== null && counted?.text === pastedText ? counted.count : null;
  const backToArticle = useCallback(() => articleTrigger.current, [articleTrigger]);
  const backToSample = useCallback(() => sampleTrigger.current, [sampleTrigger]);

  const tile: Tool | HomeTool | null = clicked ? clicked.tool : toolFromWords(text);
  /** 我想逐步設計 is the lesson workspace's door, not a generator tool: it has no kind of its own. */
  const stepwise = isStepwise(tile);
  const tool: Tool | null = isStepwise(tile) ? null : tile;
  const mimic = tool === "mimic";
  const material = choice ? materialFields(choice) : null;
  const attachedKey = JSON.stringify([material, mimic && sample ? sampleFields(sample) : null]);
  // 送出 refused while a material is still being read is said beside the arrow, while the same ones are attached.
  const flow = useStartFlow(attachedKey);
  const { busy, setBusy, error, setError, ocrBusy, needMaterial, setNeedMaterial, drafts } = flow;
  const typed = text.trim();
  // No tile and no tool word: an attached article makes the first tile's 完整學習單; text alone asks Manna.
  const kind: StartKind | null = stepwise ? null : tool ? kindOf(tool) : material ? "worksheet" : typed ? "ask" : null;
  const missing = needMaterial && material === null && kind !== null && kind !== "ask";
  /** 照我的學習單做 waits for a sample, then for the lesson text. */
  const slotsNeeded = !mimic ? null : sample === null ? QUICK_NEED_SAMPLE : material === null ? QUICK_NEED_LESSON_TEXT : null;
  const need = (mimic ? (needSlots ? slotsNeeded : null) : missing ? MATERIAL_REQUIRED : null) ?? ocrBusy;
  const picked = choice?.picked ?? null;
  const paragraphCount = choice?.way === "paste" ? pasteCount : (picked?.paragraph_count ?? null);
  const longId = choice?.way === "paste" ? PASTED_KEY : (picked?.id ?? null);
  const longShown =
    !stepwise && selection === null && isLongMaterial(paragraphCount) && longId !== null && longOk !== longId;
  const worksheetLike =
    !mimic && !stepwise && picked?.looks_like_worksheet === true && !keptAsText.has(picked.id) ? picked : null;

  async function send() {
    // 我想逐步設計: the unit dialog next (the article and any typed name go along); nothing is required.
    if (stepwise) {
      if (!busy) setDialog({ slot: "stepwise" });
      return;
    }
    if (busy || kind === null || drafts.state !== null) return;
    if (mimic && slotsNeeded !== null) {
      setNeedSlots(true);
      return;
    }
    if (kind !== "ask" && material === null) {
      setNeedMaterial(true);
      return;
    }
    if (kind === "ask" || material === null) {
      await create("", "");
      return;
    }
    // A tool: the quick box has no objective fields, so Manna drafts them first (SIOP G2).
    const none = await flow.askDrafts({
      kind: kind as ObjectiveDraftKind,
      ...material,
      ...(selection && "material_id" in material ? { paragraph_ids: selection } : {}),
      ...(typed ? { request: typed } : {}),
    });
    if (none) await create("", "");
  }

  async function adoptDrafts(outcome: DraftOutcome, decisions: DraftDecision[]) {
    await flow.adopt(decisions);
    await create(outcome.content, outcome.language);
  }

  async function create(content: string, language: string) {
    if (kind === null) return;
    await flow.submit(async () => {
      const request = typed || (kind === "ask" ? "" : TOOL_CHIP[kind]);
      const options = {
        ...objectiveOptions(content, language),
        ...(selection && material && "material_id" in material ? { paragraph_ids: selection } : {}),
      };
      const session = await api.createPrepSession(
        asCreateRequest({
          kind,
          request,
          ...(material ?? {}),
          ...(mimic && sample ? sampleFields(sample) : {}),
          ...(Object.keys(options).length > 0 ? { options } : {}),
          entry: "home",
        }),
      );
      // The conversation's 回上一步 comes back to this box, filled in as it is now.
      rememberOrigin(session.id, draftNow());
      navigate(`/prep/${session.id}`);
    });
  }

  /** The attached material changed (another one, or none): what was chosen of the old one goes. */
  function changeChoice(next: MaterialChoice | null) {
    setChoice(next);
    setSelection(null);
    setLongOk(null);
  }

  /** 修改課文: pasted text in its own dialog; a library material (pasted before) in the text editor. */
  function openEdit() {
    if (choice?.way === "paste") setDialog({ slot: "article", way: "paste" });
    else if (picked) setDialog({ slot: "edit-text", id: picked.id });
  }

  /** 只選要用的段落: pasted text is stored in the library first (the paragraphs need ids). */
  async function openPick() {
    if (!choice) return;
    if (choice.way !== "paste") {
      if (picked) setDialog({ slot: "pick-paragraphs", id: picked.id });
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const made = await api.createLibraryMaterial({
        kind: "text",
        ...(choice.title.trim() ? { title: choice.title.trim() } : {}),
        text: choice.text,
      });
      setChoice({
        way: "library",
        picked: { id: made.id, title: made.title, paragraph_count: made.paragraphs.length },
        title: "",
        text: "",
      });
      setDialog({ slot: "pick-paragraphs", id: made.id });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  /** The text of the attached library material was edited: its paragraphs may be others now. */
  function textEdited(detail: { id: number; title: string; paragraphs: Array<{ id: string }> }) {
    setChoice((c) =>
      c?.picked?.id === detail.id
        ? { ...c, picked: { ...c.picked, title: detail.title, paragraph_count: detail.paragraphs.length } }
        : c,
    );
    setSelection((now) => {
      if (!now) return now;
      const still = now.filter((id) => detail.paragraphs.some((p) => p.id === id));
      return still.length > 0 ? still : null;
    });
    setError(null);
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    void send();
  }

  function pickTool(t: HomeTool) {
    setClicked({ tool: t });
    setNeedMaterial(false);
    setNeedSlots(false);
  }

  /** Arrow keys move through the tiles and choose as they go (a radiogroup). */
  function onTileKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    const forward = event.key === "ArrowRight" || event.key === "ArrowDown";
    const back = event.key === "ArrowLeft" || event.key === "ArrowUp";
    if (!forward && !back) return;
    event.preventDefault();
    const next = (index + (forward ? 1 : -1) + HOME_TOOLS.length) % HOME_TOOLS.length;
    tileRefs.current[next]?.focus();
    pickTool(HOME_TOOLS[next]!);
  }

  /**
   * 更多設定: the full form for the chosen tool. What is already attached here (the article, pasted
   * or picked, and 照我的學習單做's sample) goes along in the history state, so the form opens
   * filled in instead of asking for it again.
   */
  const fullFormPath = tool ? `/create/${tool}` : null;
  const carry: CreateCarry = { carry: { choice, sample: mimic ? sample : null } };

  function chooseSample(next: Sample) {
    setSample(next);
    setDialog(null);
    // The same material cannot be both the sample and the lesson text.
    if (next.kind === "material") setChoice((c) => (c?.picked?.id === next.material.id ? null : c));
  }

  /** 把它當範本: 照我的學習單做 with this material as the sample; the lesson text goes in next. */
  function takeAsSample(m: PickedMaterial) {
    setClicked({ tool: "mimic" });
    setSample({ kind: "material", material: { id: m.id, title: m.title } });
    changeChoice(null);
    setNeedMaterial(false);
    setNeedSlots(false);
    articleTrigger.current?.focus();
  }

  const articleLabel = mimic ? ATTACH_LESSON_TEXT : "附上文章";
  const shownSample = mimic ? sample : null;

  return (
    <>
      <section className="compose-card is-quick" aria-label="快速開始">
        <label className="sr-only" htmlFor={`${ids}-box`}>
          給 Manna 的要求
        </label>
        <textarea
          id={`${ids}-box`}
          ref={boxRef}
          className="prompt-input"
          rows={2}
          maxLength={REQUEST_MAX}
          value={text}
          placeholder={mimic ? MIMIC_PLACEHOLDER : stepwise ? STEPWISE_PLACEHOLDER : QUICK_PLACEHOLDER}
          disabled={busy}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
        />
        {(shownSample || choice) && (
          <div className="prompt-chips">
            {shownSample && (
              <MaterialChip
                label={SAMPLE_CHIP_LABEL}
                title={sampleName(shownSample)}
                meta={shownSample.kind === "template" ? `${shownSample.template.section_count} 區` : undefined}
                icon={shownSample.kind === "template" ? "template" : "file"}
                disabled={busy}
                onRemove={() => setSample(null)}
              />
            )}
            {choice && (
              <MaterialChip
                label={mimic ? LESSON_TEXT_CHIP_LABEL : undefined}
                title={choiceTitle(choice)}
                disabled={busy}
                onRemove={() => changeChoice(null)}
              />
            )}
          </div>
        )}
        <div className={`prompt-toolbar${mimic ? " has-two-slots" : ""}`}>
          {mimic && (
            <AttachMenu
              id={`${ids}-sample`}
              label={ATTACH_SAMPLE}
              icon="template"
              ways={SAMPLE_ATTACH}
              menu={sampleMenu}
              disabled={busy}
              onPick={(way) => setDialog({ slot: "sample", way })}
            />
          )}
          <AttachMenu
            id={`${ids}-attach`}
            label={articleLabel}
            icon="paperclip"
            ways={ARTICLE_WAYS}
            menu={articleMenu}
            disabled={busy}
            onPick={(way) => setDialog({ slot: "article", way })}
          />
          {need && (
            <p className={`prompt-need${mimic || need === ocrBusy ? " is-by-send" : ""}`} id={`${ids}-need`} role="alert">
              {need}
            </p>
          )}
          <button
            type="button"
            className="send-button"
            aria-label="送出"
            aria-describedby={need ? `${ids}-need` : undefined}
            disabled={busy || (kind === null && !stepwise) || drafts.state !== null}
            onClick={() => void send()}
          >
            <Icon name="send" />
          </button>
        </div>
      </section>

      {longShown && paragraphCount !== null && (
        <LongMaterialNotice
          paragraphs={paragraphCount}
          disabled={busy}
          onEdit={openEdit}
          onPick={() => void openPick()}
          onGo={() => {
            setLongOk(longId);
            void send();
          }}
        />
      )}
      {selection && picked && typeof picked.paragraph_count === "number" && (
        <SelectionLine disabled={busy} onChange={() => void openPick()}>
          {selectedLine(selection.length, picked.paragraph_count)}
        </SelectionLine>
      )}

      {worksheetLike && (
        <WorksheetNotice
          useLabel={TAKE_AS_SAMPLE}
          disabled={busy}
          onUse={() => takeAsSample(worksheetLike)}
          onKeep={() => setKeptAsText((kept) => new Set(kept).add(worksheetLike.id))}
        />
      )}

      <StartDrafts
        flow={flow}
        onUse={(outcome, decisions) => void adoptDrafts(outcome, decisions)}
        onSkip={() => void create("", "")}
      />

      <div className="tool-tiles" role="radiogroup" aria-label="要做什麼">
        {HOME_TOOLS.map((t, i) => (
          <button
            key={t}
            type="button"
            role="radio"
            aria-checked={tile === t}
            aria-labelledby={`${ids}-${t}-name`}
            aria-describedby={`${ids}-${t}-blurb`}
            tabIndex={tile === t || (i === 0 && !(HOME_TOOLS as readonly (Tool | HomeTool | null)[]).includes(tile)) ? 0 : -1}
            ref={(el) => {
              tileRefs.current[i] = el;
            }}
            className="tool-tile"
            disabled={busy}
            onClick={() => pickTool(t)}
            onKeyDown={(e) => onTileKeyDown(e, i)}
          >
            <span className="tool-card-icon" aria-hidden="true">
              <Icon name={t} />
            </span>
            <span className="tool-card-title" id={`${ids}-${t}-name`}>
              {HOME_NAME[t]}
            </span>
            <span className="tool-card-blurb" id={`${ids}-${t}-blurb`}>
              {HOME_BLURB[t]}
            </span>
          </button>
        ))}
      </div>
      <p className="home-others">
        閱讀理解題、差異化學習單、課堂活動、教學套件：學習單做好後，在對話裡接著做；也可以直接在上面的框寫出來。
      </p>
      {fullFormPath && (
        <p className="home-more">
          <Link className="text-link is-plain" to={fullFormPath} state={carry}>
            更多設定（班級、題數、Bloom、題型）
          </Link>
        </p>
      )}
      <ErrorText message={error} />
      {error && choice && (
        <div className="material-fix-note">
          <p className="quiet-note">{KEPT_LINE}</p>
          <MaterialFixButtons
            disabled={busy}
            onEdit={openEdit}
            onChange={() => setDialog({ slot: "article", way: "library" })}
            onRetry={() => void send()}
            retryLabel={RETRY}
          />
        </div>
      )}
      {dialog === null && <PrivacyReminder />}

      {dialog?.slot === "stepwise" && (
        <StepwiseDialog choice={choice} typed={typed} onClose={() => setDialog(null)} />
      )}
      {dialog?.slot === "article" && dialog.way === "library" && (
        <LibraryPicker
          kind="material"
          returnFocus={backToArticle}
          onClose={() => setDialog(null)}
          onPickMaterial={(card) => {
            changeChoice({ way: "library", picked: pickedOf(card), title: "", text: "" });
            setNeedMaterial(false);
            setDialog(null);
          }}
        />
      )}
      {dialog?.slot === "article" && dialog.way !== "library" && (
        <AttachDialog
          way={dialog.way}
          pasted={choice?.way === "paste" ? { title: choice.title, text: choice.text } : undefined}
          returnFocus={backToArticle}
          onClose={() => setDialog(null)}
          onChoice={(next) => {
            changeChoice(next);
            setError(null);
            setNeedMaterial(false);
            setDialog(null);
          }}
        />
      )}
      {dialog?.slot === "edit-text" && (
        <MaterialTextDialog
          materialId={dialog.id}
          from="home"
          returnFocus={backToArticle}
          onClose={() => setDialog(null)}
          onSaved={textEdited}
          otherWay={{ label: CHANGE_MATERIAL, onClick: () => setDialog({ slot: "article", way: "library" }) }}
        />
      )}
      {dialog?.slot === "pick-paragraphs" && (
        <ParagraphPickerDialog
          materialId={dialog.id}
          initial={selection}
          returnFocus={backToArticle}
          onClose={() => setDialog(null)}
          onSave={(chosen) => setSelection(chosen)}
        />
      )}
      {dialog?.slot === "sample" && dialog.way === "upload" && (
        <AttachDialog
          way="upload"
          purpose="worksheet_sample"
          returnFocus={backToSample}
          onClose={() => setDialog(null)}
          onChoice={(next) => {
            if (next.picked) chooseSample({ kind: "material", material: next.picked });
          }}
        />
      )}
      {dialog?.slot === "sample" && dialog.way === "template" && (
        <LibraryPicker
          kind="template"
          returnFocus={backToSample}
          onClose={() => setDialog(null)}
          onPickTemplate={(card) => chooseSample(templateSample(card))}
        />
      )}
    </>
  );
}

/**
 * 首頁: a greeting with the quick box, the three ways as one radiogroup of tiles under it (the other four
 * are offered in the conversation or by typing them; 更多設定 goes to the full form), and the latest prep sessions.
 */
export function PrepHomePage() {
  useDocumentTitle(HOME_HEADING);
  return (
    <OncePerPage>
      <div className="home-head">
        <h1>{HOME_HEADING}</h1>
        <p className="lead">附上一篇文章或教材，選要做什麼，Manna 幫你做出來。</p>
      </div>

      <QuickBox />

      <RecentSessions />
    </OncePerPage>
  );
}
