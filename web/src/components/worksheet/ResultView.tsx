import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import type {
  WsAudience,
  WsBlock,
  WsItem,
  WsItemTier,
  WsLevel,
  WsScaffold,
  WsSupportKind,
} from "../../api/types";
import {
  ANSWER_FORMATS,
  AUDIENCE_LABEL,
  BLOCK_KIND_LABEL,
  INFO_FUNCTION_LABEL,
  LEVEL_LABEL,
  SCAFFOLD_TYPE_LABEL,
  SUPPORT_KINDS_FOR,
  SUPPORT_KIND_LABEL,
  POS_OPTIONS,
  STEPS_LEAD,
  TIERED_FORMATS,
  buildSupport,
  editedOrigin,
  isFromManna,
  moveBlock,
  newBlock,
  newId,
  frameShown,
  newItem,
  itemHasAnswer,
  printedNumbers,
  projectSheet,
  renumberRefs,
  withoutOwnNumber,
  studentVersionName,
  toggleIn,
  versionCode,
  versionLabel,
  withAnswerFormat,
  type SheetBlock,
} from "../../lib/worksheet";
import {
  BLOOM_LABEL,
  BLOOM_NONE_LABEL,
  blockBloomLine,
  isBloom,
  matchesFilter,
  tally,
  type BloomFilter,
} from "../../lib/bloom";
import { useConfirmFocus } from "../../lib/useConfirmFocus";
import { holdsBlock, type SheetSelection } from "../../lib/blockRevise";
import { editorBlockText } from "../../lib/blockText";
import { needsMaterial } from "../../lib/templates";
import { flip } from "../../lib/flip";
import { prefersReducedMotion } from "../../lib/scroll";
import { ErrorText } from "../common";
import { MenuItem, SectionTools, menuButtonOf } from "../prep/SectionTools";
import { BloomDistribution, BloomSelect } from "./Bloom";
import { ChoicesArea, WritingArea, promptPlaceholder } from "./ItemAnswer";
import { BlockHistoryButton } from "./BlockHistory";
import { SheetText } from "./SheetText";
import { KeyVocabulary } from "./KeyVocabulary";
import { WordBankEditor } from "./WordBankEditor";
import {
  BlockReviseArea,
  LostNotes,
  ReviseByNotesButton,
  ReviseForm,
  useBlockRevise,
  type BlockRevise,
} from "./BlockRevise";
import { LessonLinkControls } from "./LessonLink";
import { OrganizerEditor } from "./Organizer";
import { printedOrganizer } from "../../lib/organizer";
import { RunOutcome, WorksheetRunProgress } from "./OptionsView";
import { GlossaryBox } from "./Glossary";
import { SlotPicture } from "./SlotPicture";
import { SopSyncNotice } from "./SopSync";
import type { WorksheetController } from "./useWorksheet";
import { Icon } from "../Icon";

function LevelChips({
  name,
  all,
  value,
  disabled,
  onChange,
}: {
  name: string;
  all: WsLevel[];
  value: WsLevel[];
  disabled: boolean;
  onChange: (next: WsLevel[]) => void;
}) {
  if (all.length < 2) return null;
  // The last version cannot be unticked: in none of them, the question (or scaffold) would vanish
  // from every tab with no way to get it back. 刪除 is the way to remove it.
  const shownIn = all.filter((l) => value.includes(l));
  return (
    <span className="ws-levels" role="group" aria-label={`${name}出現的版本`}>
      {all.map((level) => {
        const last = shownIn.length === 1 && shownIn[0] === level;
        return (
          <label key={level} title={last ? "至少要留在一個版本；不要了請按「刪除」。" : undefined}>
            <input
              type="checkbox"
              checked={value.includes(level)}
              disabled={disabled || last}
              aria-label={`${name}出現在${LEVEL_LABEL[level]}`}
              onChange={() => onChange(toggleIn(value, level, all))}
            />
            <span>{LEVEL_LABEL[level]}</span>
          </label>
        );
      })}
    </span>
  );
}

/** Lines typed one per line (blank lines dropped on save by the server's trim). */
const linesOf = (text: string) => text.split("\n").map((l) => l.trimStart());
/** A word bank typed with commas (or 、). */
const wordsOf = (text: string) => text.split(/[,、]/).map((w) => w.trimStart());

/** What a version's support looked like in each form it had this session, so choosing a form
 * again brings back its lines instead of making them anew (the word bank and model sentence are the
 * version's, whatever the form). Keyed `item|level|kind`. */
type SupportMemory = Map<string, string[]>;

const SUPPORT_PLACEHOLDER: Record<WsSupportKind, string> = {
  cloze: "The neighbors thought the ___ would be ___.",
  word_bank: "Use two of these words. (optional)",
  sentence_frame: "I think ___ because ___.",
  sentence_starter: "The writer thinks that …",
  none: "",
};

/** The forms that print a word bank (a starter does not, T3). */
const PRINTS_BANK = new Set<WsSupportKind>(["cloze", "word_bank", "sentence_frame"]);

/**
 * 差異化學習單: one question's part for the version shown — its support form, the lines printed
 * under the question, the word bank and the model sentence they are made from; in the teacher
 * view also this version's answer and scoring note. Choosing another form makes its support from
 * the model sentence and word bank ([`buildSupport`]); the lines stay editable.
 */
