import type { RecordEvent, RecordOp, SyncResponse } from "../api/types";

export function recordEvent(id: string, device_ts: string, overrides: Partial<RecordEvent> = {}): RecordEvent {
  return {
    id: 1,
    device_event_id: id,
    device_ts,
    received_at: device_ts,
    polarity: "good",
    note: "",
    device_type: "desktop",
    via: "console",
    activity_step_id: null,
    behaviors: [],
    handling: null,
    followup_text: "",
    followup_at: null,
    revision_count: 0,
    ...overrides,
  };
}

/** In-memory stand-in for POST /api/runs/:id/events/sync with the API's idempotency rules. */
export function fakeRecordServer() {
  const events = new Map<string, RecordEvent>();
  const seen = new Set<string>();
  let nextId = 1;
  const apply = (ops: RecordOp[]): SyncResponse => {
    const results = ops.map((op) => {
      if (op.kind === "create") {
        if (events.has(op.device_event_id)) return { op_id: op.op_id, status: "duplicate" as const };
        events.set(
          op.device_event_id,
          recordEvent(op.device_event_id, op.device_ts, {
            id: nextId++,
            polarity: op.polarity,
            note: op.note ?? "",
            device_type: op.device_type,
            via: op.via ?? "console",
          }),
        );
        return { op_id: op.op_id, status: "applied" as const };
      }
      if (seen.has(op.op_id)) return { op_id: op.op_id, status: "duplicate" as const };
      seen.add(op.op_id);
      const ev = events.get(op.device_event_id);
      if (!ev) return { op_id: op.op_id, status: "rejected" as const, code: "not_found", message: "找不到這個書籤。" };
      if (op.kind === "edit") {
        events.set(op.device_event_id, { ...ev, polarity: op.polarity ?? ev.polarity, note: op.note ?? ev.note });
      } else {
        events.delete(op.device_event_id);
      }
      return { op_id: op.op_id, status: "applied" as const };
    });
    return { results, events: [...events.values()], ended_at: null };
  };
  return { events, apply };
}
