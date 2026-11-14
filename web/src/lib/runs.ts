import { ApiError, api } from "../api/client";

/**
 * 開始本節課 in `classId` (omitted: the server picks the unit's only class, or none). If the
 * lesson already has an open run in that class (409), returns that one instead.
 */
export async function startOrResumeRun(lessonId: number, classId?: number | null): Promise<number> {
  try {
    return (await api.startRun(lessonId, classId)).id;
  } catch (err) {
    if (err instanceof ApiError && err.status === 409) {
      const open = (await api.listLessonRuns(lessonId)).find(
        (r) => r.ended_at === null && (!classId || r.class_id === classId),
      );
      if (open) return open.id;
    }
    throw err;
  }
}

/** Scope of the device-side queue for a run (shared by the console, PiP and review page). */
export const runQueueScope = (runId: number) => `run:${runId}`;