function TierEditor({
  itemId,
  tier,
  no,
  level,
  teacher,
  disabled,
  memory,
  onChange,
}: {
  itemId: string;
  tier: WsItemTier;
  no: number;
  level: WsLevel;
  teacher: boolean;
  disabled: boolean;
  memory: SupportMemory;
  onChange: (change: (t: WsItemTier) => WsItemTier) => void;
}) {
  const kinds = SUPPORT_KINDS_FOR[level].includes(tier.support_kind)
    ? SUPPORT_KINDS_FOR[level]
    : [tier.support_kind, ...SUPPORT_KINDS_FOR[level]];
  const name = LEVEL_LABEL[level];
  const kind = tier.support_kind;
  /** Why the support of the form just chosen could not be made (shown for that version and form). */
  const [unmade, setUnmade] = useState<{ level: WsLevel; kind: WsSupportKind; missing: "model_sentence" | "words" } | null>(
    null,
  );
  const key = (k: WsSupportKind) => `${itemId}|${level}|${k}`;

  /** Makes the support of `next` from the model sentence and bank; says so when it cannot. */
  const make = (next: WsSupportKind) => {
    const built = buildSupport(next, tier.model_sentence, tier.word_bank);
    if (built && !built.ok) {
      setUnmade({ level, kind: next, missing: built.missing });
      onChange((t) => ({ ...t, support_kind: next }));
      return;
    }
    setUnmade(null);
    onChange((t) => (built ? { ...t, support_kind: next, support: built.support, word_bank: built.word_bank } : { ...t, support_kind: next }));
  };
  const choose = (next: WsSupportKind) => {
    memory.set(key(kind), tier.support);
    const before = next === "none" ? undefined : memory.get(key(next));
    if (before) {
      setUnmade(null);
      onChange((t) => ({ ...t, support_kind: next, support: before }));
    } else make(next);
  };
  const note = unmade && unmade.level === level && unmade.kind === kind ? unmade : null;

  return (
    <div className="ws-hint ws-tier" lang="zh-Hant" role="group" aria-label={`${name}的支持 ${no}`}>
      <span>
        {name}的支持
        <select
          aria-label={`支持形式 ${no}`}
          value={kind}
          disabled={disabled}
          onChange={(e) => choose(e.target.value as WsSupportKind)}
        >
          {kinds.map((k) => (
            <option key={k} value={k}>
              {SUPPORT_KIND_LABEL[k]}
            </option>
          ))}
        </select>
      </span>
      <div className="ws-tier-body">
        {kind === "none" ? (
          <p className="quiet-note">不給支持：這個版本在題目下方只印作答處。</p>
        ) : (
          <>
            <SheetText
              label={`支持 ${no}（每行一項）`}
              value={tier.support.join("\n")}
              placeholder={SUPPORT_PLACEHOLDER[kind]}
              disabled={disabled}
              onChange={(text) => onChange((t) => ({ ...t, support: linesOf(text) }))}
            />
            {PRINTS_BANK.has(kind) ? (
              <SheetText
                label={`字庫 ${no}（用逗號分開）`}
                value={tier.word_bank.join(", ")}
                placeholder="Word bank"
                disabled={disabled}
                onChange={(text) => onChange((t) => ({ ...t, word_bank: wordsOf(text) }))}
              />
            ) : (
              tier.word_bank.some((w) => w.trim() !== "") && (
                <p className="quiet-note">字庫還留著，句首不印字庫：{tier.word_bank.join(", ")}</p>
              )
            )}
          </>
        )}
        {note && (
          <p className="ws-tier-note" role="status">
            {note.missing === "model_sentence"
              ? `這個版本沒有範句，沒辦法自動做成「${SUPPORT_KIND_LABEL[note.kind]}」；上面的支持照舊，可以直接改。也可以先寫範句，再按「依範句重做」。`
              : "沒有字庫也沒有範句，沒辦法自動做成字庫；請在字庫寫幾個字詞（用逗號分開），或先寫範句再按「依範句重做」。"}
          </p>
        )}
        {kind !== "none" && (
          <div className="ws-tier-model">
            <span>範句（不印出）</span>
            <SheetText
              label={`範句 ${no}（不印出）`}
              value={tier.model_sentence}
              placeholder="A complete model sentence"
              disabled={disabled}
              onChange={(model_sentence) => onChange((t) => ({ ...t, model_sentence }))}
            />
            <button
              type="button"
              className="text-link is-plain"
              aria-label={`依範句重做支持 ${no}`}
              disabled={disabled}
              onClick={() => make(kind)}
            >
              依範句重做
            </button>
          </div>
        )}
      </div>
      {teacher && (
        <div className="ws-answer">
          <div className="ws-answer-row">
            <span>參考答案（{name}）</span>
            <SheetText
              label={`參考答案 ${no}`}
              value={tier.answer}
              disabled={disabled}
              onChange={(answer) => onChange((t) => ({ ...t, answer }))}
            />
          </div>
          <div className="ws-answer-row">
            <span>評分說明（{name}）</span>
            <SheetText
              label={`評分說明 ${no}`}
              lang="zh-Hant"
              value={tier.scoring}
              disabled={disabled}
              onChange={(scoring) => onChange((t) => ({ ...t, scoring }))}
            />
          </div>
        </div>
      )}
    </div>
  );
}

interface BlockProps {
  ws: WorksheetController;
  view: SheetBlock;
  /** The version shown (a 差異化學習單 question edits this version's part). */
  level: WsLevel;
  index: number;
  /** Where 上移／下移 put the block: the place of the block shown before (after) it, so a move is
   * always visible, also while the Bloom filter hides some blocks; null at either end. */
  upTo: number | null;
  downTo: number | null;
  audience: WsAudience;
  locked: boolean;
  dragging: string | null;
  setDragging: (id: string | null) => void;
  /** Show only the items at this Bloom level (the distribution bar); null shows all. */
  filter: BloomFilter;
  /** 逐區修改與註記 */
  revise: BlockRevise;
  /** A 差異化學習單: a question added here gets its part in every version. */
  tiered: boolean;
  supportMemory: SupportMemory;
  /** The AI call that made this block, for 有幫助／不合用 (null: nothing to rate). */
  interactionOf: (block: WsBlock) => number | null;
  /** This block's 1-based place on the printed sheet: which part of a shared AI call a rating is about. */
  ratingNo: number;
  /** Manna changed this block a moment ago. */
  changed: boolean;
  /** 改這一區 asks the page's bottom bar (the prep conversation) instead of opening a form here. */
  blockTarget?: BlockTarget;
  targeted: boolean;
  flash: (ids: string[]) => void;
}

/** The prep page's bottom bar revises one block of this worksheet: 改這一區 points it at the block. */
export interface BlockTarget {
  blockId: string | null;
  onTarget: (blockId: string, region: string) => void;
}

/** One block's 字詞表: in its block, or (字詞表放在學習單最後) after the last block. */
function WordTable({
  ws,
  block,
  name,
  locked,
}: {
  ws: WorksheetController;
  block: WsBlock;
  name: string;
  locked: boolean;
}) {
  if (block.vocabulary.length === 0) return null;
  // The table's language is the worksheet's (one switch, next to 字詞表放在學習單最後).
  const zh = ws.state?.content.vocabulary_language === "zh";
  const patch = (change: (b: WsBlock) => WsBlock) =>
    ws.setContent((c) => ({
      ...c,
      blocks: c.blocks.map((b) => (b.id === block.id ? { ...change(b), origin: editedOrigin(b.origin) } : b)),
    }));
  return (
    <>
      <p className="ws-support-head" lang="zh-Hant">
        <span>字詞表</span>
        <button
          type="button"
          className="text-link is-plain"
          disabled={locked}
          aria-label={`刪除字詞表 ${name}`}
          // The rows are kept apart so 恢復字詞表 can bring them back.
          onClick={() => patch((b) => ({ ...b, vocabulary: [], vocabulary_removed: b.vocabulary }))}
        >
          刪除
        </button>
      </p>
      <KeyVocabulary
        label={`字詞表 ${name}`}
        extraHead={<span lang="zh-Hant">級別</span>}
        rows={block.vocabulary.map((v, i) => ({
          id: `${v.word}-${i}`,
          task: v.task_word === true,
          word: v.word,
          pos: (
            <select
              className="ws-pos"
              aria-label={`${v.word} 的詞性`}
              value={v.pos ?? ""}
              disabled={locked}
              onChange={(e) =>
                patch((b) => ({
                  ...b,
                  vocabulary: b.vocabulary.map((x, j) => (j === i ? { ...x, pos: e.target.value } : x)),
                }))
              }
            >
              <option value="">（未填）</option>
              {POS_OPTIONS.map((p) => (
                <option key={p} value={p}>
                  {p}
                </option>
              ))}
            </select>
          ),
          meaning: (
            <SheetText
              label={zh ? `${v.word} 的中文解釋` : `${v.word} 的解釋`}
              value={zh ? (v.meaning_zh ?? "") : v.definition}
              placeholder={zh ? `還沒有中文字義，先印英文：${v.definition}` : undefined}
              lang={zh ? "zh-Hant" : "en"}
              disabled={locked}
              onChange={(text) =>
                patch((b) => ({
                  ...b,
                  vocabulary: b.vocabulary.map((x, j) =>
                    j === i ? (zh ? { ...x, meaning_zh: text } : { ...x, definition: text }) : x,
                  ),
                }))
              }
            />
          ),
          extra: (
            <span lang="zh-Hant" className="ws-vocab-level">
              {v.task_word ? "指示用語" : v.level === null ? "級別未知" : `第 ${v.level} 級`}
            </span>
          ),
        }))}
      />
    </>
  );
}

