import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, api, errorMessage } from "../../api/client";
import type {
  SaveWorksheetMaterialRequest,
  WorksheetBrief,
  WorksheetContent,
  WorksheetImages,
  WorksheetSettings,
  WorksheetState,
} from "../../api/types";
import { useAutosave } from "../../lib/useAutosave";
import { anyOpen } from "./SlotPicture";

/** How often running generation steps are asked for. Tests shorten it. */
export const worksheetConfig = { pollMs: 2000 };

export interface WorksheetController {
  status: "loading" | "ready" | "error";
  loadError: string | null;
  reload: () => void;
  /** The worksheet as last heard from the server, with the teacher's unsaved edits on top. */
  state: WorksheetState | null;
  setTitle: (title: string) => void;
  setFilename: (filename: string) => void;
  /** 給 Manna 的要求 (saved with the rest). */
  setRequest: (request: string) => void;
  setSettings: (change: (settings: WorksheetSettings) => WorksheetSettings) => void;
  setBrief: (change: (brief: WorksheetBrief) => WorksheetBrief) => void;
  /** Every edit of the worksheet goes through here; it is saved a moment later. */
  setContent: (change: (content: WorksheetContent) => WorksheetContent) => void;
  /** Why an action (generate, regenerate, undo, change material) could not start. */
  actionError: string | null;
  /** An action is being sent (not: the model is working; that is `state.running`). */
  busy: boolean;
  generate: () => Promise<boolean>;
  regenerate: (blockId: string) => Promise<void>;
  undo: (blockId: string) => Promise<void>;
  /** 恢復字詞表: puts back the table that was deleted from the block (levels and hints are computed again). */
  restoreWordTable: (blockId: string) => Promise<void>;
  saveMaterial: (body: SaveWorksheetMaterialRequest) => Promise<boolean>;
  /** 只選要用的段落 (ids of the library material); `null`: all of them. The library entry is not changed. */
  saveSelection: (paragraphIds: string[] | null) => Promise<boolean>;
  /** 照我的版型: sets (the server re-derives the settings from it) or removes the template. */
  setTemplate: (templateId: number | null) => Promise<boolean>;
  /** Takes a state the server returned from another call (link, save as lesson). */
  adopt: (state: WorksheetState) => void;
  /** Takes the pictures the server returned (a picture action); nothing to save. */
  setImages: (images: WorksheetImages) => void;
  /** Re-reads the worksheet (after a sync, for the new difference count). */
  refresh: () => Promise<void>;
  dirty: boolean;
  saving: boolean;
  saveError: string | null;
  savedAt: string | null;
  flush: () => Promise<string | null>;
  /**
   * The last save was refused because the worksheet has a newer version (a block revision was
   * accepted elsewhere): what the teacher typed since was not stored; 重新載入 is the way on.
   */
  conflict: boolean;
}

/**
 * One worksheet: loading, quiet saving, and the generation steps as background work that is
 * polled. While steps run the teacher cannot edit (the server replaces the blocks when they end),
 * but can always leave.
 */
