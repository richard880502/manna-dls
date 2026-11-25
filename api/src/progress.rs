//! 產生進度 (docs/API.md「產生進度」): the steps of a background run as the teacher reads them
//! (「讀教材」「寫逐段任務」…), which one is being worked on, and how the run ended.
//!
//! The work reports every step change through a [`Tracker`], one small write per change: a
//! worksheet run into `worksheets.progress`, a prep turn into `prep_turns.progress`, drafting
//! objectives (a request the page waits for) into memory ([`Registry`]). The polling responses
//! read it back as a [`Progress`]: the stored steps plus what other rows already say (the
//! pictures of the run, a turn's SIOP 自我檢核), with a run that died settled as failed. A row
//! without stored steps (made before this existed) has no progress, and the pages show what
//! they always showed.
//!
//! Writing progress never fails the work: an error is logged and the run goes on.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::PgPool;

// ---------------------------------------------------------------- the steps

/// Step keys shared by several runs (the worksheet steps use their job names: `tasks`,
/// `warmup`, `scaffolds`, `exit_ticket`, `template`, `bloom:<level>`, `bloom_check`).
pub const READ: &str = "read";
pub const CURRICULUM: &str = "curriculum";
pub const PICTURES: &str = "pictures";
pub const SIOP: &str = "siop";
pub const PLAN: &str = "plan";
/// A worksheet turn that follows a template: reading the teacher's sample.
pub const SAMPLE: &str = "sample";

pub const L_READ: &str = "讀教材";
pub const L_CURRICULUM: &str = "找課綱條目";
pub const L_PICTURES: &str = "配圖";
/// The pictures step while some are still being drawn: 「配圖中（3／6）」.
pub const L_PICTURES_RUNNING: &str = "配圖中";
pub const L_PLAN: &str = "規劃教學活動";
pub const L_SIOP: &str = "SIOP 自我檢核";
pub const L_SAMPLE: &str = "讀你的學習單範本";
/// A step whose model call is being repaired (the second call of a step).
pub const NOTE_RETRY: &str = "重試中";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Failed,
}

/// Parts done of a step made of several (配圖 3／6, 照版型寫各區塊 2／8).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Count {
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Step {
    /// Stable within a run: `read`, `tasks`, `bloom:remember`, `pictures`, `siop`…
    pub key: String,
    /// What the teacher reads (Traditional Chinese, short).
    pub label: String,
    pub status: StepStatus,
    /// Said beside a running step, e.g. 「重試中」.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<Count>,
    /// A failed step: the error text the page already shows for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Step {
    pub fn new(key: &str, label: &str) -> Self {
        Self {
            key: key.to_string(),
            label: label.to_string(),
            status: StepStatus::Pending,
            note: None,
            count: None,
            message: None,
        }
    }

    pub fn with_status(mut self, status: StepStatus) -> Self {
        self.status = status;
        self
    }
}

/// What is stored: the steps of one run, in order.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Stored {
    /// The worksheet run (`worksheet_jobs.run_no`) these steps belong to; 0 for a turn.
    #[serde(default)]
    pub run_no: i32,
    #[serde(default)]
    pub steps: Vec<Step>,
    /// `worksheet_images` rows queued by the run: the pictures step is read from them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pictures: Vec<i64>,
    /// The work is over (its text part: pictures may still be drawn).
    #[serde(default)]
    pub finished: bool,
}

impl Stored {
    pub fn new(run_no: i32, mut steps: Vec<Step>) -> Self {
        if let Some(first) = steps.first_mut() {
            first.status = StepStatus::Running;
        }
        Self {
            run_no,
            steps,
            pictures: Vec::new(),
            finished: false,
        }
    }

    fn step_mut(&mut self, key: &str) -> Option<&mut Step> {
        self.steps.iter_mut().find(|s| s.key == key)
    }