/** 恢復字詞表: a block whose table was deleted keeps its rows; the server puts them back. */
function RestoreWordTable({
  ws,
  block,
  name,
  locked,
}: {
  ws: WorksheetController;
  block: WsBlock;
  name: string;
  locked: boolean;
}) {
  if (block.vocabulary.length > 0 || (block.vocabulary_removed ?? []).length === 0) return null;
  return (
    <p className="ws-support-head ws-vocab-restore" lang="zh-Hant">
      <span>字詞表已刪除</span>
      <button
        type="button"
        className="text-link is-plain is-gold"
        disabled={locked}
        aria-label={`恢復字詞表 ${name}`}
        onClick={() => void ws.restoreWordTable(block.id)}
      >
        恢復字詞表
      </button>
    </p>
  );
}

/** 字詞表放在學習單最後: the blocks' tables, edited here, printed after the last block. */
function EndWordTables({
  ws,
  views,
  locked,
}: {
  ws: WorksheetController;
  views: SheetBlock[];
  locked: boolean;
}) {
  const withWords = views.filter((v) => v.block.vocabulary.length > 0);
  if (withWords.length === 0) return null;
  return (
    <section className="ws-block ws-vocab-end" aria-label="字詞表（學習單最後）">
      <h3 lang="zh-Hant">字詞表</h3>
      {withWords.map((v) => (
        <WordTable
          key={v.block.id}
          ws={ws}
          block={v.block}
          name={v.label || BLOCK_KIND_LABEL[v.block.kind]}
          locked={locked}
        />
      ))}
    </section>
  );
}

