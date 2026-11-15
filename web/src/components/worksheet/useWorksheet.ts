import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type {
  SaveWorksheetMaterialRequest,
  WorksheetBrief,
  WorksheetContent,
  WorksheetSettings,
  WorksheetState,
} from "../../api/types";
import { useAutosave } from "../../lib/useAutosave";

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
  saveMaterial: (body: SaveWorksheetMaterialRequest) => Promise<boolean>;
  /** 照我的版型: sets (the server re-derives the settings from it) or removes the template. */
  setTemplate: (templateId: number | null) => Promise<boolean>;
  /** Takes a state the server returned from another call (link, save as lesson). */
  adopt: (state: WorksheetState) => void;
  /** Re-reads the worksheet (after a sync, for the new difference count). */
  refresh: () => Promise<void>;
  dirty: boolean;
  saving: boolean;
  saveError: string | null;
  savedAt: string | null;
  flush: () => Promise<string | null>;
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
    const sent = { title: now.title, filename: now.filename, settings: now.settings, brief: now.brief };
    // While steps run the blocks belong to the server: the content is not sent.
    const body = now.running ? sent : { ...sent, content: now.content };
    const saved = await api.saveWorksheet(id, body);
    const latest = stateRef.current;
    if (alive.current && latest) {
      // Keep what the teacher typed meanwhile; take what only the server knows.
      put({
        ...saved,
        title: latest.title,
        filename: latest.filename,
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
  const saveMaterial = useCallback(
    (body: SaveWorksheetMaterialRequest) => act(() => api.saveWorksheetMaterial(id, body)),
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

  // Poll while steps are queued or running; when they end, take the assembled worksheet.
  const running = state?.running ?? false;
  useEffect(() => {
    if (!running) return;
    let stopped = false;
    const timer = setInterval(() => {
      void (async () => {
        try {
          const jobs = await api.worksheetJobs(id);
          if (stopped) return;
          if (jobs.running) {
            const now = stateRef.current;
            if (now) put({ ...now, jobs: jobs.jobs });
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
  }, [running, id, put]);

  return {
    status,
    loadError,
    reload: () => {
      setStatus("loading");
      setLoadError(null);
      setLoadSeq((n) => n + 1);
    },
    state,
    setTitle,
    setFilename,
    setSettings,
    setBrief,
    setContent,
    actionError,
    busy,
    generate,
    regenerate,
    undo,
    saveMaterial,
    setTemplate,
    adopt: put,
    refresh,
    dirty: autosave.dirty,
    saving: autosave.saving,
    saveError: autosave.error,
    savedAt: autosave.savedAt,
    flush,
  };
}