    /// The work ended (`failure`: why it failed). A step still running is done, or failed with
    /// `failure`; on success the steps that never ran were skipped and are taken out. The
    /// pictures step stays (running) only when the run queued pictures. A failure that no
    /// step owns goes to the step that was next (or the last one).
    pub fn settle(&mut self, failure: Option<&str>) {
        if self.finished {
            return;
        }
        self.finished = true;
        let has_pictures = !self.pictures.is_empty();
        self.steps.retain(|s| s.key != PICTURES || has_pictures);
        // A step that already failed owns the failure.
        let mut owned = self.steps.iter().any(|s| s.status == StepStatus::Failed);
        for s in &mut self.steps {
            if s.key == PICTURES {
                s.status = StepStatus::Running;
                continue;
            }
            if s.status == StepStatus::Running {
                s.note = None;
                match failure {
                    Some(message) => {
                        s.status = StepStatus::Failed;
                        s.message.get_or_insert_with(|| message.to_string());
                        owned = true;
                    }
                    None => s.status = StepStatus::Done,
                }
            }
        }
        match failure {
            None => self.steps.retain(|s| s.status != StepStatus::Pending),
            Some(message) if !owned => {
                let at = self
                    .steps
                    .iter()
                    .position(|s| s.status == StepStatus::Pending)
                    .or_else(|| self.steps.iter().rposition(|s| s.key != PICTURES));
                if let Some(s) = at.and_then(|i| self.steps.get_mut(i)) {
                    s.status = StepStatus::Failed;
                    s.message.get_or_insert_with(|| message.to_string());
                }
            }
            Some(_) => {}
        }
    }
}

// ---------------------------------------------------------------- what the API returns

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Done,
    Failed,
}

/// `progress` of the polling responses.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Progress {
    pub steps: Vec<Step>,
    /// Index of the step being worked on (the first, when two run side by side); `null` once
    /// the run is over.
    pub current: Option<usize>,
    /// `running` while a step runs or is still to come; then `failed` when a step failed,
    /// otherwise `done`.
    pub status: RunStatus,
    /// While the run's job waits in the AI queue: where it stands (docs/API.md「AI 工作佇列」).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<QueueInfo>,
}

/// A run that waits in the AI queue.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QueueInfo {
    /// The job to cancel with `DELETE /api/ai-jobs/{job_id}`.
    pub job_id: i64,
    /// Queued jobs that will start before this one (an estimate).
    pub ahead: i64,
    /// About how long until it starts, from the last hour's runs; `null` without data.
    pub eta_secs: Option<i64>,
}

impl Progress {
    /// `open`: the work is still going, so pending steps are still to come.
    pub fn of(steps: Vec<Step>, open: bool) -> Option<Self> {
        if steps.is_empty() {
            return None;
        }
        let running = steps.iter().position(|s| s.status == StepStatus::Running);
        let pending = steps.iter().position(|s| s.status == StepStatus::Pending);
        let failed = steps.iter().any(|s| s.status == StepStatus::Failed);
        let status = if running.is_some() || (open && pending.is_some()) {
            RunStatus::Running
        } else if failed {
            RunStatus::Failed
        } else {
            RunStatus::Done
        };
        let current = match status {
            RunStatus::Running => running.or(pending),
            _ => None,
        };
        Some(Self {
            steps,
            current,
            status,
            queue: None,
        })
    }

    /// The run failed as a whole (a turn closed `failed`): a step says so, with `message`
    /// when no step failed on its own.
    pub fn failed(mut steps: Vec<Step>, message: &str) -> Option<Self> {
        if !steps.iter().any(|s| s.status == StepStatus::Failed) {
            let at = steps
                .iter()
                .position(|s| matches!(s.status, StepStatus::Running | StepStatus::Pending))
                .or_else(|| steps.len().checked_sub(1));
            if let Some(s) = at.and_then(|i| steps.get_mut(i)) {
                s.status = StepStatus::Failed;
                s.note = None;
                s.message = Some(message.to_string());
            }
        }
        // Nothing runs on after a failed turn.
        steps.retain(|s| s.status != StepStatus::Pending);
        for s in &mut steps {
            if s.status == StepStatus::Running {
                s.status = StepStatus::Failed;
                s.note = None;
            }
        }
        if steps.is_empty() {
            return None;
        }
        Some(Self {
            steps,
            current: None,
            status: RunStatus::Failed,
            queue: None,
        })
    }
}