function BlockSection({
  ws,
  view,
  level,
  index,
  upTo,
  downTo,
  audience,
  locked: editorLocked,
  dragging,
  setDragging,
  filter,
  revise,
  tiered,
  supportMemory,
  interactionOf,
  ratingNo,
  changed,
  blockTarget,
  targeted,
  flash,
}: BlockProps) {
  const { block, label } = view;
  // While Manna revises this block, or its proposal waits for 採用／放棄, the block stays as it is.
  const held = holdsBlock(revise.proposalOf(block.id));
  const locked = editorLocked || held;
  const [reviseOpen, setReviseOpen] = useState(false);
  const [draft, setDraft] = useState<SheetSelection | null>(null);
  const numbers = new Map(view.items.map(({ item, no }) => [item.id, no]));
  const printed = printedNumbers(view.items);
  /** What prints for a text that names questions by number, when it differs from the stored text. */
  const printedAs = (text: string) => {
    const out = renumberRefs(text, printed);
    return out !== text ? out : null;
  };
  /** A question prints without a number of its own at the start (the sheet numbers it). */
  const promptPrintedAs = (text: string) => {
    const out = renumberRefs(withoutOwnNumber(text), printed);
    return out !== text ? out : null;
  };
  const shownItems = view.items.filter(({ item }) => matchesFilter(item.bloom, filter));
  const bloomLine = blockBloomLine(view.items.map(({ item }) => item));
  const [confirmDelete, setConfirmDelete] = useState(false);
  const toolsRef = useRef<HTMLDivElement>(null);
  // Closing the question goes back to this block's 「更多…」 (the 刪除 item that asked is in its menu).
  const { safeRef: deleteSafeRef } = useConfirmFocus(
    confirmDelete,
    () => toolsRef.current?.querySelector<HTMLElement>("[aria-controls]") ?? null,
  );
  const [over, setOver] = useState(false);
  const levels = ws.state?.content.levels ?? [];
  const teacher = audience === "teacher";
  // 看答案: the student view can open one block's answers; they are drawn as the teacher view draws them.
  const [revealed, setRevealed] = useState(false);
  const answersId = useId();
  const hasAnswers = view.items.some(({ item }) => itemHasAnswer(item));
  const canReveal = !teacher && hasAnswers;
  const showAnswers = teacher || (revealed && hasAnswers);
  const name = label || BLOCK_KIND_LABEL[block.kind];
  /** How the bottom bar names it: 「B 區」 (the exit ticket has no letter). */
  const region = label ? `${label} 區` : BLOCK_KIND_LABEL[block.kind];
  const undoable = ws.state?.undoable.includes(block.id) ?? false;
  const toggleName = block.kind === "warmup" ? "暖身" : block.kind === "exit_ticket" ? "Exit ticket" : null;

  const patch = (change: (b: WsBlock) => WsBlock, touchOrigin = true) =>
    ws.setContent((c) => ({
      ...c,
      blocks: c.blocks.map((b) =>
        b.id === block.id ? { ...change(b), origin: touchOrigin ? editedOrigin(b.origin) : b.origin } : b,
      ),
    }));
  const patchItem = (id: string, change: (it: WsItem) => WsItem) =>
    patch((b) => ({
      ...b,
      items: b.items.map((it) => (it.id === id ? { ...change(it), origin: editedOrigin(it.origin) } : it)),
    }));
  const patchTier = (id: string, change: (t: WsItemTier) => WsItemTier) =>
    patchItem(id, (it) => ({
      ...it,
      tiers: (it.tiers ?? []).map((t) => (t.level === level ? change(t) : t)),
    }));
  const patchScaffold = (id: string, change: (s: WsScaffold) => WsScaffold) =>
    patch((b) => ({
      ...b,
      scaffolds: b.scaffolds.map((s) => (s.id === id ? { ...change(s), origin: editedOrigin(s.origin) } : s)),
    }));
  /** Whether the last click inside this block came from a pointer (keyboard activation has detail 0). */
  const pointerClick = useRef(false);
  const [moved, setMoved] = useState(false);
  useEffect(() => {
    if (!moved) return undefined;
    const timer = setTimeout(() => setMoved(false), MOVED_MS);
    return () => clearTimeout(timer);
  }, [moved]);
  // 上移／下移: the blocks that changed place slide there (pointer only; the keyboard gets the
  // result at once), and the moved block is kept in view. Reduced motion: lit for a moment instead.
  const move = (to: number) => {
    const reduced = prefersReducedMotion();
    const root = toolsRef.current?.closest(".ws-result") ?? document;
    flip({
      items: () => Array.from(root.querySelectorAll<HTMLElement>("[data-block]")),
      change: () =>
        flushSync(() => ws.setContent((c) => ({ ...c, blocks: moveBlock(c.blocks, block.id, to) }))),
      animate: pointerClick.current && !reduced,
    });
    if (reduced) setMoved(true);
    const el = document.getElementById(`sheet-block-${block.id}`);
    if (el && typeof el.scrollIntoView === "function") el.scrollIntoView({ block: "nearest" });
  };

  const procedural = view.scaffolds.filter((s) => s.type === "procedural");
  const supports = view.scaffolds.filter((s) => s.type !== "procedural");

  return (
    // oxlint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- drop target for the drag handle; 上移／下移 are the keyboard way
    <section
      id={`sheet-block-${block.id}`}
      data-block={block.id}
      className={`ws-block${isFromManna(block.origin) ? " is-manna" : ""}${over ? " is-drop" : ""}${changed || moved ? " is-changed" : ""}${targeted ? " is-targeted" : ""}`}
      aria-label={`區塊 ${name}`}
      onClickCapture={(e) => {
        pointerClick.current = e.detail > 0;
      }}
      onDragOver={(e) => {
        if (dragging && dragging !== block.id) {
          e.preventDefault();
          setOver(true);
        }
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        e.preventDefault();
        setOver(false);
        if (dragging && dragging !== block.id) {
          ws.setContent((c) => ({ ...c, blocks: moveBlock(c.blocks, dragging, index) }));
        }
        setDragging(null);
      }}
    >
      <div className="ws-tools" lang="zh-Hant" ref={toolsRef}>
        <span
          className="ws-drag"
          draggable={!locked}
          aria-hidden="true"
          title="拖曳調整順序"
          onDragStart={(e) => {
            e.dataTransfer?.setData("text/plain", block.id);
            setDragging(block.id);
          }}
          onDragEnd={() => setDragging(null)}
        >
          <Icon name="drag" className="is-glyph" />
        </span>
        <SectionTools
          name={`區塊 ${name}`}
          text={editorBlockText(view)}
          interactionId={interactionOf(block)}
          ratingTarget={{ itemIndex: ratingNo }}
          more={
            <>
              <MenuItem disabled={locked} onClick={() => void ws.regenerate(block.id)}>
                重新產生
              </MenuItem>
              {ws.state && (
                <BlockHistoryButton
                  worksheetId={ws.state.id}
                  blockId={block.id}
                  label={`修改歷程（區塊 ${name}）`}
                  className="popover-item"
                  disabled={locked}
                  focusAfter={menuButtonOf}
                  beforeOpen={ws.flush}
                  onAdopted={(done) => {
                    ws.adopt(done.worksheet);
                    flash([block.id]);
                  }}
                />
              )}
              {undoable && (
                <MenuItem disabled={locked} onClick={() => void ws.undo(block.id)}>
                  復原上一版
                </MenuItem>
              )}
              <MenuItem disabled={locked || upTo === null} onClick={() => upTo !== null && move(upTo)}>
                上移
              </MenuItem>
              <MenuItem disabled={locked || downTo === null} onClick={() => downTo !== null && move(downTo)}>
                下移
              </MenuItem>
              {toggleName && (
                <MenuItem disabled={locked} onClick={() => patch((b) => ({ ...b, enabled: false }), false)}>
                  關掉{toggleName === "暖身" ? toggleName : ` ${toggleName}`}
                </MenuItem>
              )}
              <MenuItem disabled={locked} onClick={() => setConfirmDelete(true)}>
                刪除
              </MenuItem>
            </>
          }
        >
          <button
            type="button"
            className="decision-button"
            aria-expanded={blockTarget ? undefined : reviseOpen}
            aria-pressed={blockTarget ? targeted : undefined}
            aria-label={`改這一區（區塊 ${name}）`}
            disabled={locked || revise.running}
            onClick={() => (blockTarget ? blockTarget.onTarget(block.id, region) : setReviseOpen((o) => !o))}
          >
            改這一區
          </button>
          <button
            type="button"
            className="decision-button"
            aria-label={`加註記（區塊 ${name}）`}
            disabled={editorLocked}
            onClick={() => setDraft({ block_id: block.id, field: "block", quote: "" })}
          >
            加註記
          </button>
          <ReviseByNotesButton blockId={block.id} ctl={revise} disabled={locked} />
        </SectionTools>
        {confirmDelete && (
          <span className="ws-confirm" role="group" aria-label="確認刪除">
            確定刪除這個區塊？
            <button
              type="button"
              className="text-link is-danger"
              disabled={locked}
              onClick={(e) => {
                // The block (and this button) goes away: focus the next block's heading, else the previous one.
                const heads = [...(e.currentTarget.closest("article")?.querySelectorAll<HTMLElement>(".ws-block-head") ?? [])];
                const mine = e.currentTarget.closest(".ws-block")?.querySelector<HTMLElement>(".ws-block-head");
                const at = mine ? heads.indexOf(mine) : -1;
                const next = at >= 0 ? (heads[at + 1] ?? heads[at - 1]) : undefined;
                ws.setContent((c) => ({ ...c, blocks: c.blocks.filter((b) => b.id !== block.id) }));
                if (next) setTimeout(() => next.focus(), 0);
              }}
            >
              確定刪除
            </button>
            <button type="button" className="text-link is-plain" ref={deleteSafeRef} onClick={() => setConfirmDelete(false)}>
              不刪
            </button>
          </span>
        )}
      </div>

      {reviseOpen && !held && (
        <ReviseForm name={`區塊 ${name}`} blockId={block.id} ctl={revise} onDone={() => setReviseOpen(false)} />
      )}
      <BlockReviseArea
        blockId={block.id}
        ctl={revise}
        numberOf={(id) => numbers.get(id)}
        draft={draft}
        onDraftDone={() => setDraft(null)}
      />

      <h3 className="ws-block-head" tabIndex={-1}>
        {label && <span className="ws-letter">{label}.</span>}
        <SheetText
          label={`區塊標題 ${name}`}
          className="is-title"
          note={{ block_id: block.id, field: "title", onNote: setDraft }}
          value={block.title}
          placeholder="Title"
          disabled={locked}
          onChange={(title) => patch((b) => ({ ...b, title }))}
        />
        {canReveal && (
          <button
            type="button"
            className="text-link is-plain ws-reveal"
            lang="zh-Hant"
            aria-expanded={showAnswers}
            aria-controls={answersId}
            aria-label={`${showAnswers ? "收起答案" : "看答案"} ${name}`}
            onClick={() => setRevealed((r) => !r)}
          >
            {showAnswers ? "收起答案" : "看答案"}
          </button>
        )}
      </h3>
      {teacher && bloomLine && (
        <p className="ws-bloom-line" lang="zh-Hant">
          {bloomLine}
        </p>
      )}
      {isFromManna(block.origin) && (
        <p className="manna-label ws-origin" lang="zh-Hant">
          <span className="star" aria-hidden="true" />
          {block.origin === "ai" ? "Manna 產生" : "Manna 產生，你改過"}
          {block.info_function ? ` · ${INFO_FUNCTION_LABEL[block.info_function]}` : ""}
        </p>
      )}
      {block.message &&
        (needsMaterial(block.message) ? (
          <div className="ws-message is-needs-material" lang="zh-Hant" role="note" aria-label="需要你提供素材">
            <p className="ws-message-label">需要你提供素材</p>
            <p>{block.message}</p>
          </div>
        ) : (
          <p className="ws-message" lang="zh-Hant" role="note">
            {block.message}
          </p>
        ))}
      {!view.printed && (
        <p className="quiet-note ws-not-printed" lang="zh-Hant">
          這個區塊目前沒有內容，印出與匯出時會略過；寫入題目或作答指示後就會出現在學習單上。
        </p>
      )}

      <SheetText
        label={`作答指示 ${name}`}
        className="is-steps"
        note={{ block_id: block.id, field: "instructions", onNote: setDraft }}
        value={block.instructions}
        placeholder="Instructions"
        disabled={locked}
        onChange={(instructions) => patch((b) => ({ ...b, instructions }))}
      />
      <PrintedAs text={printedAs(block.instructions)} />

      {(block.image_slots ?? []).map((slot) => {
        const live = ws.state?.images?.slots.find((s) => s.slot_id === slot.id);
        const id = ws.state?.id;
        if (id === undefined) return null;
        return (
          <SlotPicture
            key={slot.id}
            worksheetId={id}
            name={name}
            teacher
            view={{
              slotId: slot.id,
              // A slot without a picture row yet: an empty box for the teacher.
              status: live?.status ?? "skipped",
              imageId: live?.image_id ?? null,
              alt: slot.alt_en,
              caption: slot.caption,
              aiGenerated: live?.status === "done" && live.source === "generated",
              purpose: slot.purpose_zh,
              message: live?.message ?? null,
              kind: slot.kind,
            }}
            actions={{
              worksheetId: id,
              images: ws.state?.images ?? null,
              onImages: ws.setImages,
              disabled: locked,
            }}
          />
        );
      })}

      {procedural.map((s) => (
        <div key={s.id} className="ws-procedure" role="group" aria-label={`程序鷹架 ${name}`}>
          <p className="ws-support-head" lang="zh-Hant">
            <span className={s.auto ? "ws-auto" : ""}>
              {SCAFFOLD_TYPE_LABEL.procedural}
              {s.auto ? " · 自動產生" : ""}
            </span>
            <LevelChips
              name="這個鷹架"
              all={levels}
              value={s.levels}
              disabled={locked}
              onChange={(next) => patchScaffold(s.id, (x) => ({ ...x, levels: next }))}
            />
            <button
              type="button"
              className="text-link is-plain"
              disabled={locked}
              aria-label="刪除程序鷹架"
              onClick={() => patch((b) => ({ ...b, scaffolds: b.scaffolds.filter((x) => x.id !== s.id) }))}
            >
              刪除
            </button>
          </p>
          {!s.heading?.trim() && <p className="ws-steps-lead">{STEPS_LEAD}</p>}
          <ol>
            {s.steps.map((stepText, i) => (
              // oxlint-disable-next-line react/no-array-index-key -- plain strings, edited in place
              <li key={i}>
                <SheetText
                  label={`步驟 ${i + 1}（${name}）`}
                  value={stepText}
                  disabled={locked}
                  onChange={(text) =>
                    patchScaffold(s.id, (x) => ({ ...x, steps: x.steps.map((v, j) => (j === i ? text : v)) }))
                  }
                />
                <PrintedAs text={printedAs(stepText)} />
              </li>
            ))}
          </ol>
        </div>
      ))}

      {!ws.state?.content.vocabulary_at_end && <WordTable ws={ws} block={block} name={name} locked={locked} />}
      <RestoreWordTable ws={ws} block={block} name={name} locked={locked} />

      {supports.map((s) => (
        <div key={s.id} className="ws-support" role="group" aria-label={`鷹架 ${s.kind}`}>
          <p className="ws-support-head" lang="zh-Hant">
            <span>
              {SCAFFOLD_TYPE_LABEL[s.type]} · {s.kind}
            </span>
            <LevelChips
              name="這個鷹架"
              all={levels}
              value={s.levels}
              disabled={locked}
              onChange={(next) => patchScaffold(s.id, (x) => ({ ...x, levels: next }))}
            />
            <button
              type="button"
              className="text-link is-plain"
              disabled={locked}
              aria-label={`刪除鷹架 ${s.kind}`}
              onClick={() => patch((b) => ({ ...b, scaffolds: b.scaffolds.filter((x) => x.id !== s.id) }))}
            >
              刪除
            </button>
          </p>
          {s.bank ? (
            <WordBankEditor ws={ws} scaffold={s} bank={s.bank} name={name} locked={locked} onChange={(change) => patchScaffold(s.id, change)} />
          ) : (
            <SheetText
              label={`${s.kind}（${name}）`}
              value={s.content}
              disabled={locked}
              onChange={(text) => patchScaffold(s.id, (x) => ({ ...x, content: text }))}
            />
          )}
        </div>
      ))}

      {showAnswers && !teacher && (
        <p className="ws-answer-note" lang="zh-Hant" role="note">
          參考答案（只有你看得到）：印出與匯出的學生版不含這些答案。
        </p>
      )}
      <ol className="ws-items" id={answersId}>
        {shownItems.map(({ item, no, lines }) => {
          const tier = item.tiers?.find((t) => t.level === level);
          const differentiated = (item.tiers ?? []).length > 0;
          // A 差異化學習單 question takes the open formats only (its support goes with them).
          const formats: readonly string[] = differentiated
            ? [...TIERED_FORMATS, ...(item.answer_format && !TIERED_FORMATS.some((f) => f === item.answer_format) ? [item.answer_format] : [])]
            : ANSWER_FORMATS;
          // The hint prints in 基礎版 only: offered where it can print, kept visible where it cannot.
          const hintPrints = levels.includes("basic") && item.levels.includes("basic");
          const hint = item.hint ?? "";
          const hintLabel = hintPrints ? "提示（只印在基礎版）" : "提示（只印在基礎版；這題不在基礎版，不會印出）";
          const changeItem = (change: (it: WsItem) => WsItem) => patchItem(item.id, change);
          // The question's own sentence frame: offered under the open formats where it prints.
          const frame = item.frame ?? "";
          const framePrints = frameShown(level, levels);
          const frameLabel = framePrints ? "句型框架" : "句型框架（這個版本不印）";
          const showFrame =
            !tier && (frame.trim() !== "" || (framePrints && (item.answer_format === "簡答題" || item.answer_format === "段落寫作")));
          const organizer = printedOrganizer(item);
          return (
            <li key={item.id} value={no}>
              <SheetText
                label={`題目 ${no}`}
                note={{ block_id: block.id, field: "item", item_id: item.id, onNote: setDraft }}
                value={item.prompt}
                placeholder={promptPlaceholder(item.answer_format)}
                disabled={locked}
                onChange={(prompt) => changeItem((it) => ({ ...it, prompt }))}
              />
              <PrintedAs text={promptPrintedAs(item.prompt)} />
              <ChoicesArea item={item} no={no} disabled={locked} onChange={changeItem} />
              {tier ? (
                <TierEditor
                  itemId={item.id}
                  tier={tier}
                  no={no}
                  level={level}
                  teacher={showAnswers}
                  disabled={locked}
                  memory={supportMemory}
                  onChange={(change) => patchTier(item.id, change)}
                />
              ) : (
                (hintPrints || hint.trim() !== "") && (
                  <div className="ws-hint" lang="zh-Hant">
                    <span>{hintLabel}</span>
                    <SheetText
                      label={`${hintLabel} ${no}`}
                      note={{ block_id: block.id, field: "item", item_id: item.id, offsets: false, onNote: setDraft }}
                      value={hint}
                      placeholder="Hint: …"
                      disabled={locked}
                      onChange={(text) => changeItem((it) => ({ ...it, hint: text }))}
                    />
                  </div>
                )
              )}
              {showFrame && (
                <div className="ws-hint ws-frame" lang="zh-Hant">
                  <span>{frameLabel}</span>
                  <SheetText
                    label={`${frameLabel} ${no}`}
                      note={{ block_id: block.id, field: "item", item_id: item.id, offsets: false, onNote: setDraft }}
                    value={frame}
                    placeholder="Hello, everyone. My answer is ___ because ___. Thank you."
                    disabled={locked}
                    onChange={(text) => changeItem((it) => ({ ...it, frame: text }))}
                  />
                  {frame.trim() !== "" && (
                    <button
                      type="button"
                      className="text-link is-plain"
                      disabled={locked}
                      aria-label={`刪除句型框架 ${no}`}
                      onClick={() => changeItem((it) => ({ ...it, frame: "" }))}
                    >
                      刪除
                    </button>
                  )}
                </div>
              )}
              {/* 圖像組織 with an organizer: the chart, edited in place; every other format (and an
                  organizer question without one, which prints a box) answers as ItemAnswer draws it. */}
              {organizer ? (
                <OrganizerEditor
                  org={organizer}
                  no={no}
                  level={level}
                  levels={levels}
                  perVersion={differentiated}
                  teacher={showAnswers}
                  disabled={locked}
                  onChange={(org) => changeItem((it) => ({ ...it, organizer: org }))}
                />
              ) : (
                <WritingArea item={item} no={no} lines={lines} disabled={locked} onChange={changeItem} />
              )}
              {showAnswers && (
                <div className="ws-answer" lang="zh-Hant">
                  {/* A 差異化學習單 question has its answer per version (in TierEditor). */}
                  {!tier && (
                    <div className="ws-answer-row">
                      <span>參考答案</span>
                      <SheetText
                        label={`參考答案 ${no}`}
                      note={{ block_id: block.id, field: "item", item_id: item.id, offsets: false, onNote: setDraft }}
                        value={item.answer}
                        disabled={locked}
                        onChange={(answer) => changeItem((it) => ({ ...it, answer }))}
                      />
                    </div>
                  )}
                  <div className="ws-answer-row">
                    <span>依據（教材原文）</span>
                    <SheetText
                      label={`依據 ${no}`}
                      note={{ block_id: block.id, field: "item", item_id: item.id, offsets: false, onNote: setDraft }}
                      value={item.evidence}
                      disabled={locked}
                      onChange={(evidence) => changeItem((it) => ({ ...it, evidence }))}
                    />
                  </div>
                </div>
              )}
              <p className="ws-item-tools" lang="zh-Hant">
                <label>
                  <span className="sr-only">作答方式 {no}</span>
                  <select
                    aria-label={`作答方式 ${no}`}
                    value={item.answer_format ?? ""}
                    disabled={locked}
                    onChange={(e) => changeItem((it) => withAnswerFormat(it, e.target.value === "" ? null : e.target.value))}
                  >
                    {(!differentiated || item.answer_format === null) && <option value="">未指定</option>}
                    {formats.map((f) => (
                      <option key={f} value={f}>
                        {f}
                      </option>
                    ))}
                  </select>
                </label>
                <BloomSelect
                  label={`Bloom 層次 ${no}`}
                  value={item.bloom}
                  disabled={locked}
                  onChange={(bloom) => changeItem((it) => ({ ...it, bloom }))}
                />
                <LevelChips
                  name={`題目 ${no} `}
                  all={levels}
                  value={item.levels}
                  disabled={locked}
                  onChange={(next) => changeItem((it) => ({ ...it, levels: next }))}
                />
                <button
                  type="button"
                  className="text-link is-plain"
                  aria-label={`註記題目 ${no}`}
                  disabled={editorLocked}
                  onClick={() => setDraft({ block_id: block.id, field: "item", item_id: item.id, quote: "" })}
                >
                  註記
                </button>
                <button
                  type="button"
                  className="text-link is-plain"
                  aria-label={`刪除題目 ${no}`}
                  disabled={locked}
                  onClick={() => patch((b) => ({ ...b, items: b.items.filter((it) => it.id !== item.id) }))}
                >
                  刪除
                </button>
              </p>
            </li>
          );
        })}
      </ol>
      <p className="ws-add" lang="zh-Hant">
        <button
          type="button"
          className="text-link is-plain"
          disabled={locked}
          // While filtering, a new item starts at the level shown, so it does not vanish.
          onClick={() =>
            patch((b) => ({
              ...b,
              items: [...b.items, { ...newItem(levels, tiered), bloom: isBloom(filter) ? filter : null }],
            }))
          }
        >
          加一題
        </button>
      </p>

      <GlossaryBox
        glosses={(block.glosses ?? []).filter((g) => g.levels.includes(level))}
        name={name}
        disabled={locked}
        onRemove={(word) =>
          patch((b) => ({
            ...b,
            glosses: (b.glosses ?? [])
              .map((g) => (g.word === word ? { ...g, levels: g.levels.filter((l) => l !== level) } : g))
              .filter((g) => g.levels.length > 0),
          }))
        }
      />
      {teacher && (
        <div className="ws-answer ws-notes" lang="zh-Hant">
          <div className="ws-answer-row">
            <span>教師備註</span>
            <SheetText
              label={`教師備註 ${name}`}
              lang="zh-Hant"
              value={block.teacher_notes}
              disabled={locked}
              onChange={(teacher_notes) => patch((b) => ({ ...b, teacher_notes }))}
            />
          </div>
        </div>
      )}
    </section>
  );
}

