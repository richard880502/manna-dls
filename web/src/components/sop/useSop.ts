import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type {
  DecisionKind,
  SopAnalysisSuggestion,
  SopContent,
  SopJob,
  SopJobKind,
  SopMaterial,
  SopObjectivesSuggestion,
} from "../../api/types";
import { logScaffold } from "../../lib/scaffold";
import { emptyContent, normalizeContent } from "../../lib/sop";
import { useAutosave } from "../../lib/useAutosave";

/** How often a running AI job is asked for. Tests shorten it. */
export const sopConfig = { pollMs: 2000 };

export interface MaterialDraft {
  title: string;
  source_note: string;
  text: string;
}

export interface SopJobs {
  analysis: SopJob<SopAnalysisSuggestion> | null;
  objectives: SopJob<SopObjectivesSuggestion> | null;
}

export interface SopController {
  status: "loading" | "ready" | "error";
  loadError: string | null;
  reload: () => void;
  /** The material as the server stored and split it (null: none yet). */
  material: SopMaterial | null;
  /** The lesson had a material and the teacher deleted it from the library (「教材已刪除」). */
  materialDeleted: boolean;
  /**
   * Uses a material of the teacher's library for this lesson, or (null) takes the material off
   * the lesson. Saves what is pending first. Resolves to an error message, or null.
   */
  pickMaterial: (materialId: number | null) => Promise<string | null>;
  /** What the teacher is typing in step 1. */
  draft: MaterialDraft;
  setDraft: (patch: Partial<MaterialDraft>) => void;
  content: SopContent;
  /** Every edit of the document goes through here; it is saved a moment later. */
  update: (change: (content: SopContent) => SopContent) => void;
  jobs: SopJobs;
  /** Why a job could not be started (not why it failed: that is in its result). */
  jobError: Record<SopJobKind, string | null>;
  startJob: (kind: SopJobKind) => Promise<void>;
  /** Records the teacher's decision on one suggestion and takes it off the screen. */
  decide: (kind: SopJobKind, itemIndex: number, decision: DecisionKind, revisedContent?: string) => void;
  dirty: boolean;
  saving: boolean;
  saveError: string | null;
  savedAt: string | null;
  /** Saves whatever is pending (material first). Resolves to an error message, or null. */
  flush: () => Promise<string | null>;
}

const SCAFFOLD = { analysis: "SC-SOP-ANALYSIS", objectives: "SC-SOP-OBJECTIVES" } as const;

/**
 * The lesson SOP of one lesson: loading, quiet saving, and the two AI jobs. Mount it once per
 * lesson (the page is keyed by the lesson id): it does not reset itself for another lesson.
 */
