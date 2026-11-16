import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "../../api/client";
import type { PickedFile } from "./files";
import { adminLibraryConfig, extensionOf, isSystemFile } from "./prefill";

/**
 * ready: waiting for 全部上傳; queued: will go when a slot frees; uploading; done (result set);
 * failed (error set, 重試 queues it again).
 */
export type RowStatus = "ready" | "queued" | "uploading" | "done" | "failed";

export interface QueueRow<R> {
  key: string;
  file: File;
  path: string;
  title: string;
  category: string;
  /** As typed: separated by 、 or commas. */
  tags: string;
  status: RowStatus;
  error: string | null;
  result: R | null;
}

export type RowFields = Pick<QueueRow<unknown>, "title" | "category" | "tags">;

export interface QueueOptions<R> {
  /** File extensions this queue takes (lower case, no dot). */
  extensions: readonly string[];
  /** The defaults of a new row, from its path. */
  prefill: (path: string) => RowFields;
  /** Why a row cannot be sent as it is, or null. */
  validate: (row: QueueRow<R>) => string | null;
  upload: (row: QueueRow<R>) => Promise<R>;
  /** Called once the last queued row has finished (some may have failed). */
  onDrained?: (uploaded: number) => void;
}

/** What adding files did: how many rows, and the files left out because they are not accepted. */
export interface AddOutcome {
  added: number;
  unsupported: string[];
}

const keyOf = (p: PickedFile) => `${p.path}\u0000${p.file.size}\u0000${p.file.lastModified}`;

/**
 * The rows of a bulk upload. 全部上傳 sends them `adminLibraryConfig.concurrency` (2) at a time;
 * each row keeps its own status and error and can be sent again on its own. The rows live in a
 * ref as well as in state so the next row is taken synchronously when a slot frees.
 */
export function useUploadQueue<R>(options: QueueOptions<R>) {
  const opts = useRef(options);
  useEffect(() => {
    opts.current = options;
  });
  const rowsRef = useRef<QueueRow<R>[]>([]);
  const [rows, setRows] = useState<QueueRow<R>[]>([]);
  const active = useRef(0);
  /** Rows finished in the current run (reported to onDrained). */
  const finished = useRef(0);

  const commit = useCallback((next: QueueRow<R>[]) => {
    rowsRef.current = next;
    setRows(next);
  }, []);

  const update = useCallback(
    (key: string, patch: Partial<QueueRow<R>>) =>
      commit(rowsRef.current.map((r) => (r.key === key ? { ...r, ...patch } : r))),
    [commit],
  );

  const pump = useCallback(
    function pumpQueue() {
      while (active.current < adminLibraryConfig.concurrency) {
        const next = rowsRef.current.find((r) => r.status === "queued");
        if (!next) break;
        active.current += 1;
        update(next.key, { status: "uploading", error: null });
        opts.current
          .upload(next)
          .then(
            (result) => {
              finished.current += 1;
              update(next.key, { status: "done", result, error: null });
            },
            (err: unknown) => update(next.key, { status: "failed", error: errorMessage(err) }),
          )
          .finally(() => {
            active.current -= 1;
            pumpQueue();
            if (active.current === 0 && !rowsRef.current.some((r) => r.status === "queued")) {
              const uploaded = finished.current;
              finished.current = 0;
              opts.current.onDrained?.(uploaded);
            }
          });
      }
    },
    [update],
  );

  const add = useCallback(
    (picked: PickedFile[]): AddOutcome => {
      const seen = new Set(rowsRef.current.map((r) => r.key));
      const unsupported: string[] = [];
      const fresh: QueueRow<R>[] = [];
      const sorted = [...picked].sort((a, b) => a.path.localeCompare(b.path, undefined, { numeric: true }));
      for (const p of sorted) {
        if (isSystemFile(p.path)) continue;
        if (!opts.current.extensions.includes(extensionOf(p.file.name))) {
          unsupported.push(p.path);
          continue;
        }
        const key = keyOf(p);
        if (seen.has(key)) continue;
        seen.add(key);
        fresh.push({
          key,
          file: p.file,
          path: p.path,
          ...opts.current.prefill(p.path),
          status: "ready",
          error: null,
          result: null,
        });
      }
      if (fresh.length > 0) commit([...rowsRef.current, ...fresh]);
      return { added: fresh.length, unsupported };
    },
    [commit],
  );

  /** Edits a row that has not been sent. */
  const edit = useCallback(
    (key: string, patch: Partial<RowFields>) => {
      const row = rowsRef.current.find((r) => r.key === key);
      if (!row || (row.status !== "ready" && row.status !== "failed")) return;
      update(key, { ...patch, error: null });
    },
    [update],
  );

  /** 全部上傳: every row not sent yet (failed ones too) that passes the checks. */
  const uploadAll = useCallback(() => {
    commit(
      rowsRef.current.map((r) => {
        if (r.status !== "ready" && r.status !== "failed") return r;
        const problem = opts.current.validate(r);
        return problem ? { ...r, status: "ready", error: problem } : { ...r, status: "queued", error: null };
      }),
    );
    pump();
  }, [commit, pump]);

  /** 重試 one row. */
  const retry = useCallback(
    (key: string) => {
      const row = rowsRef.current.find((r) => r.key === key);
      if (!row || (row.status !== "failed" && row.status !== "ready")) return;
      const problem = opts.current.validate(row);
      if (problem) {
        update(key, { status: "ready", error: problem });
        return;
      }
      update(key, { status: "queued", error: null });
      pump();
    },
    [pump, update],
  );

  const remove = useCallback(
    (key: string) => commit(rowsRef.current.filter((r) => r.key !== key || r.status === "queued" || r.status === "uploading")),
    [commit],
  );

  /** Takes away every row that is not on its way (queued or uploading rows stay). */
  const clear = useCallback(
    (which: "done" | "all") =>
      commit(
        rowsRef.current.filter(
          (r) => r.status === "queued" || r.status === "uploading" || (which === "done" && r.status !== "done"),
        ),
      ),
    [commit],
  );

  /** Replaces results the server reported again (a template's extraction status). */
  const patchResults = useCallback(
    (fn: (result: R) => R) => {
      let changed = false;
      const next = rowsRef.current.map((r) => {
        if (r.result === null) return r;
        const result = fn(r.result);
        if (result === r.result) return r;
        changed = true;
        return { ...r, result };
      });
      if (changed) commit(next);
    },
    [commit],
  );

  /** Some row is on its way (leaving the page would stop the queue). */
  const busy = rows.some((r) => r.status === "queued" || r.status === "uploading");

  return { rows, busy, add, edit, uploadAll, retry, remove, clear, patchResults };
}

export type UploadQueue<R> = ReturnType<typeof useUploadQueue<R>>;