/** The full page's file name for the exports (the menu itself is in the workspace's toolbar). */
function ExportFilename({ ws, level }: { ws: WorksheetController; level: WsLevel }) {
  const code = versionCode(ws.state?.content.levels ?? [], level, ws.state?.content.student_order);
  const id = useId();
  const state = ws.state;
  if (!state) return null;
  return (
    <section className="ws-export" aria-label="匯出設定">
      {code && <p className="quiet-note">學生版印成「{studentVersionName(code)}」，不印程度名稱。</p>}
      <div className="ws-export-row">
        <label className="ws-filename" htmlFor={`${id}-fn`}>
          檔名
          <input id={`${id}-fn`} type="text" value={state.filename} onChange={(e) => ws.setFilename(e.target.value)} />
        </label>
      </div>
    </section>
  );
}

interface ResultViewProps {
  ws: WorksheetController;
  /** The version (程度) and the audience shown; the workspace's toolbar and dialogs follow the same ones. */
  level: WsLevel;
  onLevel: (level: WsLevel) => void;
  audience: WsAudience;
  /**
   * The editor sits beside the prep conversation (about half a screen): the tools that belong to
   * the full page (the file name, linking the lesson) are left to it.
   */
  embedded?: boolean;
  /** Something else (a chat turn) is changing the worksheet: everything is read-only meanwhile. */
  frozen?: boolean;
  /** The AI call that made a block (for 有幫助／不合用); without it nothing is rated. */
  interactionOf?: (block: WsBlock) => number | null;
  /** Blocks Manna changed in the last chat turn: lit up for a moment. */
  highlight?: ReadonlySet<string>;
  /** The prep conversation's bottom bar revises one block: 改這一區 points it at the block. */
  blockTarget?: BlockTarget;
  /** Changes when the bottom bar started a block revision (its proposal is read again). */
  reviseReloadKey?: number;
}