export function useSop(lessonId: number, classId: number | null): SopController {
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loadSeq, setLoadSeq] = useState(0);
  const [material, setMaterial] = useState<SopMaterial | null>(null);
  const [materialDeleted, setMaterialDeleted] = useState(false);
  const materialIdRef = useRef<number | null>(null);
  const [draft, setDraftState] = useState<MaterialDraft>({ title: "", source_note: "", text: "" });
  const [content, setContent] = useState<SopContent>(emptyContent);
  const [jobs, setJobs] = useState<SopJobs>({ analysis: null, objectives: null });
  const [jobError, setJobError] = useState<Record<SopJobKind, string | null>>({ analysis: null, objectives: null });

  const contentRef = useRef(content);
  const draftRef = useRef(draft);

  useEffect(() => {
    let alive = true;
    api
      .getSop(lessonId)
      .then((state) => {
        if (!alive) return;
        const loaded = normalizeContent(state.content);
        const loadedDraft = {
          title: state.material?.title ?? "",
          source_note: state.material?.source_note ?? "",
          text: state.material?.text ?? "",
        };
        contentRef.current = loaded;
        draftRef.current = loadedDraft;
        setContent(loaded);
        setDraftState(loadedDraft);
        setMaterial(state.material ?? null);
        setMaterialDeleted(state.material_deleted ?? false);
        materialIdRef.current = state.material?.id ?? null;
        setJobs({ analysis: state.jobs?.analysis ?? null, objectives: state.jobs?.objectives ?? null });
        setLoadError(null);
        setStatus("ready");
      })
      .catch((err: unknown) => {
        if (!alive) return;
        setLoadError(errorMessage(err));
        setStatus("error");
      });
    return () => {
      alive = false;
    };
  }, [lessonId, loadSeq]);

  const saveContent = useCallback(async () => {
    const saved = await api.saveSop(lessonId, contentRef.current);
    return saved.saved_at;
  }, [lessonId]);
  const saveMaterial = useCallback(async () => {
    const state = await api.saveSopMaterial(lessonId, draftRef.current);
    // The server split the text: its paragraphs (and their ids) are what the other steps use.
    setMaterial(state.material ?? null);
    setMaterialDeleted(state.material_deleted ?? false);
    const nextId = state.material?.id ?? null;
    if (nextId !== materialIdRef.current) {
      // Another material: the server set the references to the old one's paragraphs aside.
      materialIdRef.current = nextId;
      const moved = normalizeContent(state.content);
      contentRef.current = moved;
      setContent(moved);
    }
    return state.saved_at;
  }, [lessonId]);
  const contentSave = useAutosave(saveContent);
  const materialSave = useAutosave(saveMaterial);

  const touchContent = contentSave.touch;
  const update = useCallback(
    (change: (content: SopContent) => SopContent) => {
      const next = change(contentRef.current);
      contentRef.current = next;
      setContent(next);
      touchContent();
    },
    [touchContent],
  );

  const touchMaterial = materialSave.touch;
  const setDraft = useCallback(
    (patch: Partial<MaterialDraft>) => {
      const next = { ...draftRef.current, ...patch };
      draftRef.current = next;
      setDraftState(next);
      touchMaterial();
    },
    [touchMaterial],
  );

  const flushMaterial = materialSave.flush;
  const flushContent = contentSave.flush;
  const flush = useCallback(async () => {
    const failed = await flushMaterial();
    if (failed) return failed;
    return flushContent();
  }, [flushMaterial, flushContent]);

  const pickMaterial = useCallback(
    async (materialId: number | null) => {
      // What is being typed belongs to the current material: save it before switching.
      const failed = await flush();
      if (failed) return `還沒有存好，所以還沒有換教材：${failed}`;
      try {
        const state = await api.saveSopMaterial(lessonId, { material_id: materialId });
        const next = state.material ?? null;
        const nextDraft = { title: next?.title ?? "", source_note: next?.source_note ?? "", text: next?.text ?? "" };
        draftRef.current = nextDraft;
        setDraftState(nextDraft);
        setMaterial(next);
        setMaterialDeleted(state.material_deleted ?? false);
        materialIdRef.current = next?.id ?? null;
        // Paragraph references of the previous material were set aside by the server.
        const moved = normalizeContent(state.content);
        contentRef.current = moved;
        setContent(moved);
        return null;
      } catch (err) {
        return errorMessage(err);
      }
    },
    [flush, lessonId],
  );

  // Poll the jobs that are still running; the teacher is free to work on other steps meanwhile.
  const analysisRunning = jobs.analysis?.status === "running" ? jobs.analysis.id : null;
  const objectivesRunning = jobs.objectives?.status === "running" ? jobs.objectives.id : null;
  useEffect(() => {
    const ids = [analysisRunning, objectivesRunning].filter((id): id is number => id !== null);
    if (ids.length === 0) return;
    let alive = true;
    const timer = setInterval(() => {
      for (const id of ids) {
        api
          .getSopJob(id)
          .then((job) => {
            if (!alive) return;
            if (job.status === "done" && job.result?.status === "shown") {
              logScaffold(SCAFFOLD[job.kind], "show", { trigger: "suggestions" });
            }
            setJobs((prev) => ({ ...prev, [job.kind]: job }));
          })
          // A missed poll is retried by the next tick.
          .catch(() => undefined);
      }
    }, sopConfig.pollMs);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [analysisRunning, objectivesRunning]);

  const startJob = useCallback(
    async (kind: SopJobKind) => {
      setJobError((prev) => ({ ...prev, [kind]: null }));
      logScaffold(SCAFFOLD[kind], "expand", { trigger: "request" });
      // The job reads what is saved: save what is pending first.
      const failed = await flush();
      if (failed) {
        setJobError((prev) => ({ ...prev, [kind]: `還沒有存好，所以還沒有請 Manna：${failed}` }));
        return;
      }
      try {
        const job = await api.startSopJob(lessonId, kind, classId);
        setJobs((prev) => ({ ...prev, [kind]: job }));
      } catch (err) {
        setJobError((prev) => ({ ...prev, [kind]: errorMessage(err) }));
      }
    },
    [flush, lessonId, classId],
  );

  const jobsRef = useRef(jobs);
  useEffect(() => {
    jobsRef.current = jobs;
  }, [jobs]);

  const decide = useCallback(
    (kind: SopJobKind, itemIndex: number, decision: DecisionKind, revisedContent?: string) => {
      const job = jobsRef.current[kind];
      if (!job) return;
      setJobs((prev) => {
        const current = prev[kind];
        if (!current || current.decided.includes(itemIndex)) return prev;
        return { ...prev, [kind]: { ...current, decided: [...current.decided, itemIndex] } };
      });
      const interactionId = job.result?.interaction_id;
      if (interactionId != null) {
        // Research logging never blocks the teacher: a failed decision log is dropped.
        void api
          .sendDecision(interactionId, { item_index: itemIndex, decision, revised_content: revisedContent })
          .catch(() => undefined);
      }
      logScaffold(SCAFFOLD[kind], decision === "reject" ? "skip" : "use", { trigger: `${decision}-${itemIndex}` });
    },
    [],
  );

  const reload = useCallback(() => {
    setStatus("loading");
    setLoadSeq((n) => n + 1);
  }, []);

  const savedAt = useMemo(() => {
    const times = [contentSave.savedAt, materialSave.savedAt].filter((t): t is string => !!t);
    return times.length > 0 ? times.sort()[times.length - 1] ?? null : null;
  }, [contentSave.savedAt, materialSave.savedAt]);

  return {
    status,
    loadError,
    reload,
    material,
    materialDeleted,
    pickMaterial,
    draft,
    setDraft,
    content,
    update,
    jobs,
    jobError,
    startJob,
    decide,
    dirty: contentSave.dirty || materialSave.dirty,
    saving: contentSave.saving || materialSave.saving,
    saveError: materialSave.error ?? contentSave.error,
    savedAt,
    flush,
  };
}
