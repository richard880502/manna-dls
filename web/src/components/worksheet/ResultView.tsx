import { useId, useState } from "react";
import { Link } from "react-router";
import type {
  WorksheetContent,
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
  AUTO_FIELD_LABEL,
  BLOCK_KIND_LABEL,
  INFO_FUNCTION_LABEL,
  LEVEL_LABEL,
  SCAFFOLD_TYPE_LABEL,
  SUPPORT_KINDS_FOR,
  SUPPORT_KIND_LABEL,
  TIERED_FORMATS,
  buildSupport,
  editedOrigin,
  isFromManna,
  moveBlock,
  newBlock,
  newId,
  newItem,
  projectSheet,
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
import { NOTES_HINT, holdsBlock, type SheetSelection } from "../../lib/blockRevise";
import { needsMaterial } from "../../lib/templates";
import { ErrorText } from "../common";
import { BloomDistribution, BloomSelect } from "./Bloom";
import { ChoicesArea, WritingArea, promptPlaceholder } from "./ItemAnswer";
import { BlockHistoryButton } from "./BlockHistory";
import { SheetText } from "./SheetText";
import {
  BlockReviseArea,
  LostNotes,
  ReviseByNotesButton,
  ReviseForm,
  useBlockRevise,
  type BlockRevise,
} from "./BlockRevise";
import { ExportMenu } from "./ExportMenu";
import { LessonLinkControls } from "./LessonLink";
import { OrganizerEditor } from "./Organizer";
import { printedOrganizer } from "../../lib/organizer";
import { RunOutcome, WorksheetRunProgress } from "./OptionsView";
import { GlossaryBox } from "./Glossary";
import { SlotPicture } from "./SlotPicture";
import { SopSyncNotice } from "./SopSync";
import type { WorksheetController } from "./useWorksheet";

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
      <table className="ws-vocab" aria-label={`字詞表 ${name}`}>
        <thead>
          <tr>
            <th scope="col">Word</th>
            <th scope="col">Meaning</th>
            <th scope="col" lang="zh-Hant">
              級別
            </th>
          </tr>
        </thead>
        <tbody>
          {block.vocabulary.map((v, i) => (
            <tr key={`${v.word}-${i}`}>
              <td>{v.word}</td>
              <td>
                <SheetText
                  label={`${v.word} 的解釋`}
                  value={v.definition}
                  disabled={locked}
                  onChange={(definition) =>
                    patch((b) => ({
                      ...b,
                      vocabulary: b.vocabulary.map((x, j) => (j === i ? { ...x, definition } : x)),
                    }))
                  }
                />
              </td>
              <td lang="zh-Hant" className="ws-vocab-level">
                {v.task_word ? "指示用語" : v.level === null ? "級別未知" : `第 ${v.level} 級`}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
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
}: BlockProps) {
  const { block, label } = view;
  // While Manna revises this block, or its proposal waits for 採用／放棄, the block stays as it is.
  const held = holdsBlock(revise.proposalOf(block.id));
  const locked = editorLocked || held;
  const [reviseOpen, setReviseOpen] = useState(false);
  const [draft, setDraft] = useState<SheetSelection | null>(null);
  const numbers = new Map(view.items.map(({ item, no }) => [item.id, no]));
  const shownItems = view.items.filter(({ item }) => matchesFilter(item.bloom, filter));
  const bloomLine = blockBloomLine(view.items.map(({ item }) => item));
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [over, setOver] = useState(false);
  const levels = ws.state?.content.levels ?? [];
  const teacher = audience === "teacher";
  const name = label || BLOCK_KIND_LABEL[block.kind];
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
  const move = (to: number) => ws.setContent((c) => ({ ...c, blocks: moveBlock(c.blocks, block.id, to) }));

  const procedural = view.scaffolds.filter((s) => s.type === "procedural");
  const supports = view.scaffolds.filter((s) => s.type !== "procedural");

  return (
    // oxlint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- drop target for the drag handle; 上移／下移 are the keyboard way
    <section
      className={`ws-block${isFromManna(block.origin) ? " is-manna" : ""}${over ? " is-drop" : ""}`}
      aria-label={`區塊 ${name}`}
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
      <div className="ws-tools" lang="zh-Hant">
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
          ⠿
        </span>
        <button
          type="button"
          className="text-link is-plain"
          disabled={locked || upTo === null}
          onClick={() => upTo !== null && move(upTo)}
        >
          上移
        </button>
        <button
          type="button"
          className="text-link is-plain"
          disabled={locked || downTo === null}
          onClick={() => downTo !== null && move(downTo)}
        >
          下移
        </button>
        <button
          type="button"
          className="text-link is-plain is-gold"
          disabled={locked}
          onClick={() => void ws.regenerate(block.id)}
        >
          重新產生
        </button>
        {undoable && (
          <button
            type="button"
            className="text-link is-plain is-gold"
            disabled={locked}
            onClick={() => void ws.undo(block.id)}
          >
            復原上一版
          </button>
        )}
        {ws.state && (
          <BlockHistoryButton
            worksheetId={ws.state.id}
            blockId={block.id}
            label={`修改歷程（區塊 ${name}）`}
            className="text-link is-plain"
            disabled={locked}
            beforeOpen={ws.flush}
            onAdopted={(done) => ws.adopt(done.worksheet)}
          />
        )}
        <button
          type="button"
          className="text-link is-plain is-gold"
          aria-expanded={reviseOpen}
          aria-label={`改這一區（區塊 ${name}）`}
          disabled={locked || revise.running}
          onClick={() => setReviseOpen((o) => !o)}
        >
          改這一區
        </button>
        <button
          type="button"
          className="text-link is-plain"
          aria-label={`加註記（區塊 ${name}）`}
          disabled={editorLocked}
          onClick={() => setDraft({ block_id: block.id, field: "block", quote: "" })}
        >
          加註記
        </button>
        <ReviseByNotesButton blockId={block.id} ctl={revise} disabled={locked} />
        {toggleName && (
          <button
            type="button"
            className="text-link is-plain"
            disabled={locked}
            onClick={() => patch((b) => ({ ...b, enabled: false }), false)}
          >
            關掉{toggleName === "暖身" ? toggleName : ` ${toggleName}`}
          </button>
        )}
        {confirmDelete ? (
          <span className="ws-confirm" role="group" aria-label="確認刪除">
            確定刪除這個區塊？
            <button
              type="button"
              className="text-link is-danger"
              disabled={locked}
              onClick={() => ws.setContent((c) => ({ ...c, blocks: c.blocks.filter((b) => b.id !== block.id) }))}
            >
              確定刪除
            </button>
            <button type="button" className="text-link is-plain" onClick={() => setConfirmDelete(false)}>
              不刪
            </button>
          </span>
        ) : (
          <button type="button" className="text-link is-plain" disabled={locked} onClick={() => setConfirmDelete(true)}>
            刪除
          </button>
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

      <h3 className="ws-block-head">
        {label && <span className="ws-letter">{label}.</span>}
        <SheetText
          label={`區塊標題 ${name}`}
          className="is-title"
          value={block.title}
          placeholder="Title"
          disabled={locked}
          onChange={(title) => patch((b) => ({ ...b, title }))}
        />
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
        value={block.instructions}
        placeholder="Instructions"
        disabled={locked}
        onChange={(instructions) => patch((b) => ({ ...b, instructions }))}
      />

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
          <SheetText
            label={`${s.kind}（${name}）`}
            value={s.content}
            disabled={locked}
            onChange={(text) => patchScaffold(s.id, (x) => ({ ...x, content: text }))}
          />
        </div>
      ))}

      <ol className="ws-items">
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
          const organizer = printedOrganizer(item);
          return (
            <li key={item.id} value={no}>
              <SheetText
                label={`題目 ${no}`}
                value={item.prompt}
                placeholder={promptPlaceholder(item.answer_format)}
                disabled={locked}
                onChange={(prompt) => changeItem((it) => ({ ...it, prompt }))}
              />
              <ChoicesArea item={item} no={no} disabled={locked} onChange={changeItem} />
              {tier ? (
                <TierEditor
                  itemId={item.id}
                  tier={tier}
                  no={no}
                  level={level}
                  teacher={teacher}
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
                      value={hint}
                      placeholder="Hint: …"
                      disabled={locked}
                      onChange={(text) => changeItem((it) => ({ ...it, hint: text }))}
                    />
                  </div>
                )
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
                  teacher={teacher}
                  disabled={locked}
                  onChange={(org) => changeItem((it) => ({ ...it, organizer: org }))}
                />
              ) : (
                <WritingArea item={item} no={no} lines={lines} disabled={locked} onChange={changeItem} />
              )}
              {teacher && (
                <div className="ws-answer" lang="zh-Hant">
                  {/* A 差異化學習單 question has its answer per version (in TierEditor). */}
                  {!tier && (
                    <div className="ws-answer-row">
                      <span>參考答案</span>
                      <SheetText
                        label={`參考答案 ${no}`}
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

function ExportArea({ ws, level }: { ws: WorksheetController; level: WsLevel }) {
  const code = versionCode(ws.state?.content.levels ?? [], level, ws.state?.content.student_order);
  const ids = useId();
  const [error, setError] = useState<string | null>(null);
  const state = ws.state;
  if (!state) return null;

  return (
    <section className="ws-export" aria-label="匯出設定">
      {code && <p className="quiet-note">學生版印成「{studentVersionName(code)}」，不印程度名稱。</p>}
      <div className="ws-export-row">
        <ExportMenu
          worksheetId={state.id}
          level={level}
          levelLabel={(state.content.levels?.length ?? 0) > 1 ? LEVEL_LABEL[level] : null}
          disabled={state.running}
          beforeExport={() => ws.flush()}
          onError={setError}
          footnote="Word 可再編輯；參考答案只在教師版。"
        />
        <label className="ws-filename" htmlFor={`${ids}-fn`}>
          檔名
          <input id={`${ids}-fn`} type="text" value={state.filename} onChange={(e) => ws.setFilename(e.target.value)} />
        </label>
      </div>
      <ErrorText message={error} />
    </section>
  );
}

function chosenLine(content: WorksheetContent): string | null {
  const chosen = content.chosen;
  if (!chosen || chosen.auto.length === 0) return null;
  const parts = chosen.auto.map((field) => {
    const s = chosen.settings;
    const values =
      field === "levels"
        ? s.levels.map((l) => LEVEL_LABEL[l])
        : field === "minutes"
          ? [`主要任務約 ${chosen.item_budget} 題`]
          : s[field];
    return `${AUTO_FIELD_LABEL[field]}：${values.length > 0 ? values.join("、") : "依教材判斷"}`;
  });
  return `這次由 Manna 決定：${parts.join("；")}。`;
}

interface ResultViewProps {
  ws: WorksheetController;
  onAdjust: () => void;
}

/**
 * The generated worksheet, edited in place: version tabs (only the selected ones), student or
 * teacher view, block tools, quiet saving. Answers exist in the page only in the teacher view.
 */
export function ResultView({ ws, onAdjust }: ResultViewProps) {
  const state = ws.state;
  const levels = state?.content.levels ?? [];
  const [level, setLevel] = useState<WsLevel>(levels[0] ?? "standard");
  const [audience, setAudience] = useState<WsAudience>("student");
  const [dragging, setDragging] = useState<string | null>(null);
  const [bloomFilter, setBloomFilter] = useState<BloomFilter>(null);
  const radioName = useId();
  // 逐區修改與註記: saved first; 採用 hands back the worksheet with the new version.
  const revise = useBlockRevise(state && !state.running ? state.id : null, {
    beforeAction: ws.flush,
    onApplied: (done) => ws.adopt(done.worksheet),
    versionNo: state?.version_no ?? null,
    onUndone: (next) => ws.adopt(next),
  });
  const [supportMemory] = useState<SupportMemory>(() => new Map());

  // The selected tab must be one of this worksheet's versions (they can change after a new run).
  const current: WsLevel = levels.includes(level) ? level : (levels[0] ?? "standard");

  if (!state) return null;
  const { content } = state;
  const locked = state.running || ws.busy || revise.busy;
  const sheet = projectSheet(content, current, audience);
  const views = new Map(sheet.blocks.map((b) => [b.block.id, b]));
  const chosen = chosenLine(content);
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
    <div className="ws-result">
      <SopSyncNotice ws={ws} />

      <div className="ws-tabs" role="tablist" aria-label="版本">
        {levels.map((l) => (
          <button
            key={l}
            type="button"
            role="tab"
            aria-selected={l === current}
            className={l === current ? "is-current" : ""}
            onClick={() => setLevel(l)}
          >
            {LEVEL_LABEL[l]}
          </button>
        ))}
        <button type="button" className="text-link is-plain ws-adjust" onClick={onAdjust}>
          調整設定
        </button>
      </div>

      <div className="ws-tool-row">
        <div className="ws-seg" role="radiogroup" aria-label="顯示版本">
          {(["student", "teacher"] as const).map((a) => (
            <label key={a}>
              <input type="radio" name={radioName} checked={audience === a} onChange={() => setAudience(a)} />
              <span>{AUDIENCE_LABEL[a]}</span>
            </label>
          ))}
        </div>
        <label className="check-row ws-show-objectives">
          <input
            type="checkbox"
            checked={content.show_objectives}
            disabled={locked}
            onChange={(e) => ws.setContent((c) => ({ ...c, show_objectives: e.target.checked }))}
          />
          <span>顯示學習目標</span>
        </label>
        <label className="check-row ws-vocab-at-end">
          <input
            type="checkbox"
            checked={content.vocabulary_at_end === true}
            disabled={locked}
            onChange={(e) => ws.setContent((c) => ({ ...c, vocabulary_at_end: e.target.checked }))}
          />
          <span>字詞表放在學習單最後</span>
        </label>
        <Link className="text-link" to={`/worksheets/${state.id}/check`}>
          詞彙級別檢查
        </Link>
      </div>

      <WorksheetRunProgress state={state} />
      {!state.running && <RunOutcome jobs={state.jobs} />}
      <ErrorText message={ws.actionError} />

      <p className="manna-label ws-draft-note">
        <span className="star" aria-hidden="true" />
        Manna 產生的草稿。直接點文字就能改；每個區塊都可以移動、重新產生或刪除，也可以按「改這一區」或加註記，請 Manna 只改那一區。
        {audience === "teacher" ? "參考答案只在教師版。" : ""}
      </p>
      <p className="field-hint">{NOTES_HINT}</p>
      <LostNotes ctl={revise} />
      {chosen && <p className="quiet-note ws-chosen">{chosen}</p>}
      {content.chosen?.reasons.map((r) => (
        <p key={r} className="quiet-note">
          {r}
        </p>
      ))}
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
        <input
          className="ws-title"
          type="text"
          aria-label="學習單標題"
          value={state.title}
          onChange={(e) => ws.setTitle(e.target.value)}
        />
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

      <ExportArea ws={ws} level={current} />
      <LessonLinkControls ws={ws} />
    </div>
  );
}