/// One step for two parts of a run that both have it (a 教學套件 reads its material twice, once
/// for the plan and once for the worksheet): failed or running if either is, done if both are.
pub fn merge_status(a: StepStatus, b: StepStatus) -> StepStatus {
    use StepStatus::*;
    match (a, b) {
        (Failed, _) | (_, Failed) => Failed,
        (Running, _) | (_, Running) => Running,
        (Done, Done) => Done,
        (Pending, Pending) => Pending,
        _ => Running,
    }
}

/// `first` then `second`, a step of `second` whose key `first` has already merged into it.
pub fn merge(first: Vec<Step>, second: Vec<Step>) -> Vec<Step> {
    let mut out = first;
    for step in second {
        match out.iter_mut().find(|s| s.key == step.key) {
            Some(s) => {
                s.status = merge_status(s.status, step.status);
                if s.message.is_none() {
                    s.message = step.message;
                }
                if s.note.is_none() {
                    s.note = step.note;
                }
            }
            None => out.push(step),
        }
    }
    out
}

// ---------------------------------------------------------------- writing it

/// Where a run's steps are written.
#[derive(Clone)]
pub enum Sink {
    /// Nowhere (a run nobody polls).
    Off,
    Worksheet(PgPool, i64),
    Turn(PgPool, i64),
    /// In memory, for `teacher_id` and the page's own key.
    Memory(Arc<Registry>, i64, String),
}

/// Reports the step changes of one run. Cheap to share (`Arc`); every change is one write.
pub struct Tracker {
    sink: Sink,
    stored: tokio::sync::Mutex<Stored>,
    /// Writes made so far (tests: one per step change, none for a change that changes nothing).
    writes: std::sync::atomic::AtomicUsize,
}

impl Tracker {
    /// A tracker that writes nothing.
    pub fn off() -> Arc<Self> {
        Arc::new(Self {
            sink: Sink::Off,
            stored: tokio::sync::Mutex::new(Stored::default()),
            writes: Default::default(),
        })
    }

    /// Starts a run with these steps (the first one running) and writes them.
    pub async fn open(sink: Sink, run_no: i32, steps: Vec<Step>) -> Arc<Self> {
        let stored = Stored::new(run_no, steps);
        let tracker = Arc::new(Self {
            sink,
            stored: tokio::sync::Mutex::new(stored.clone()),
            writes: Default::default(),
        });
        tracker.write(&stored).await;
        tracker
    }

    async fn write(&self, stored: &Stored) {
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let result = match &self.sink {
            Sink::Off => Ok(()),
            Sink::Worksheet(db, id) => {
                sqlx::query("UPDATE worksheets SET progress = $2 WHERE id = $1")
                    .bind(id)
                    .bind(Json(stored))
                    .execute(db)
                    .await
                    .map(|_| ())
            }
            Sink::Turn(db, id) => sqlx::query("UPDATE prep_turns SET progress = $2 WHERE id = $1")
                .bind(id)
                .bind(Json(stored))
                .execute(db)
                .await
                .map(|_| ()),
            Sink::Memory(registry, teacher_id, key) => {
                registry.put(*teacher_id, key, stored.clone());
                Ok(())
            }
        };
        if let Err(e) = result {
            tracing::warn!(error = %e, "could not write the progress of a run");
        }
    }

    /// Applies `change`; writes when it changed something.
    async fn change(&self, change: impl FnOnce(&mut Stored) -> bool) {
        if matches!(self.sink, Sink::Off) {
            return;
        }
        let mut stored = self.stored.lock().await;
        if change(&mut stored) {
            self.write(&stored).await;
        }
    }

    /// The step is being worked on (nothing changes when it already is).
    pub async fn start(&self, key: &str) {
        self.change(|s| match s.step_mut(key) {
            Some(step) if step.status != StepStatus::Running => {
                step.status = StepStatus::Running;
                step.note = None;
                true
            }
            _ => false,
        })
        .await;
    }