const NO_BLOCKS: ReadonlySet<string> = new Set();
const NO_INTERACTION = () => null;
/** How long a block stays lit after Manna changed it (採用, a restored step). */
const FLASH_MS = 4000;
/** How long a moved block stays lit when the movement itself is not played (reduced motion). */
const MOVED_MS = 1000;

/**
 * The generated worksheet, edited in place: the version tabs (when there are several), block tools,
 * quiet saving. Answers exist in the page only in the teacher view. The toolbar above it (audience,
 * display, export, history…) is the workspace's.
 */
export function ResultView({
  ws,
  level,
  onLevel,
  audience,
  embedded = false,
  frozen = false,
  interactionOf = NO_INTERACTION,
  highlight = NO_BLOCKS,
  blockTarget,
  reviseReloadKey,
}: ResultViewProps) {
  const state = ws.state;
  const levels = state?.content.levels ?? [];
  const [dragging, setDragging] = useState<string | null>(null);
  const [bloomFilter, setBloomFilter] = useState<BloomFilter>(null);
  // Blocks lit by what happened here (採用, 修改歷程), on top of the page's `highlight`.
  const [lit, setLit] = useState<ReadonlySet<string>>(NO_BLOCKS);
  useEffect(() => {
    if (lit.size === 0) return undefined;
    const timer = setTimeout(() => setLit(NO_BLOCKS), FLASH_MS);
    return () => clearTimeout(timer);
  }, [lit]);
  const flash = (ids: string[]) => setLit(new Set(ids));
  // 逐區修改與註記: saved first; 採用 hands back the worksheet with the new version.
  const revise = useBlockRevise(state && !state.running ? state.id : null, {
    beforeAction: ws.flush,
    reloadKey: reviseReloadKey,
    onApplied: (done) => {
      ws.adopt(done.worksheet);
      flash([done.proposal.block_id]);
    },
    versionNo: state?.version_no ?? null,
    onUndone: (next) => {
      ws.adopt(next);
      setLit(NO_BLOCKS);
    },
  });
  const [supportMemory] = useState<SupportMemory>(() => new Map());

  // The selected tab must be one of this worksheet's versions (they can change after a new run).
  const current: WsLevel = levels.includes(level) ? level : (levels[0] ?? "standard");

  if (!state) return null;
  const { content } = state;
  const locked = state.running || ws.busy || revise.busy || frozen;
  const sheet = projectSheet(content, current, audience);
  const views = new Map(sheet.blocks.map((b) => [b.block.id, b]));
  const needsCount = content.blocks.filter((b) => b.enabled && needsMaterial(b.message)).length;
  // The distribution of what this version prints. A filter on a level with nothing left is off.
  const bloomTally = tally(sheet.blocks.flatMap((b) => b.items.map(({ item }) => item)));
  const tagged = bloomTally.total - bloomTally.none;
  const filterCount =
    bloomFilter === null ? 0 : bloomFilter === "none" ? bloomTally.none : bloomTally.counts[bloomFilter];
  const filter: BloomFilter = filterCount > 0 ? bloomFilter : null;
  const tiered =
    state.settings.output === "tiered" || content.blocks.some((b) => b.items.some((i) => (i.tiers ?? []).length > 0));
  // The blocks drawn (filtering by Bloom level keeps only those with an item at that level; a
  // switched-off one is drawn as its line): 上移／下移 move past the neighbour that is drawn.
  const drawn = content.blocks
    .map((b, i) => ({ b, i }))
    .filter(({ b }) => {
      const view = views.get(b.id);
      return filter === null || (view?.items.some(({ item }) => matchesFilter(item.bloom, filter)) ?? false);
    })
    .map(({ i }) => i);
  // T9: the student sheet carries a neutral code, never the level's name.
  const code = versionCode(levels, current, state?.content.student_order);
  const sheetName =
    audience === "student" && code ? `${versionLabel(code)} ${AUDIENCE_LABEL.student}` : `${LEVEL_LABEL[current]} ${AUDIENCE_LABEL[audience]}`;

  return (
    <div className={`ws-result${embedded ? " is-embedded" : ""}`}>
      <SopSyncNotice ws={ws} />

      {/* One version only: no tab that says 「一般版」 for nothing. */}
      {levels.length > 1 && (
        <div className="ws-tabs" role="tablist" aria-label="版本">
          {levels.map((l) => (
            <button
              key={l}
              type="button"
              role="tab"
              aria-selected={l === current}
              className={l === current ? "is-current" : ""}
              onClick={() => onLevel(l)}
            >
              {LEVEL_LABEL[l]}
            </button>
          ))}
        </div>
      )}

      <WorksheetRunProgress state={state} onCancelled={() => void ws.refresh()} />
      {!state.running && <RunOutcome jobs={state.jobs} context={false} />}
      <ErrorText message={ws.actionError} />

      <LostNotes ctl={revise} />
      {needsCount > 0 && (
        <p className="ws-needs-summary" role="note">
          有 {needsCount} 個區塊需要你提供素材（影片、錄音、圖片或另一篇文章）。Manna 不會自己編造，請在發給學生前補上。
        </p>
      )}
      {content.notices.length > 0 && (
        <ul className="ws-notices" aria-label="產生時的提醒">
          {content.notices.map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      )}

      {tagged > 0 && (
        <section className="ws-bloom" aria-label="Bloom 層次">
          <p className="ws-sub">
            Bloom 層次 · {LEVEL_LABEL[current]}共 {bloomTally.total} 題
          </p>
          <BloomDistribution tally={bloomTally} filter={filter} onFilter={setBloomFilter} />
          {filter !== null && (
            <p className="quiet-note" role="status">
              只顯示「{filter === "none" ? BLOOM_NONE_LABEL : BLOOM_LABEL[filter]}」的題目（{filterCount} 題）。按「全部」看全部。
            </p>
          )}
        </section>
      )}

      <article className="ws" lang="en" aria-label={sheetName}>
        <SheetTitle value={state.title} onChange={ws.setTitle} />
        <p className="ws-meta">
          <span>Class ______</span>
          <span>No. ______</span>
          <span>Name ____________</span>
          {code && <span className="ws-version">{studentVersionName(code)}</span>}
        </p>

        {content.show_objectives && (
          <section className="ws-block ws-objectives" aria-label="學習目標">
            <h3>Today I can …</h3>
            <ol>
              {content.objectives.map((o, i) => (
                <li key={o.id}>
                  <SheetText
                    label={`學習目標 ${i + 1}`}
                    value={o.student_facing_en}
                    placeholder={o.text || "I can …"}
                    disabled={locked}
                    onChange={(text) =>
                      ws.setContent((c) => ({
                        ...c,
                        objectives: c.objectives.map((x) =>
                          x.id === o.id ? { ...x, student_facing_en: text, origin: editedOrigin(x.origin) } : x,
                        ),
                      }))
                    }
                  />
                  <button
                    type="button"
                    className="text-link is-plain"
                    lang="zh-Hant"
                    aria-label={`刪除學習目標 ${i + 1}`}
                    disabled={locked}
                    onClick={() =>
                      ws.setContent((c) => ({ ...c, objectives: c.objectives.filter((x) => x.id !== o.id) }))
                    }
                  >
                    刪除
                  </button>
                </li>
              ))}
            </ol>
            <p className="ws-add" lang="zh-Hant">
              <button
                type="button"
                className="text-link is-plain"
                disabled={locked}
                onClick={() =>
                  ws.setContent((c) => ({
                    ...c,
                    objectives: [
                      ...c.objectives,
                      { id: newId("o"), type: "content", text: "", student_facing_en: "", origin: "teacher", sop_id: null },
                    ],
                  }))
                }
              >
                加一項目標
              </button>
            </p>
          </section>
        )}

        {content.blocks.map((block, index) => {
          const view = views.get(block.id);
          // Filtering by level: only the blocks with an item at that level.
          if (filter !== null && !view?.items.some(({ item }) => matchesFilter(item.bloom, filter))) return null;
          if (!view) {
            const name = BLOCK_KIND_LABEL[block.kind];
            return (
              <section key={block.id} className="ws-block is-off" aria-label={`區塊 ${name}（已關掉）`} lang="zh-Hant">
                <p className="quiet-note">
                  {name}已關掉，不會印出。內容還留著。
                  <button
                    type="button"
                    className="text-link is-plain"
                    disabled={locked}
                    onClick={() =>
                      ws.setContent((c) => ({
                        ...c,
                        blocks: c.blocks.map((b) => (b.id === block.id ? { ...b, enabled: true } : b)),
                      }))
                    }
                  >
                    加回{block.kind === "exit_ticket" ? ` ${name}` : name}
                  </button>
                </p>
              </section>
            );
          }
          return (
            <BlockSection
              key={block.id}
              ws={ws}
              view={view}
              level={current}
              index={index}
              upTo={drawn[drawn.indexOf(index) - 1] ?? null}
              downTo={drawn[drawn.indexOf(index) + 1] ?? null}
              audience={audience}
              locked={locked}
              dragging={dragging}
              setDragging={setDragging}
              filter={filter}
              revise={revise}
              tiered={tiered}
              supportMemory={supportMemory}
              interactionOf={interactionOf}
              ratingNo={sheet.blocks.indexOf(view) + 1}
              changed={highlight.has(block.id) || lit.has(block.id)}
              blockTarget={blockTarget}
              targeted={blockTarget?.blockId === block.id}
              flash={flash}
            />
          );
        })}

        {content.vocabulary_at_end && filter === null && (
          <EndWordTables ws={ws} views={sheet.blocks} locked={locked} />
        )}

        <p className="ws-add" lang="zh-Hant">
          <button
            type="button"
            className="text-link is-plain"
            disabled={locked}
            onClick={() =>
              ws.setContent((c) => {
                const exit = c.blocks.findIndex((b) => b.kind === "exit_ticket");
                // Its question has its versions (差異化學習單) and, while filtering, the level shown,
                // so the new block does not vanish behind the filter.
                const added = {
                  ...newBlock(c.levels),
                  items: [{ ...newItem(c.levels, tiered), bloom: isBloom(filter) ? filter : null }],
                };
                const blocks = [...c.blocks];
                blocks.splice(exit < 0 ? blocks.length : exit, 0, added);
                return { ...c, blocks };
              })
            }
          >
            加一個區塊
          </button>
        </p>
      </article>

      {!embedded && <ExportFilename ws={ws} level={current} />}
      {!embedded && <LessonLinkControls ws={ws} />}
    </div>
  );
}

/**
 * The sheet numbers questions through the whole worksheet, but steps and instructions name them by
 * their number in the block (「item 2」). The print shows the sheet's number; the editor keeps the
 * stored text and says what will print.
 */
function PrintedAs({ text }: { text: string | null }) {
  if (!text) return null;
  return (
    <p className="ws-printed-as quiet-note" lang="zh-Hant">
      印出時：<span lang="en">{text}</span>
    </p>
  );
}

/**
 * The title on the sheet. It wraps the way the printed title does: a one-line field cut long titles
 * off at the pane's edge. Still one line of text: Enter adds no line break, and a pasted one becomes a
 * space. The height follows the text and the pane's width (the divider and the folded chat change it).
 */
function SheetTitle({ value, onChange }: { value: string; onChange: (title: string) => void }) {
  const ref = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return undefined;
    const fit = () => {
      el.style.height = "auto";
      if (el.scrollHeight > 0) el.style.height = `${el.scrollHeight}px`;
    };
    fit();
    if (typeof ResizeObserver !== "function") return undefined;
    let width = el.clientWidth;
    const observer = new ResizeObserver(() => {
      // Only a new width can change the line count; fitting changes the height, not the width.
      if (el.clientWidth === width) return;
      width = el.clientWidth;
      fit();
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [value]);
  return (
    <textarea
      ref={ref}
      className="ws-title"
      rows={1}
      aria-label="學習單標題"
      value={value}
      onChange={(e) => onChange(e.target.value.replace(/\s*\n\s*/g, " "))}
      onKeyDown={(e) => {
        // Not while an input method is composing: its Enter picks the characters.
        if (e.key === "Enter" && !e.nativeEvent.isComposing) e.preventDefault();
      }}
    />
  );
}
