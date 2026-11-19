import { useCallback, useEffect, useId, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type {
  CreateWorksheetCommentRequest,
  ReviseWorksheetBlockRequest,
  UpdateWorksheetCommentRequest,
  PictureOutcome,
  WorksheetComment,
  WsBlock,
  WsItem,
  WsProposal,
  WsProposalAccepted,
  WsProposalChange,
  WorksheetState,
} from "../../api/types";
import {
  COMMENT_STATUS_LABEL,
  INSTRUCTION_MAX,
  NOTE_MAX,
  NOTE_PLACEHOLDER,
  REVISE_PLACEHOLDER,
  blockOfNote,
  blockReviseConfig,
  openAiNotes,
  placeOf,
  type NumberOf,
  type SheetSelection,
} from "../../lib/blockRevise";
import { clipQuote } from "../../lib/materials";
import { printedAnswer, printedOption } from "../../lib/worksheet";
import { ErrorText } from "../common";
import { PictureOutcomeList, PrepImageThumbs } from "../prep/ImageAttach";

// ---------------------------------------------------------------- state

export interface BlockReviseOptions {
  /** Runs before a revision is asked for or accepted (the editor saves first); a message stops it. */
  beforeAction?: () => Promise<string | null>;
  /** 採用 wrote a new version: the worksheet as it is now. */
  onApplied?: (accepted: WsProposalAccepted) => void;
  /** Changes when someone else started a revision (the prep page's bottom bar): load again. */
  reloadKey?: unknown;
  /** The worksheet's current version: 「已採用」 stays only while nothing else changed it. */
  versionNo?: number | null;
  /** 復原 (right after 採用) wrote a new version: the worksheet as it is now. */
  onUndone?: (state: WorksheetState) => void;
}

export interface BlockRevise {
  comments: WorksheetComment[];
  proposals: WsProposal[];
  /** Notes or proposals could not be loaded (an older server, a network hiccup). */
  loadFailed: boolean;
  /** The last action that failed, and the block it was about. */
  error: { blockId: string | null; message: string } | null;
  /** An action is being sent. */
  busy: boolean;
  /** A revision runs somewhere in the worksheet (only one at a time). */
  running: boolean;
  notesOf: (blockId: string) => WorksheetComment[];
  proposalOf: (blockId: string) => WsProposal | undefined;
  /** What 採用 did with the picture actions of a block (until the next action on it). */
  picturesOf: (blockId: string) => PictureOutcome[];
  /** 「已採用」: true right after 採用, until the block is changed again by anything. */
  justAccepted: (blockId: string) => boolean;
  /** 復原 of the 採用 just made (the block's text and pictures go back as they were). */
  undoAccepted: (blockId: string) => Promise<boolean>;
  /** Saves a note; with `revise`, asks Manna to revise the block by it right away. */
  addNote: (body: CreateWorksheetCommentRequest, revise?: boolean) => Promise<boolean>;
  updateNote: (c: WorksheetComment, patch: UpdateWorksheetCommentRequest) => Promise<void>;
  deleteNote: (c: WorksheetComment) => Promise<void>;
  revise: (blockId: string, body: ReviseWorksheetBlockRequest) => Promise<boolean>;
  accept: (p: WsProposal) => Promise<boolean>;
  /** 放棄, or 知道了 for one that failed or went stale. */
  reject: (p: WsProposal) => Promise<void>;
  /** 再改一次: the same request again (the server sets the open proposal aside). */
  retry: (p: WsProposal) => Promise<boolean>;
  reload: () => void;
}

/**
 * One worksheet's notes and block revisions: loaded together, polled while a revision runs.
 * `worksheetId` null (no worksheet yet, or it is being generated) loads nothing.
 */
export function useBlockRevise(worksheetId: number | null, options: BlockReviseOptions = {}): BlockRevise {
  const [comments, setComments] = useState<WorksheetComment[]>([]);
  const [proposals, setProposals] = useState<WsProposal[]>([]);
  const [loadFailed, setLoadFailed] = useState(false);
  const [error, setError] = useState<BlockRevise["error"]>(null);
  const [busy, setBusy] = useState(false);
  const [seq, setSeq] = useState(0);
  const [outcomes, setOutcomes] = useState<Record<string, PictureOutcome[]>>({});
  // The worksheet version each block's 採用 produced.
  const [acceptedAt, setAcceptedAt] = useState<Record<string, number>>({});
  const optionsRef = useRef(options);
  useEffect(() => {
    optionsRef.current = options;
  });
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const { reloadKey } = options;
  useEffect(() => {
    if (worksheetId === null) return undefined;
    let current = true;
    Promise.all([api.listWorksheetComments(worksheetId), api.listWorksheetProposals(worksheetId)])
      .then(([c, p]) => {
        if (!current) return;
        setComments(c);
        setProposals(p);
        setLoadFailed(false);
      })
      .catch(() => current && setLoadFailed(true));
    return () => {
      current = false;
    };
  }, [worksheetId, seq, reloadKey]);

  const running = proposals.some((p) => p.status === "running");
  // While a revision runs, ask again until it is done.
  useEffect(() => {
    if (!running || worksheetId === null) return undefined;
    let current = true;
    const timer = setTimeout(() => {
      api
        .listWorksheetProposals(worksheetId)
        .then((p) => current && setProposals(p))
        .catch(() => undefined);
    }, blockReviseConfig.pollMs);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [running, proposals, worksheetId]);

  const act = useCallback(
    async (blockId: string | null, work: () => Promise<void>, before = false): Promise<boolean> => {
      if (worksheetId === null) return false;
      setBusy(true);
      setError(null);
      try {
        if (before && optionsRef.current.beforeAction) {
          const failed = await optionsRef.current.beforeAction();
          if (failed) {
            if (alive.current) setError({ blockId, message: `沒有儲存成功，所以還沒送出：${failed}` });
            return false;
          }
        }
        await work();
        return true;
      } catch (err) {
        if (alive.current) setError({ blockId, message: errorMessage(err) });
        return false;
      } finally {
        if (alive.current) setBusy(false);
      }
    },
    [worksheetId],
  );

  const putProposal = (p: WsProposal) =>
    setProposals((prev) => [...prev.filter((x) => x.block_id !== p.block_id), p]);
  const putComment = (c: WorksheetComment) =>
    setComments((prev) => (prev.some((x) => x.id === c.id) ? prev.map((x) => (x.id === c.id ? c : x)) : [...prev, c]));

  const revise = (blockId: string, body: ReviseWorksheetBlockRequest) =>
    act(
      blockId,
      async () => {
        setOutcomes((prev) => Object.fromEntries(Object.entries(prev).filter(([k]) => k !== blockId)));
        setAcceptedAt((prev) => Object.fromEntries(Object.entries(prev).filter(([k]) => k !== blockId)));
        const p = await api.reviseWorksheetBlock(worksheetId!, blockId, body);
        if (alive.current) putProposal(p);
      },
      true,
    );

  return {
    comments,
    proposals,
    loadFailed,
    error,
    busy,
    running,
    notesOf: (blockId) => comments.filter((c) => blockOfNote(c) === blockId && c.anchor.block_id !== null),
    proposalOf: (blockId) => proposals.find((p) => p.block_id === blockId),
    picturesOf: (blockId) => outcomes[blockId] ?? [],
    justAccepted: (blockId) => options.versionNo != null && acceptedAt[blockId] === options.versionNo,
    undoAccepted: (blockId) =>
      act(
        blockId,
        async () => {
          const next = await api.undoWorksheetBlock(worksheetId!, blockId);
          if (!alive.current) return;
          setAcceptedAt((prev) => Object.fromEntries(Object.entries(prev).filter(([k]) => k !== blockId)));
          setOutcomes((prev) => Object.fromEntries(Object.entries(prev).filter(([k]) => k !== blockId)));
          optionsRef.current.onUndone?.(next);
        },
        true,
      ),
    addNote: async (body, andRevise = false) => {
      let created: WorksheetComment | null = null;
      const saved = await act(body.block_id, async () => {
        created = await api.createWorksheetComment(worksheetId!, body);
        if (alive.current) putComment(created);
      });
      const note = created as WorksheetComment | null;
      if (!saved || !andRevise || !note) return saved;
      return revise(body.block_id, { comment_ids: [note.id] });
    },
    updateNote: async (c, patch) => {
      await act(blockOfNote(c), async () => {
        const next = await api.updateWorksheetComment(worksheetId!, c.id, patch);
        if (alive.current) putComment(next);
      });
    },
    deleteNote: async (c) => {
      await act(blockOfNote(c), async () => {
        await api.deleteWorksheetComment(worksheetId!, c.id);
        if (alive.current) setComments((prev) => prev.filter((x) => x.id !== c.id));
      });
    },
    revise,
    accept: (p) =>
      act(
        p.block_id,
        async () => {
          try {
            const done = await api.acceptWorksheetProposal(worksheetId!, p.id);
            if (!alive.current) return;
            // Adopting any attempt ends the round: the block's other attempts go too.
            setProposals((prev) => prev.filter((x) => x.block_id !== p.block_id));
            setOutcomes((prev) => ({ ...prev, [p.block_id]: done.pictures ?? [] }));
            setAcceptedAt((prev) => ({ ...prev, [p.block_id]: done.worksheet.version_no }));
            optionsRef.current.onApplied?.(done);
          } finally {
            // Notes are marked applied; a proposal that went stale says so.
            if (alive.current) setSeq((n) => n + 1);
          }
        },
        true,
      ),
    reject: async (p) => {
      await act(p.block_id, async () => {
        await api.rejectWorksheetProposal(worksheetId!, p.id);
        if (alive.current) setProposals((prev) => prev.filter((x) => x.id !== p.id));
      });
    },
    retry: (p) =>
      revise(p.block_id, {
        ...(p.instruction ? { instruction: p.instruction } : {}),
        ...(p.comment_ids.length > 0 ? { comment_ids: p.comment_ids } : {}),
        ...(p.whole_block ? { whole_block: true } : {}),
      }),
    reload: () => setSeq((n) => n + 1),
  };
}

// ---------------------------------------------------------------- one note

function NoteCard({ note, place, ctl }: { note: WorksheetComment; place: string; ctl: BlockRevise }) {
  const ids = useId();
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(note.note);
  const [confirming, setConfirming] = useState(false);
  const done = note.status !== "open";
  return (
    <div
      className={`anno-note ws-note${done ? " is-done" : ""}${note.anchor.status === "lost" ? " is-lost" : ""}`}
      role="group"
      aria-label={`我的註記：${place}`}
    >
      <p className="anno-head">
        <b>我的註記</b>
        <span>{place}</span>
        {note.quote && (
          <span className="anno-quote" lang="en">
            「{clipQuote(note.quote)}」
          </span>
        )}
        <span className={`anno-flag${note.status === "open" ? "" : " is-quiet"}`}>
          {COMMENT_STATUS_LABEL[note.status]}
        </span>
      </p>
      {editing ? (
        <form
          className="anno-form"
          onSubmit={(e) => {
            e.preventDefault();
            void ctl.updateNote(note, { note: text.trim() }).then(() => setEditing(false));
          }}
        >
          <label className="sr-only" htmlFor={`${ids}-note`}>
            註記內容
          </label>
          <textarea
            id={`${ids}-note`}
            rows={2}
            maxLength={NOTE_MAX}
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
          <div className="anno-actions">
            <button type="submit" className="primary-button small-button" disabled={ctl.busy}>
              儲存註記
            </button>
            <button
              type="button"
              className="text-link is-plain"
              onClick={() => {
                setText(note.note);
                setEditing(false);
              }}
            >
              取消
            </button>
          </div>
        </form>
      ) : (
        <p className="anno-text">{note.note || "（只標記，沒有寫說明）"}</p>
      )}
      {!editing && (
        <div className="anno-actions">
          <label className="anno-share">
            <input
              type="checkbox"
              checked={note.share_with_ai}
              disabled={ctl.busy}
              onChange={(e) => void ctl.updateNote(note, { share_with_ai: e.target.checked })}
            />
            提供給 AI
          </label>
          <button type="button" className="text-link is-plain" onClick={() => setEditing(true)}>
            修改
          </button>
          {note.status === "open" ? (
            <button
              type="button"
              className="text-link is-plain"
              disabled={ctl.busy}
              onClick={() => void ctl.updateNote(note, { status: "dismissed" })}
            >
              略過
            </button>
          ) : (
            <button
              type="button"
              className="text-link is-plain"
              disabled={ctl.busy}
              onClick={() => void ctl.updateNote(note, { status: "open" })}
            >
              改回待處理
            </button>
          )}
          {confirming ? (
            <span className="confirm-row" role="group" aria-label="確認刪除註記">
              刪除這則註記？學習單不會變動。
              <button
                type="button"
                className="ghost-button small-button"
                disabled={ctl.busy}
                onClick={() => void ctl.deleteNote(note)}
              >
                確定刪除
              </button>
              <button type="button" className="ghost-button small-button" onClick={() => setConfirming(false)}>
                取消
              </button>
            </span>
          ) : (
            <button type="button" className="text-link is-plain" onClick={() => setConfirming(true)}>
              刪除
            </button>
          )}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- forms

/** A new note: where it goes is already chosen (a selection, a question or the block). */
export function NoteDraft({
  draft,
  place,
  ctl,
  onDone,
}: {
  draft: SheetSelection;
  place: string;
  ctl: BlockRevise;
  onDone: () => void;
}) {
  const ids = useId();
  const [note, setNote] = useState("");
  const [share, setShare] = useState(true);
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    ref.current?.focus();
  }, []);
  const body: CreateWorksheetCommentRequest = {
    block_id: draft.block_id,
    field: draft.field,
    ...(draft.item_id ? { item_id: draft.item_id } : {}),
    ...(draft.start !== undefined && draft.end !== undefined ? { start: draft.start, end: draft.end } : {}),
    ...(draft.quote ? { quote: draft.quote } : {}),
    note: note.trim(),
    share_with_ai: share,
  };
  const save = (revise: boolean) => void ctl.addNote(body, revise).then((ok) => ok && onDone());
  return (
    <form
      className="anno-note is-draft"
      aria-label="新增註記"
      lang="zh-Hant"
      onSubmit={(e) => {
        e.preventDefault();
        save(false);
      }}
    >
      <p className="anno-head">
        <b>新增註記</b>
        <span>{place}</span>
        {draft.quote && (
          <span className="anno-quote" lang="en">
            「{clipQuote(draft.quote)}」
          </span>
        )}
      </p>
      <label className="sr-only" htmlFor={`${ids}-note`}>
        註記內容
      </label>
      <textarea
        id={`${ids}-note`}
        ref={ref}
        rows={2}
        maxLength={NOTE_MAX}
        value={note}
        placeholder={NOTE_PLACEHOLDER}
        onChange={(e) => setNote(e.target.value)}
      />
      <div className="anno-actions">
        <label className="anno-share">
          <input type="checkbox" checked={share} onChange={(e) => setShare(e.target.checked)} />
          提供給 AI
        </label>
        <button type="submit" className="secondary-button small-button" disabled={ctl.busy}>
          儲存註記
        </button>
        <button
          type="button"
          className="primary-button small-button"
          disabled={ctl.busy || ctl.running || !share || note.trim() === ""}
          onClick={() => save(true)}
        >
          儲存並請 Manna 改
        </button>
        <button type="button" className="text-link is-plain" onClick={onDone}>
          取消
        </button>
      </div>
    </form>
  );
}

/** 「改這一區」 in the editor: one sentence for this block. */
export function ReviseForm({
  name,
  blockId,
  ctl,
  onDone,
}: {
  name: string;
  blockId: string;
  ctl: BlockRevise;
  onDone: () => void;
}) {
  const ids = useId();
  const [text, setText] = useState("");
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    ref.current?.focus();
  }, []);
  return (
    <form
      className="ws-revise-form"
      aria-label={`請 Manna 改${name}`}
      lang="zh-Hant"
      onSubmit={(e) => {
        e.preventDefault();
        const instruction = text.trim();
        if (!instruction) return;
        void ctl.revise(blockId, { instruction }).then((ok) => ok && onDone());
      }}
    >
      <label htmlFor={`${ids}-text`}>想怎麼改{name}？</label>
      <textarea
        id={`${ids}-text`}
        ref={ref}
        rows={2}
        maxLength={INSTRUCTION_MAX}
        value={text}
        placeholder={REVISE_PLACEHOLDER}
        onChange={(e) => setText(e.target.value)}
      />
      <div className="anno-actions">
        <button
          type="submit"
          className="primary-button small-button"
          disabled={ctl.busy || ctl.running || text.trim() === ""}
        >
          請 Manna 改這一區
        </button>
        <button type="button" className="text-link is-plain" onClick={onDone}>
          取消
        </button>
      </div>
    </form>
  );
}

// ---------------------------------------------------------------- the proposal

const CHANGE_LABEL: Record<WsProposalChange["change"], string> = {
  kept: "不變",
  modified: "修改",
  new: "新增",
  removed: "刪除",
};

function ItemLines({ item, teacher }: { item: WsItem; teacher: boolean }) {
  return (
    <>
      {item.options.length > 0 && (
        <ul className="prep-sheet-options" lang="en">
          {item.options.map((o, i) => (
            <li key={o}>{printedOption(o, i)}</li>
          ))}
        </ul>
      )}
      {teacher && item.answer.trim() && (
        <p className="ws-proposal-answer">
          參考答案：<span lang="en">{printedAnswer(item.answer_format, item.answer, item.options)}</span>
        </p>
      )}
    </>
  );
}

/** Manna's proposal for one block: what changed, and 採用／放棄／再改一次. */
export function ProposalPanel({
  proposal,
  ctl,
  teacher = true,
}: {
  proposal: WsProposal;
  ctl: BlockRevise;
  /** Show the reference answers (the teacher version, the editor). */
  teacher?: boolean;
}) {
  const p = proposal;
  // The attempts of this round (the ones 再改一次 set aside, then the newest); the newest shows first.
  const attempts = [...(p.earlier ?? []), p];
  const [view, setView] = useState<{ latest: number; at: number } | null>(null);
  const at = view && view.latest === p.id ? Math.min(view.at, attempts.length - 1) : attempts.length - 1;
  const shown = attempts[at] ?? p;
  if (p.status === "running") {
    return (
      <p className="manna-label prep-working ws-proposal-running" role="status" lang="zh-Hant">
        <span className="star is-working" aria-hidden="true" />
        Manna 正在改這一區…
      </p>
    );
  }
  if (p.status === "failed" || p.status === "stale") {
    return (
      <div className="ws-proposal is-failed" role="alert" lang="zh-Hant">
        <p>{p.error || "這次沒有改成。"}</p>
        <div className="anno-actions">
          <button
            type="button"
            className="secondary-button small-button"
            disabled={ctl.busy || ctl.running}
            onClick={() => void ctl.retry(p)}
          >
            再改一次
          </button>
          <button type="button" className="text-link is-plain" disabled={ctl.busy} onClick={() => void ctl.reject(p)}>
            知道了
          </button>
        </div>
      </div>
    );
  }
  if (p.status !== "proposed") return null;
  const s = shown;
  const pictureActions = s.picture_actions ?? [];
  // Only pictures to add, redraw or remove: there is no changed block, but 採用 still does it.
  if ((!s.block || !s.changes) && pictureActions.length === 0) return null;
  const after = s.block ?? s.before;
  const changes = s.changes ?? { title: false, instructions: false, items: [] };
  const find = (block: WsBlock, id: string) => block.items.find((i) => i.id === id);
  const go = (to: number) => setView({ latest: p.id, at: to });
  return (
    <section className="ws-proposal" aria-label="Manna 的修改建議" lang="zh-Hant">
      <p className="manna-label ws-proposal-head">
        <span className="star" aria-hidden="true" />
        Manna 的修改建議{attempts.length === 1 && p.round > 1 ? `（第 ${p.round} 次）` : ""}：還沒有改進學習單，按「採用」才會寫入。
      </p>
      {attempts.length > 1 && (
        <div className="ws-proposal-attempts" role="group" aria-label="這一區的各次建議">
          <button
            type="button"
            className="text-link is-plain"
            aria-label="上一次的建議"
            disabled={ctl.busy || at === 0}
            onClick={() => go(at - 1)}
          >
            ‹
          </button>
          <span role="status" aria-live="polite" aria-atomic="true">
            第 {at + 1} 次（共 {attempts.length} 次）
          </span>
          <button
            type="button"
            className="text-link is-plain"
            aria-label="下一次的建議"
            disabled={ctl.busy || at === attempts.length - 1}
            onClick={() => go(at + 1)}
          >
            ›
          </button>
        </div>
      )}
      {attempts.length > 1 && s.instruction && <p className="ws-proposal-asked">這次的要求：{s.instruction}</p>}
      {s.reply && <p className="ws-proposal-reply">{s.reply}</p>}
      {changes.title && (
        <p className="ws-proposal-field">
          標題：<ins lang="en">{after.title}</ins> <del lang="en">{s.before.title}</del>
        </p>
      )}
      {changes.instructions && (
        <p className="ws-proposal-field">
          作答指示：<ins lang="en">{after.instructions}</ins> <del lang="en">{s.before.instructions}</del>
        </p>
      )}
      <PrepImageThumbs images={s.images} />
      <ol className="ws-proposal-items" aria-label="修改後的題目">
        {changes.items.map((c) => {
          const item = c.change === "removed" ? find(s.before, c.id) : find(after, c.id);
          if (!item) return null;
          const old = c.change === "modified" ? find(s.before, c.id) : undefined;
          return (
            <li key={`${c.change}-${c.id}`} className={`is-${c.change}`}>
              <span className="anno-flag is-quiet">{CHANGE_LABEL[c.change]}</span>{" "}
              {c.change === "removed" ? (
                <del lang="en">{item.prompt}</del>
              ) : (
                <span lang="en" className={c.change === "kept" ? "ws-proposal-kept" : undefined}>
                  {item.prompt}
                </span>
              )}
              {old && old.prompt !== item.prompt && (
                <p className="ws-proposal-old">
                  原本：<del lang="en">{old.prompt}</del>
                </p>
              )}
              {c.change !== "kept" && c.change !== "removed" && <ItemLines item={item} teacher={teacher} />}
            </li>
          );
        })}
      </ol>
      {pictureActions.length > 0 && (
        <div className="ws-proposal-pictures" role="group" aria-label="圖片">
          <h4>圖片</h4>
          <ul>
            {pictureActions.map((a, i) => (
              <li key={`${a.slot_id}-${i}`} data-action={a.action}>
                {a.text}
              </li>
            ))}
          </ul>
          <p className="field-hint">按「採用」後才會畫圖或移除圖片。</p>
        </div>
      )}
      <div className="anno-actions">
        <button type="button" className="primary-button small-button" disabled={ctl.busy} onClick={() => void ctl.accept(s)}>
          採用
        </button>
        <button type="button" className="secondary-button small-button" disabled={ctl.busy} onClick={() => void ctl.reject(p)}>
          放棄
        </button>
        <button
          type="button"
          className="text-link is-plain"
          disabled={ctl.busy}
          onClick={() => void ctl.retry(s)}
        >
          再改一次
        </button>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------- right after 採用

/** 「已採用 Manna 的修改。」 with 復原: shown until the block is changed again by anything. */
function AcceptedNotice({ blockId, ctl }: { blockId: string; ctl: BlockRevise }) {
  const ref = useRef<HTMLDivElement>(null);
  // 採用 removed the button that had focus: put it here, where 復原 is the next stop.
  useEffect(() => {
    ref.current?.focus();
  }, []);
  return (
    <div className="ws-accepted-notice" role="status" aria-live="polite" ref={ref} tabIndex={-1} lang="zh-Hant">
      <span>已採用 Manna 的修改。</span>{" "}
      <button
        type="button"
        className="secondary-button small-button"
        disabled={ctl.busy || ctl.running}
        onClick={() => void ctl.undoAccepted(blockId)}
      >
        復原
      </button>
    </div>
  );
}

// ---------------------------------------------------------------- what goes under a block

/**
 * Under one block: Manna's proposal (or that it is working), the form being filled in, the
 * notes on the block and what went wrong.
 */
export function BlockReviseArea({
  blockId,
  ctl,
  numberOf,
  draft,
  onDraftDone,
  teacher,
}: {
  blockId: string;
  ctl: BlockRevise;
  numberOf: NumberOf;
  draft: SheetSelection | null;
  onDraftDone: () => void;
  teacher?: boolean;
}) {
  const proposal = ctl.proposalOf(blockId);
  const notes = ctl.notesOf(blockId);
  const error = ctl.error && ctl.error.blockId === blockId ? ctl.error.message : null;
  const pictures = ctl.picturesOf(blockId);
  const accepted = ctl.justAccepted(blockId);
  if (!proposal && notes.length === 0 && !draft && !error && pictures.length === 0 && !accepted) return null;
  return (
    <div className="ws-revise-area" lang="zh-Hant">
      {proposal && <ProposalPanel proposal={proposal} ctl={ctl} teacher={teacher} />}
      {accepted && !proposal && <AcceptedNotice blockId={blockId} ctl={ctl} />}
      {pictures.length > 0 && (
        <div className="ws-proposal-pictures" role="status" aria-label="圖片的結果">
          <PictureOutcomeList outcomes={pictures} />
        </div>
      )}
      {draft && draft.block_id === blockId && (
        <NoteDraft
          key={`${draft.field}-${draft.item_id ?? ""}-${draft.start ?? ""}`}
          draft={draft}
          place={placeOf(
            {
              field: draft.field,
              anchor: { status: "ok", block_id: blockId, item_id: draft.item_id ?? null, start: null, end: null },
            },
            numberOf,
          )}
          ctl={ctl}
          onDone={onDraftDone}
        />
      )}
      {notes.map((n) => (
        <NoteCard key={n.id} note={n} place={placeOf(n, numberOf)} ctl={ctl} />
      ))}
      <ErrorText message={error} />
    </div>
  );
}

/** 「依註記修改（n 則）」 for a block with open notes for Manna. */
export function ReviseByNotesButton({ blockId, ctl, disabled }: { blockId: string; ctl: BlockRevise; disabled?: boolean }) {
  const notes = openAiNotes(ctl.notesOf(blockId));
  if (notes.length === 0) return null;
  return (
    <button
      type="button"
      className="text-link is-plain is-gold"
      disabled={disabled || ctl.busy || ctl.running}
      onClick={() => void ctl.revise(blockId, { comment_ids: notes.map((n) => n.id) })}
    >
      依註記修改（{notes.length} 則）
    </button>
  );
}

/** Notes whose block is gone (a new generation made new blocks). */
export function LostNotes({ ctl }: { ctl: BlockRevise }) {
  const lost = ctl.comments.filter((c) => c.anchor.block_id === null);
  if (lost.length === 0) return null;
  return (
    <section className="anno-lost" aria-label="找不到位置的註記" lang="zh-Hant">
      <p className="quiet-note">這些註記原本標的區塊已經不在了（例如重新產生了整份學習單）。可以刪除，或留著參考。</p>
      {lost.map((n) => (
        <NoteCard key={n.id} note={n} place="原本的區塊" ctl={ctl} />
      ))}
    </section>
  );
}