    pub async fn done(&self, key: &str) {
        self.change(|s| match s.step_mut(key) {
            Some(step) if step.status != StepStatus::Done => {
                step.status = StepStatus::Done;
                step.note = None;
                true
            }
            _ => false,
        })
        .await;
    }

    /// The step failed; `message` is what the page already says about it.
    pub async fn fail(&self, key: &str, message: &str) {
        self.change(|s| match s.step_mut(key) {
            Some(step)
                if step.status != StepStatus::Failed
                    || step.message.as_deref() != Some(message) =>
            {
                step.status = StepStatus::Failed;
                step.note = None;
                step.message = Some(message.to_string());
                true
            }
            _ => false,
        })
        .await;
    }

    pub async fn note(&self, key: &str, note: Option<&str>) {
        self.change(|s| match s.step_mut(key) {
            Some(step) if step.note.as_deref() != note => {
                step.note = note.map(str::to_string);
                true
            }
            _ => false,
        })
        .await;
    }

    pub async fn count(&self, key: &str, done: usize, total: usize) {
        self.change(|s| match s.step_mut(key) {
            Some(step) => {
                let count = Some(Count { done, total });
                let changed = step.count != count;
                step.count = count;
                changed
            }
            None => false,
        })
        .await;
    }

    /// One more part of the step is done (its count must be set).
    pub async fn bump(&self, key: &str) {
        self.change(
            |s| match s.step_mut(key).and_then(|step| step.count.as_mut()) {
                Some(count) if count.done < count.total => {
                    count.done += 1;
                    true
                }
                _ => false,
            },
        )
        .await;
    }

    /// The step will not run (skipped): it is taken out of the list.
    pub async fn skip(&self, key: &str) {
        self.change(|s| {
            let before = s.steps.len();
            s.steps.retain(|step| step.key != key);
            s.steps.len() != before
        })
        .await;
    }

    /// Adds a step the run turned out to need, before the pictures step (or at the end).
    pub async fn add(&self, step: Step) {
        self.change(|s| {
            if s.steps.iter().any(|x| x.key == step.key) {
                return false;
            }
            let at = s
                .steps
                .iter()
                .position(|x| x.key == PICTURES)
                .unwrap_or(s.steps.len());
            s.steps.insert(at, step);
            true
        })
        .await;
    }

    /// The pictures the run queued (their `worksheet_images` ids).
    /// Nothing is written when there are none (the pictures step then goes when the run is
    /// settled): how far they are is read from their rows, never written here.
    pub async fn pictures(&self, ids: Vec<i64>) {
        self.change(|s| {
            if s.pictures == ids {
                return false;
            }
            s.pictures = ids;
            true
        })
        .await;
    }

    /// The work ended; see [`Stored::settle`]. Nothing changes once it was settled.
    pub async fn finish(&self, failure: Option<&str>) {
        self.change(|s| {
            if s.finished {
                return false;
            }
            s.settle(failure);
            true
        })
        .await;
    }