export function useWorksheet(id: number): WorksheetController {
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loadSeq, setLoadSeq] = useState(0);
  const [state, setState] = useState<WorksheetState | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [conflict, setConflict] = useState(false);
  const stateRef = useRef<WorksheetState | null>(null);
  const alive = useRef(true);

  const put = useCallback((next: WorksheetState | null) => {
    stateRef.current = next;
    setState(next);
  }, []);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    let current = true;
    api
      .getWorksheet(id)
      .then((loaded) => {
        if (!current) return;
        put(loaded);
        setStatus("ready");
      })
      .catch((err: unknown) => {
        if (!current) return;
        setLoadError(errorMessage(err));
        setStatus("error");
      });
    return () => {
      current = false;
    };
  }, [id, loadSeq, put]);

  const save = useCallback(async (): Promise<string | null> => {
    const now = stateRef.current;
    if (!now) return null;
    const sent = {
      title: now.title,
      filename: now.filename,
      settings: now.settings,
      brief: now.brief,
      // Only once there is something to say: an older server does not know the field.
      ...(now.request !== undefined ? { request: now.request } : {}),
    };
    // While steps run the blocks belong to the server: the content is not sent. With it goes the
    // version it was edited from, so a newer one (a block revision accepted elsewhere) is never
    // overwritten: the server answers 409 instead.
    const body = now.running ? sent : { ...sent, content: now.content, base_version_no: now.version_no };
    let saved: WorksheetState;
    try {
      saved = await api.saveWorksheet(id, body);
    } catch (err) {
      if (err instanceof ApiError && err.status === 409 && !now.running && alive.current) setConflict(true);
      throw err;
    }
    const latest = stateRef.current;
    if (alive.current && latest) {
      // Keep what the teacher typed meanwhile; take what only the server knows.
      put({
        ...saved,
        title: latest.title,
        filename: latest.filename,
        request: latest.request,
        settings: latest.settings,
        brief: latest.brief,
        content: latest.running ? saved.content : latest.content,
        jobs: latest.jobs,
        running: latest.running,
      });
    }
    return saved.saved_at;
  }, [id, put]);

  const autosave = useAutosave(save);
  const { touch, flush } = autosave;

  const edit = useCallback(
    (change: (s: WorksheetState) => WorksheetState) => {
      const now = stateRef.current;
      if (!now) return;
      put(change(now));
      touch();
    },
    [put, touch],
  );

  const setTitle = useCallback((title: string) => edit((s) => ({ ...s, title })), [edit]);
  const setFilename = useCallback((filename: string) => edit((s) => ({ ...s, filename })), [edit]);
  const setRequest = useCallback((request: string) => edit((s) => ({ ...s, request })), [edit]);
  const setSettings = useCallback(
    (change: (settings: WorksheetSettings) => WorksheetSettings) =>
      edit((s) => ({ ...s, settings: change(s.settings) })),
    [edit],
  );
  const setBrief = useCallback(
    (change: (brief: WorksheetBrief) => WorksheetBrief) => edit((s) => ({ ...s, brief: change(s.brief) })),
    [edit],
  );
  const setContent = useCallback(
    (change: (content: WorksheetContent) => WorksheetContent) =>
      edit((s) => (s.running ? s : { ...s, content: change(s.content) })),
    [edit],
  );

  /** Saves first, then sends an action whose answer is the whole worksheet. */
  const act = useCallback(
    async (call: () => Promise<WorksheetState>): Promise<boolean> => {
      setActionError(null);
      setBusy(true);
      try {
        const failed = await flush();
        if (failed) {
          if (alive.current) setActionError(`沒有儲存成功，所以還沒開始：${failed}`);
          return false;
        }
        const next = await call();
        if (alive.current) put(next);
        return true;
      } catch (err) {
        if (alive.current) setActionError(errorMessage(err));
        return false;
      } finally {
        if (alive.current) setBusy(false);
      }
    },
    [flush, put],
  );

  const generate = useCallback(() => act(() => api.generateWorksheet(id)), [act, id]);
  const regenerate = useCallback(
    async (blockId: string) => {
      await act(() => api.regenerateWorksheetBlock(id, blockId));
    },
    [act, id],
  );
  const undo = useCallback(
    async (blockId: string) => {
      await act(() => api.undoWorksheetBlock(id, blockId));
    },
    [act, id],
  );
  const restoreWordTable = useCallback(
    async (blockId: string) => {
      await act(() => api.restoreWordTable(id, blockId));
    },
    [act, id],
  );
  const saveMaterial = useCallback(
    (body: SaveWorksheetMaterialRequest) => act(() => api.saveWorksheetMaterial(id, body)),
    [act, id],
  );

  const saveSelection = useCallback(
    (paragraphIds: string[] | null) => act(() => api.putWorksheetMaterialSelection(id, paragraphIds)),
    [act, id],
  );

  const setTemplate = useCallback(
    (templateId: number | null) => act(() => api.setWorksheetTemplate(id, templateId)),
    [act, id],
  );

  const refresh = useCallback(async () => {
    try {
      const failed = await flush();
      if (failed) return;
      const next = await api.getWorksheet(id);
      if (alive.current) put(next);
    } catch {
      // The page keeps what it has; the next action reports a real failure.
    }
  }, [flush, id, put]);

  const setImages = useCallback(
    (images: WorksheetImages) => {
      const now = stateRef.current;
      if (now) put({ ...now, images });
    },
    [put],
  );

  // Pictures being made (after a generation or a 重畫): ask for them until none is open.
  const picturesOpen = !(state?.running ?? false) && anyOpen(state?.images);
  useEffect(() => {
    if (!picturesOpen) return;
    let stopped = false;
    const timer = setInterval(() => {
      void api
        .worksheetImages(id)
        .then((images) => {
          if (!stopped) setImages(images);
        })
        .catch(() => undefined);
    }, worksheetConfig.pollMs);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [picturesOpen, id, setImages]);

  // Poll while steps are queued or running; when they end, take the assembled worksheet. After the
  // text, the run's progress may still be open (its pictures): follow it until it ends.
  const running = state?.running ?? false;
  const progressOpen = state?.progress?.status === "running";
  useEffect(() => {
    if (!running && !progressOpen) return;
    let stopped = false;
    const timer = setInterval(() => {
      void (async () => {
        try {
          const jobs = await api.worksheetJobs(id);
          if (stopped) return;
          const now = stateRef.current;
          if (jobs.running || (now && !now.running)) {
            if (now) put({ ...now, jobs: jobs.jobs, progress: jobs.progress ?? null });
            return;
          }
          const next = await api.getWorksheet(id);
          if (!stopped) put(next);
        } catch {
          // A missed poll is not a failure; the next one tries again.
        }
      })();
    }, worksheetConfig.pollMs);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [running, progressOpen, id, put]);

  return {
    status,
    loadError,
    reload: () => {
      setStatus("loading");
      setLoadError(null);
      setConflict(false);
      setLoadSeq((n) => n + 1);
    },
    state,
    setTitle,
    setFilename,
    setRequest,
    setSettings,
    setBrief,
    setContent,
    actionError,
    busy,
    generate,
    regenerate,
    undo,
    restoreWordTable,
    saveMaterial,
    saveSelection,
    setTemplate,
    adopt: put,
    setImages,
    refresh,
    dirty: autosave.dirty,
    saving: autosave.saving,
    saveError: autosave.error,
    savedAt: autosave.savedAt,
    flush,
    conflict,
  };
}