    /// How many writes this run made (tests).
    pub fn writes(&self) -> usize {
        self.writes.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The steps as they are now (tests).
    pub async fn snapshot(&self) -> Stored {
        self.stored.lock().await.clone()
    }
}

// ---------------------------------------------------------------- in memory

/// Progress of runs the page waits for in one request (drafting objectives), kept in memory
/// for a while after they end: the page asks for it with its own key while it waits.
#[derive(Default)]
pub struct Registry {
    runs: StdMutex<HashMap<(i64, String), (Instant, Stored)>>,
}

/// How long a run's progress is kept after its last change.
const KEEP: Duration = Duration::from_secs(15 * 60);
/// Most runs kept (the oldest go first).
const MAX_RUNS: usize = 2000;
/// A page's key: 8–64 of `A–Z a–z 0–9 - _`.
pub fn valid_key(key: &str) -> bool {
    (8..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Registry {
    pub fn put(&self, teacher_id: i64, key: &str, stored: Stored) {
        let Ok(mut runs) = self.runs.lock() else {
            return;
        };
        let now = Instant::now();
        runs.retain(|_, (at, _)| now.duration_since(*at) < KEEP);
        if runs.len() >= MAX_RUNS {
            if let Some(oldest) = runs
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| k.clone())
            {
                runs.remove(&oldest);
            }
        }
        runs.insert((teacher_id, key.to_string()), (now, stored));
    }

    /// The run of `teacher_id` with this key (another teacher's key finds nothing).
    pub fn get(&self, teacher_id: i64, key: &str) -> Option<Stored> {
        let runs = self.runs.lock().ok()?;
        runs.get(&(teacher_id, key.to_string()))
            .filter(|(at, _)| at.elapsed() < KEEP)
            .map(|(_, s)| s.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(keys: &[&str]) -> Vec<Step> {
        keys.iter().map(|k| Step::new(k, k)).collect()
    }

    fn statuses(s: &[Step]) -> Vec<(&str, StepStatus)> {
        s.iter().map(|x| (x.key.as_str(), x.status)).collect()
    }

    #[test]
    fn a_new_run_works_on_its_first_step() {
        let s = Stored::new(3, steps(&["read", "tasks", "pictures"]));
        let p = Progress::of(s.steps, true).unwrap();
        assert_eq!(p.status, RunStatus::Running);
        assert_eq!(p.current, Some(0));
        assert_eq!(Progress::of(Vec::new(), true), None);
    }

    #[test]
    fn between_two_steps_the_next_one_is_current() {
        let mut s = Stored::new(1, steps(&["read", "tasks"]));
        s.steps[0].status = StepStatus::Done;
        let p = Progress::of(s.steps.clone(), true).unwrap();
        assert_eq!((p.status, p.current), (RunStatus::Running, Some(1)));
        // Over: what never ran does not make it running.
        let p = Progress::of(s.steps, false).unwrap();
        assert_eq!((p.status, p.current), (RunStatus::Done, None));
    }

    #[test]
    fn settling_a_success_drops_what_never_ran_and_keeps_queued_pictures() {
        let mut s = Stored::new(1, steps(&["read", "tasks", "warmup", "pictures"]));
        s.steps[1].status = StepStatus::Running;
        s.steps[1].note = Some(NOTE_RETRY.to_string());
        s.pictures = vec![7, 8];
        s.settle(None);
        assert!(s.finished);
        assert_eq!(
            statuses(&s.steps),
            [
                ("read", StepStatus::Done),
                ("tasks", StepStatus::Done),
                ("pictures", StepStatus::Running)
            ]
        );
        assert_eq!(s.steps[1].note, None);
        // No pictures queued: the step goes.
        let mut s = Stored::new(1, steps(&["read", "pictures"]));
        s.settle(None);
        assert_eq!(statuses(&s.steps), [("read", StepStatus::Done)]);
    }

    #[test]
    fn settling_a_failure_fails_the_running_step_or_the_next_one() {
        let mut s = Stored::new(1, steps(&["read", "tasks", "warmup"]));
        s.steps[0].status = StepStatus::Done;
        s.steps[1].status = StepStatus::Running;
        s.settle(Some("中斷了"));
        assert_eq!(s.steps[1].status, StepStatus::Failed);
        assert_eq!(s.steps[1].message.as_deref(), Some("中斷了"));
        assert_eq!(s.steps[2].status, StepStatus::Pending);
        let p = Progress::of(s.steps.clone(), false).unwrap();
        assert_eq!((p.status, p.current), (RunStatus::Failed, None));
        // Settling twice changes nothing.
        let before = s.clone();
        s.settle(None);
        assert_eq!(s, before);
        // Between two steps: the next one takes the failure.
        let mut s = Stored::new(1, steps(&["read", "tasks"]));
        s.steps[0].status = StepStatus::Done;
        s.settle(Some("壞了"));
        assert_eq!(s.steps[1].status, StepStatus::Failed);
    }

    #[test]
    fn a_failed_turn_names_a_step_and_stops_the_rest() {
        let mut list = steps(&["read", "plan", "siop"]);
        list[0].status = StepStatus::Done;
        list[1].status = StepStatus::Running;
        let p = Progress::failed(list, "AI 目前沒有回應").unwrap();
        assert_eq!(p.status, RunStatus::Failed);
        assert_eq!(
            statuses(&p.steps),
            [("read", StepStatus::Done), ("plan", StepStatus::Failed)]
        );
        assert_eq!(p.steps[1].message.as_deref(), Some("AI 目前沒有回應"));
    }

    #[test]
    fn two_parts_share_a_step() {
        let mut a = steps(&["read", "plan"]);
        a[0].status = StepStatus::Done;
        a[1].status = StepStatus::Running;
        let b = vec![
            Step::new("read", "讀教材").with_status(StepStatus::Running),
            Step::new("bloom:remember", "x"),
        ];
        let m = merge(a, b);
        assert_eq!(
            statuses(&m),
            [
                ("read", StepStatus::Running),
                ("plan", StepStatus::Running),
                ("bloom:remember", StepStatus::Pending)
            ]
        );
        assert_eq!(
            merge_status(StepStatus::Done, StepStatus::Pending),
            StepStatus::Running
        );
        assert_eq!(
            merge_status(StepStatus::Done, StepStatus::Done),
            StepStatus::Done
        );
        assert_eq!(
            merge_status(StepStatus::Failed, StepStatus::Done),
            StepStatus::Failed
        );
    }

    #[tokio::test]
    async fn the_memory_tracker_reports_each_change() {
        let registry = Arc::new(Registry::default());
        let t = Tracker::open(
            Sink::Memory(registry.clone(), 5, "abcdefgh".to_string()),
            0,
            vec![
                Step::new("read", "讀教材"),
                Step::new("draft", "草擬學習目標"),
            ],
        )
        .await;
        assert_eq!(
            registry.get(5, "abcdefgh").unwrap().steps[0].status,
            StepStatus::Running
        );
        assert!(
            registry.get(6, "abcdefgh").is_none(),
            "another teacher sees nothing"
        );
        t.done("read").await;
        t.start("draft").await;
        t.note("draft", Some(NOTE_RETRY)).await;
        let s = registry.get(5, "abcdefgh").unwrap();
        assert_eq!(s.steps[1].status, StepStatus::Running);
        assert_eq!(s.steps[1].note.as_deref(), Some(NOTE_RETRY));
        t.add(Step::new("draft2", "重新草擬")).await;
        t.finish(None).await;
        let s = registry.get(5, "abcdefgh").unwrap();
        assert!(s.finished);
        assert_eq!(
            statuses(&s.steps),
            [("read", StepStatus::Done), ("draft", StepStatus::Done)]
        );
        // Off: nothing is kept anywhere.
        let off = Tracker::off();
        off.start("read").await;
        assert_eq!(off.snapshot().await, Stored::default());
    }

    #[tokio::test]
    async fn a_change_that_changes_nothing_writes_nothing() {
        let registry = Arc::new(Registry::default());
        let t = Tracker::open(
            Sink::Memory(registry, 1, "abcdefgh".to_string()),
            0,
            vec![
                Step::new("read", "讀教材"),
                Step::new("template", "照版型寫各區塊"),
            ],
        )
        .await;
        assert_eq!(t.writes(), 1, "the steps as they start");
        t.start("read").await; // already running
        t.done("read").await;
        t.done("read").await;
        t.count("template", 0, 2).await;
        t.count("template", 0, 2).await;
        t.bump("template").await;
        t.bump("template").await;
        t.bump("template").await; // already 2／2
        t.pictures(Vec::new()).await; // none queued
        t.fail("template", "壞了").await;
        t.fail("template", "壞了").await;
        t.skip("nothing").await;
        t.finish(None).await;
        t.finish(None).await;
        // open, done, count, bump, bump, fail, finish.
        assert_eq!(t.writes(), 7);
    }

    #[test]
    fn keys_are_checked() {
        assert!(valid_key("abcd-1234_XY"));
        assert!(!valid_key("short"));
        assert!(!valid_key("has space in it"));
        assert!(!valid_key(&"a".repeat(65)));
    }
}
