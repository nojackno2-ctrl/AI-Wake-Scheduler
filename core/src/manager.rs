//! 管理 AI 喚醒排程的初始化、掃描、手動觸發與背景執行。
//!
//! 只依賴 `DataStore`、`CliRunner`、`UsageReader` 與 `Clock`，
//! 不知道資料存在哪裡、CLI 怎麼啟動，也不知道現在幾點。

use chrono::Timelike;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::calculator;
use crate::debug_log;
use crate::error::{CoreError, CoreResult};
use crate::models::{
    AppSettings, CliActivityEvent, CliActivityKind, CliKind, CliProfile, CliRunResult, Guid, ScheduleRecurrence,
    ScheduleStatus, ScheduledJob, SCHEDULE_REVISION,
};
use crate::runner::CliRunner;
use crate::store::DataStore;
use crate::time::{self, CancelToken, Clock, Moment, Span, SystemClock};
use crate::usage::{
    active_reset_at, exhausted_until, is_counting_down, CliUsageAvailability, CliUsageSnapshot, UsageReader,
};

/// 閒置時最長的等待間隔。
/// 排程器不每秒輪詢，而是直接睡到下一個到期時間；這個上限只是為了
/// 在系統休眠、時鐘調整或時區變更後仍能在可接受的時間內重新校準。
const MAX_IDLE_WAIT_SECONDS: i64 = 30;

/// 最短等待間隔，避免時間已到但狀態尚未落地時空轉。
const MIN_WAIT_MILLISECONDS: i64 = 200;

const QUOTA_WAKEUP_COOLDOWN_MINUTES: i64 = 2;

#[derive(Clone, Copy)]
struct RunOptions {
    timeout: Duration,
    token_saver_mode: bool,
    automatic: bool,
}

type Listener<T> = Arc<dyn Fn(&T) + Send + Sync>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn max_idle_wait() -> Span {
    Span::seconds(MAX_IDLE_WAIT_SECONDS)
}

fn quota_wakeup_cooldown() -> Span {
    Span::minutes(QUOTA_WAKEUP_COOLDOWN_MINUTES)
}

fn initial_time_of(job: &ScheduledJob) -> Span {
    job.initial_time_of_day.unwrap_or_else(|| time::local_time_of_day(&job.scheduled_at))
}

/// 除錯日誌用的本機時間文字。
fn local_text(moment: &Moment) -> String {
    time::local_naive(moment).format("%Y-%m-%d %H:%M:%S").to_string()
}

fn kinds_text(kinds: &[CliKind]) -> String {
    kinds.iter().map(|kind| kind.short_name()).collect::<Vec<_>>().join("、")
}

fn describe_job(job: &ScheduledJob) -> String {
    let start = job
        .initial_time_of_day
        .map(|start| format!("{:02}:{:02}", start.num_hours(), start.num_minutes() % 60))
        .unwrap_or_else(|| "無".to_string());
    format!(
        "「{}」每日 {start}–{:02}:{:02} 目標 [{}] 狀態 {}{} 下次 {}",
        job.name,
        job.end_time_of_day.num_hours(),
        job.end_time_of_day.num_minutes() % 60,
        kinds_text(&job.targets),
        job.status,
        if job.enabled { "" } else { "（停用）" },
        local_text(&job.scheduled_at)
    )
}

struct Inner {
    job_store: Arc<dyn DataStore<Vec<ScheduledJob>>>,
    cli_runner: Arc<dyn CliRunner>,
    usage_reader: Option<Arc<dyn UsageReader>>,
    settings_provider: Box<dyn Fn() -> AppSettings + Send + Sync>,
    clock: Arc<dyn Clock>,

    /// 排程清單；鎖同時保護資料檔寫入的先後順序。
    jobs: Mutex<Vec<ScheduledJob>>,
    /// 僅在 jobs 鎖內讀寫；已執行的結果即使存檔失敗仍須保留並重試。
    persistence_pending: AtomicBool,
    last_quota_wakeup_by_cli: Mutex<HashMap<CliKind, Moment>>,
    last_quota_probe_at: Mutex<HashMap<(Guid, CliKind), Moment>>,
    known_reset_at: Mutex<HashMap<(Guid, CliKind), Moment>>,
    active_targets: Mutex<HashSet<CliKind>>,

    /// 排程異動時用來立刻喚醒背景迴圈，不必等到下一個間隔。
    wake_pending: Mutex<bool>,
    wake_signal: Condvar,
    stop: CancelToken,
    threads: Mutex<Vec<JoinHandle<()>>>,
    disposed: AtomicBool,

    jobs_changed: Mutex<Vec<Listener<()>>>,
    background_error: Mutex<Vec<Listener<CoreError>>>,
    activity_logged: Mutex<Vec<Listener<CliActivityEvent>>>,
}

pub struct ScheduleManager {
    inner: Arc<Inner>,
}

impl ScheduleManager {
    pub fn new(
        job_store: Arc<dyn DataStore<Vec<ScheduledJob>>>,
        cli_runner: Arc<dyn CliRunner>,
        settings_provider: impl Fn() -> AppSettings + Send + Sync + 'static,
        usage_reader: Option<Arc<dyn UsageReader>>,
        clock: Option<Arc<dyn Clock>>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                job_store,
                cli_runner,
                usage_reader,
                settings_provider: Box::new(settings_provider),
                clock: clock.unwrap_or_else(|| Arc::new(SystemClock)),
                jobs: Mutex::new(Vec::new()),
                persistence_pending: AtomicBool::new(false),
                last_quota_wakeup_by_cli: Mutex::new(HashMap::new()),
                last_quota_probe_at: Mutex::new(HashMap::new()),
                known_reset_at: Mutex::new(HashMap::new()),
                active_targets: Mutex::new(HashSet::new()),
                wake_pending: Mutex::new(false),
                wake_signal: Condvar::new(),
                stop: CancelToken::new(),
                threads: Mutex::new(Vec::new()),
                disposed: AtomicBool::new(false),
                jobs_changed: Mutex::new(Vec::new()),
                background_error: Mutex::new(Vec::new()),
                activity_logged: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn on_jobs_changed(&self, listener: impl Fn() + Send + Sync + 'static) {
        lock(&self.inner.jobs_changed).push(Arc::new(move |_: &()| listener()));
    }

    pub fn on_background_error(&self, listener: impl Fn(&CoreError) + Send + Sync + 'static) {
        lock(&self.inner.background_error).push(Arc::new(listener));
    }

    /// 單一 CLI 呼叫的即時活動（開始、輸出片段、完成/失敗），供 UI 端的終端機面板顯示。
    pub fn on_activity_logged(&self, listener: impl Fn(&CliActivityEvent) + Send + Sync + 'static) {
        lock(&self.inner.activity_logged).push(Arc::new(listener));
    }

    pub fn initialize(&self) -> CoreResult<()> {
        self.inner.ensure_not_disposed()?;
        let inner = &self.inner;
        let mut loaded = inner.job_store.load()?;
        let original = loaded.clone();
        let now = inner.now();
        let mut ids = std::collections::HashSet::new();

        for job in &mut loaded {
            if !ids.insert(job.id) {
                return Err(CoreError::InvalidData("排程識別碼重複，未儲存任何變更。".to_string()));
            }
            if !(0..=SCHEDULE_REVISION).contains(&job.schedule_revision) {
                return Err(CoreError::InvalidData("不支援此排程資料版本。".to_string()));
            }
            let mut seen = HashSet::new();
            job.targets.retain(|target| seen.insert(*target));
            // 舊週期只供資料相容；一律沿用開始時間並採每日時段偵測。
            let initial_time = initial_time_of(job);
            job.recurrence = ScheduleRecurrence::Interval;
            job.initial_time_of_day = Some(initial_time);
            if !time::is_valid_time_of_day(initial_time) {
                return Err(CoreError::InvalidData("開始時間必須介於 00:00 到 23:59。".to_string()));
            }
            job.schedule_revision = SCHEDULE_REVISION;

            calculator::validate_window(time::truncate_to_minutes(initial_time), job.end_time_of_day)?;
            job.initial_time_of_day = Some(time::truncate_to_minutes(initial_time));
            calculator::next_aligned_detection(
                initial_time_of(job),
                job.end_time_of_day,
                &now,
                job.probe_interval_minutes,
                true,
            )?;
            // 保留尚未到期與歷史資料；到期只準備偵測，不會直接呼叫 CLI。
            if job.enabled
                && matches!(job.status, ScheduleStatus::Pending | ScheduleStatus::Running)
                && job.scheduled_at <= now
            {
                job.scheduled_at = calculator::next_aligned_detection(
                    initial_time_of(job),
                    job.end_time_of_day,
                    &now,
                    job.probe_interval_minutes,
                    true,
                )?;
            }

            if job.status == ScheduleStatus::Running {
                // 上次是被強制關閉的，不可能還在跑。
                job.status = if job.enabled { ScheduleStatus::Pending } else { ScheduleStatus::Disabled };
                job.started_at = None;
            }
        }

        if inner.usage_reader.is_none() {
            debug_log::write("偵測", "未提供額度讀取器，停止自動呼叫；仍可手動立即執行。");
        }
        debug_log::write("排程", format!("載入 {} 個排程（現在 {}）", loaded.len(), local_text(&now)));
        for job in &loaded {
            debug_log::write("排程", format!("載入 {}", describe_job(job)));
        }
        {
            let mut jobs = lock(&inner.jobs);
            // 所有資料成功解析、遷移並儲存後，才發布至記憶體與啟動背景排程。
            if loaded != original {
                inner.job_store.save(&loaded)?;
            }
            *jobs = loaded;
        }

        let loop_inner = Arc::clone(inner);
        let handle = std::thread::spawn(move || loop_inner.run_scheduler_loop());
        lock(&inner.threads).push(handle);
        inner.raise_jobs_changed();
        Ok(())
    }

    pub fn get_jobs(&self) -> CoreResult<Vec<ScheduledJob>> {
        self.inner.ensure_not_disposed()?;
        let jobs = lock(&self.inner.jobs);
        let mut snapshot: Vec<ScheduledJob> = jobs.iter().map(ScheduledJob::snapshot).collect();
        snapshot.sort_by_key(|job| job.scheduled_at);
        Ok(snapshot)
    }

    pub fn upsert(&self, job: &ScheduledJob) -> CoreResult<()> {
        let inner = &self.inner;
        inner.ensure_not_disposed()?;
        validate_job(job)?;
        {
            let mut current = lock(&inner.jobs);
            let mut jobs = current.clone();
            let now = inner.now();
            let mut saved = job.snapshot();
            let mut seen = HashSet::new();
            saved.targets.retain(|target| seen.insert(*target));
            // 舊週期欄位只保留相容標記。
            let initial_time = initial_time_of(&saved);
            saved.recurrence = ScheduleRecurrence::Interval;
            saved.schedule_revision = SCHEDULE_REVISION;
            saved.initial_time_of_day = Some(initial_time);
            saved.initial_time_of_day = Some(time::truncate_to_minutes(initial_time));
            saved.scheduled_at = calculator::next_aligned_detection(
                initial_time_of(&saved),
                saved.end_time_of_day,
                &now,
                saved.probe_interval_minutes,
                true,
            )?;
            saved.status = if saved.enabled { ScheduleStatus::Pending } else { ScheduleStatus::Disabled };
            saved.started_at = None;
            saved.finished_at = None;
            saved.last_results = Vec::new();

            match jobs.iter().position(|item| item.id == saved.id) {
                Some(index) => {
                    if jobs[index].status == ScheduleStatus::Running {
                        return Err(CoreError::InvalidOperation("排程正在執行，暫時不能修改。".to_string()));
                    }
                    jobs[index] = saved;
                }
                None => jobs.push(saved),
            }
            if let Some(stored) = jobs.iter().find(|item| item.id == job.id) {
                debug_log::write("排程", format!("儲存 {}", describe_job(stored)));
            }

            inner.job_store.save(&jobs)?;
            *current = jobs;
            lock(&inner.last_quota_probe_at).retain(|(id, _), _| *id != job.id);
            lock(&inner.known_reset_at).retain(|(id, _), _| *id != job.id);
        }

        inner.raise_jobs_changed();
        inner.signal_scheduler();
        Ok(())
    }

    pub fn delete(&self, id: Guid) -> CoreResult<()> {
        let inner = &self.inner;
        inner.ensure_not_disposed()?;
        {
            let mut current = lock(&inner.jobs);
            let mut jobs = current.clone();
            if jobs.iter().any(|job| job.id == id && job.status == ScheduleStatus::Running) {
                return Err(CoreError::InvalidOperation("排程正在執行，暫時不能刪除。".to_string()));
            }
            if let Some(job) = jobs.iter().find(|job| job.id == id) {
                debug_log::write("排程", format!("刪除「{}」", job.name));
            }
            jobs.retain(|job| job.id != id);
            inner.job_store.save(&jobs)?;
            *current = jobs;
            lock(&inner.last_quota_probe_at).retain(|(job_id, _), _| *job_id != id);
            lock(&inner.known_reset_at).retain(|(job_id, _), _| *job_id != id);
        }

        inner.raise_jobs_changed();
        inner.signal_scheduler();
        Ok(())
    }

    pub fn run_now(&self, id: Guid) -> CoreResult<()> {
        let inner = &self.inner;
        inner.ensure_not_disposed()?;
        let execution = {
            let mut current = lock(&inner.jobs);
            let mut jobs = current.clone();
            let job = jobs
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| CoreError::InvalidOperation("找不到指定排程。".to_string()))?;
            if job.status == ScheduleStatus::Running {
                return Err(CoreError::InvalidOperation("排程已經在執行中。".to_string()));
            }

            let mut active = lock(&inner.active_targets);
            if job.targets.iter().any(|target| active.contains(target)) {
                return Err(CoreError::InvalidOperation("選取的 CLI 已有其他排程正在執行。".to_string()));
            }
            job.enabled = true;
            job.status = ScheduleStatus::Running;
            job.started_at = Some(inner.now());
            job.finished_at = None;
            job.last_results = Vec::new();
            let execution = job.snapshot();
            inner.job_store.save(&jobs)?;
            *current = jobs;
            active.extend(execution.targets.iter().copied());
            execution
        };

        let now = inner.now();
        inner.mark_wakeup(&execution.targets, now);

        debug_log::write(
            "執行",
            format!("手動立即執行「{}」目標 [{}]", execution.name, kinds_text(&execution.targets)),
        );
        inner.raise_jobs_changed();
        let targets = execution.targets.clone();
        inner.start_execution(execution, targets, inner.stop.clone(), false);
        Ok(())
    }

    /// 停止背景迴圈並等待進行中的執行結束。
    pub fn shutdown(&self) {
        let inner = &self.inner;
        if inner.disposed.swap(true, Ordering::SeqCst) {
            return;
        }

        inner.stop.cancel();
        inner.signal_scheduler();
        loop {
            let handles = std::mem::take(&mut *lock(&inner.threads));
            if handles.is_empty() {
                break;
            }
            for handle in handles {
                // 忽略關閉期間仍在執行的背景工作錯誤
                let _ = handle.join();
            }
        }
    }
}

impl Drop for ScheduleManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Inner {
    fn now(&self) -> Moment {
        self.clock.now()
    }

    fn ensure_not_disposed(&self) -> CoreResult<()> {
        if self.disposed.load(Ordering::SeqCst) {
            Err(CoreError::InvalidOperation("排程管理器已關閉。".to_string()))
        } else {
            Ok(())
        }
    }

    /// 背景排程迴圈。每一輪掃描到期排程後，直接睡到下一個到期時間
    /// （上限 `MAX_IDLE_WAIT_SECONDS`），而不是固定每秒喚醒。
    fn run_scheduler_loop(self: Arc<Self>) {
        while !self.stop.is_canceled() {
            if let Err(error) = self.retry_pending_persistence() {
                self.raise_background_error(&error);
                self.wait_for_next_round(Span::seconds(1));
                continue;
            }
            match self.scan_and_start_due_jobs() {
                Ok(next_delay) => self.wait_for_next_round(next_delay),
                Err(CoreError::Canceled) if self.stop.is_canceled() => break,
                Err(error) => {
                    self.raise_background_error(&error);
                    self.wait_for_next_round(Span::seconds(1));
                }
            }
        }
    }

    fn retry_pending_persistence(&self) -> CoreResult<()> {
        let jobs = lock(&self.jobs);
        if self.persistence_pending.load(Ordering::SeqCst) {
            self.job_store.save(&jobs)?;
            self.persistence_pending.store(false, Ordering::SeqCst);
        }
        Ok(())
    }

    /// 等待下一輪掃描，期間若有排程異動會被 `signal_scheduler` 立即喚醒。
    fn wait_for_next_round(&self, delay: Span) {
        let delay = delay.clamp(Span::milliseconds(MIN_WAIT_MILLISECONDS), max_idle_wait());
        let deadline = std::time::Instant::now() + time::to_std(delay);

        // 逾時代表「睡到下一個到期時間」，取得訊號代表「排程被改動了，立刻重新評估」。
        let mut pending = lock(&self.wake_pending);
        while !*pending && !self.stop.is_canceled() {
            let now = std::time::Instant::now();
            if now >= deadline {
                break;
            }
            pending = self.wake_signal.wait_timeout(pending, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
        *pending = false;
    }

    fn signal_scheduler(&self) {
        *lock(&self.wake_pending) = true;
        self.wake_signal.notify_all();
    }

    fn mark_wakeup(&self, targets: &[CliKind], now: Moment) {
        let mut last_wakeup = lock(&self.last_quota_wakeup_by_cli);
        let mut known = lock(&self.known_reset_at);
        for target in targets {
            last_wakeup.insert(*target, now);
            known.retain(|(_, kind), _| kind != target);
        }
    }

    fn in_wakeup_cooldown(&self, kind: CliKind, now: &Moment) -> Option<Moment> {
        lock(&self.last_quota_wakeup_by_cli).get(&kind).copied().filter(|last| *now - *last < quota_wakeup_cooldown())
    }

    /// 排程自己的重置時間不受 UI 重新整理共用快照影響。
    fn remember_reset(&self, id: Guid, kind: CliKind, value: &CliUsageSnapshot, now: &Moment) {
        let mut known = lock(&self.known_reset_at);
        if let Some(reset) = active_reset_at(Some(value), now) {
            known.insert((id, kind), reset);
        } else {
            known.remove(&(id, kind));
        }
    }

    fn known_reset(&self, reader: &dyn UsageReader, id: Guid, kind: CliKind, now: &Moment) -> Option<Moment> {
        if let Some(reset) = lock(&self.known_reset_at).get(&(id, kind)).copied() {
            return Some(reset);
        }
        let last_probe = lock(&self.last_quota_probe_at).get(&(id, kind)).copied();
        let latest = reader.latest_snapshot(kind)?;
        if last_probe.is_some_and(|last| last > latest.observed_at) {
            return None;
        }
        let reset = active_reset_at(Some(&latest), now)?;
        lock(&self.known_reset_at).insert((id, kind), reset);
        Some(reset)
    }

    fn next_job_schedule_time(&self, job: &ScheduledJob, now: &Moment) -> CoreResult<Moment> {
        let start = initial_time_of(job);
        let end = job.end_time_of_day;
        let next_aligned = calculator::next_aligned_detection(start, end, now, job.probe_interval_minutes, false)?;
        let Some(reader) = &self.usage_reader else {
            return Ok(next_aligned);
        };
        if job.targets.is_empty() {
            return Ok(next_aligned);
        }
        let (begin, _) = calculator::detection_window(start, end, now)?;
        let mut next: Option<Moment> = None;
        for &kind in &job.targets {
            let mut target_next = match self.known_reset(reader.as_ref(), job.id, kind, now) {
                Some(reset) if reset > *now => {
                    if calculator::in_detection_window(start, end, &reset) {
                        reset
                    } else {
                        calculator::detection_window(start, end, &reset)?.0
                    }
                }
                Some(_) if *now < begin => begin,
                Some(_) => *now,
                None => next_aligned,
            };
            if let Some(last) = self.in_wakeup_cooldown(kind, now) {
                let cooldown_end = last + quota_wakeup_cooldown();
                let allowed = if calculator::in_detection_window(start, end, &cooldown_end) {
                    cooldown_end
                } else {
                    calculator::detection_window(start, end, &cooldown_end)?.0
                };
                target_next = target_next.max(allowed);
            }
            if lock(&self.active_targets).contains(&kind) {
                target_next = target_next.max(*now + max_idle_wait());
            }
            next = Some(next.map_or(target_next, |current| current.min(target_next)));
        }
        Ok(next.unwrap_or(next_aligned))
    }

    /// 排程自己的重置時間不受 UI 重新整理共用快照影響。
    /// 僅在沒有讀到倒數時依設定間隔讀取；已讀到倒數時記住下次倒數結束時間，倒數結束立即重讀並喚醒。
    fn scan_and_start_due_jobs(self: &Arc<Self>) -> CoreResult<Span> {
        let Some(reader) = &self.usage_reader else {
            return Ok(max_idle_wait());
        };
        let settings = (self.settings_provider)().normalized();
        let snapshot = lock(&self.jobs).clone();
        for job in snapshot {
            if !job.enabled || job.status != ScheduleStatus::Pending {
                continue;
            }
            let now = self.now();
            if !calculator::in_detection_window(initial_time_of(&job), job.end_time_of_day, &now) {
                continue;
            }
            let mut targets = Vec::new();
            for &kind in &job.targets {
                let now = self.now();
                if self.stop.is_canceled() {
                    return Err(CoreError::Canceled);
                }
                if !calculator::in_detection_window(initial_time_of(&job), job.end_time_of_day, &now) {
                    break;
                }
                if lock(&self.active_targets).contains(&kind) {
                    continue;
                }
                if let Some(last) = self.in_wakeup_cooldown(kind, &now) {
                    debug_log::write(
                        "偵測",
                        format!("{} 於 {} 呼叫過，冷卻中略過", kind.short_name(), local_text(&last)),
                    );
                    continue;
                }
                let last = lock(&self.last_quota_probe_at).get(&(job.id, kind)).copied();
                let slot = now - Span::seconds(now.second() as i64) - Span::nanoseconds(now.nanosecond() as i64);
                let known_reset = self.known_reset(reader.as_ref(), job.id, kind, &now);
                if let Some(reset) = known_reset.filter(|reset| *reset > now) {
                    if calculator::aligned_minute(&now, job.probe_interval_minutes)
                        && last.is_none_or(|last| last < slot)
                    {
                        debug_log::write(
                            "偵測",
                            format!("{} 已記住倒數結束時間 {}，略過讀取", kind.short_name(), local_text(&reset)),
                        );
                    }
                    continue;
                }
                let countdown_ended =
                    known_reset.is_some_and(|reset| reset <= now && last.is_none_or(|last| last < reset));
                if !countdown_ended
                    && (!calculator::aligned_minute(&now, job.probe_interval_minutes)
                        || last.is_some_and(|last| last >= slot))
                {
                    continue;
                }
                let result = reader.read(kind, &settings.profile(kind), &job.working_directory, &self.stop);
                let observed = self.now();
                lock(&self.last_quota_probe_at).insert((job.id, kind), observed);
                match &result {
                    Ok(value) => self.remember_reset(job.id, kind, value, &observed),
                    Err(_) => {
                        lock(&self.known_reset_at).remove(&(job.id, kind));
                    }
                }
                if !calculator::in_detection_window(initial_time_of(&job), job.end_time_of_day, &observed) {
                    break;
                }
                if !countdown_ended && !calculator::aligned_minute(&observed, job.probe_interval_minutes) {
                    break;
                }
                match result {
                    Ok(value) => {
                        // 權杖逾期時讀不到額度，但呼叫 CLI 正好會換新權杖；不呼叫就會一直卡在讀不到。
                        let reason = if value.credentials_stale {
                            debug_log::write(
                                "偵測",
                                format!("{} 存取權杖已逾期，呼叫 CLI 以換新權杖", kind.short_name()),
                            );
                            None
                        } else if value.availability != CliUsageAvailability::Available || value.windows.is_empty() {
                            Some("額度無法確認，不呼叫".to_string())
                        } else if let Some(reset) = exhausted_until(Some(&value), &observed) {
                            Some(format!("額度已用盡，至 {} 重置前不呼叫", local_text(&reset)))
                        } else if value.windows.iter().any(|window| window.used_percent >= 100) {
                            Some("額度已用盡且尚未確認重置，不呼叫".to_string())
                        } else if is_counting_down(Some(&value), &observed) {
                            Some("倒數中，不呼叫".to_string())
                        } else {
                            None
                        };
                        if let Some(reason) = reason {
                            debug_log::write("偵測", format!("{} {reason}", kind.short_name()));
                        } else if !targets.contains(&kind) {
                            targets.push(kind);
                        }
                    }
                    Err(CoreError::Canceled) if self.stop.is_canceled() => return Err(CoreError::Canceled),
                    Err(error) => {
                        debug_log::write("偵測", format!("{} 讀取失敗，不呼叫：{}", kind.short_name(), error))
                    }
                }
            }
            if targets.is_empty() {
                continue;
            }
            let execution = {
                let mut current = lock(&self.jobs);
                let mut jobs = current.clone();
                let mut active = lock(&self.active_targets);
                let now = self.now();
                let stored = jobs.iter_mut().find(|stored| stored.id == job.id && **stored == job);
                if let Some(stored) = stored {
                    targets
                        .retain(|target| !active.contains(target) && self.in_wakeup_cooldown(*target, &now).is_none());
                    if !targets.is_empty()
                        && calculator::in_detection_window(initial_time_of(stored), stored.end_time_of_day, &now)
                    {
                        stored.status = ScheduleStatus::Running;
                        stored.started_at = Some(now);
                        let execution = stored.clone();
                        self.job_store.save(&jobs)?;
                        *current = jobs;
                        active.extend(targets.iter().copied());
                        Some(execution)
                    } else {
                        None
                    }
                } else {
                    None
                }
            };
            if let Some(execution) = execution {
                debug_log::write("偵測", format!("「{}」未倒數 [{}]，立即呼叫", execution.name, kinds_text(&targets)));
                self.mark_wakeup(&targets, self.now());
                self.raise_jobs_changed();
                self.start_execution(execution, targets, self.stop.clone(), true);
            }
        }
        let now = self.now();
        let mut delay = max_idle_wait();
        let mut changed = false;
        {
            let mut current = lock(&self.jobs);
            let mut jobs = current.clone();
            for job in jobs.iter_mut().filter(|job| job.enabled && job.status == ScheduleStatus::Pending) {
                let next = self.next_job_schedule_time(job, &now)?;
                if job.scheduled_at != next {
                    job.scheduled_at = next;
                    changed = true;
                }
                delay = delay.min(next - now);
            }
            if changed {
                self.job_store.save(&jobs)?;
                *current = jobs;
            }
        }
        if changed {
            self.raise_jobs_changed();
        }
        Ok(delay)
    }

    fn start_execution(
        self: &Arc<Self>,
        job: ScheduledJob,
        targets: Vec<CliKind>,
        cancel: CancelToken,
        automatic: bool,
    ) {
        let inner = Arc::clone(self);
        let handle = std::thread::spawn(move || inner.execute_job(job, targets, cancel, automatic));
        let mut threads = lock(&self.threads);
        threads.retain(|existing| !existing.is_finished());
        threads.push(handle);
    }

    fn execute_job(&self, job: ScheduledJob, targets: Vec<CliKind>, cancel: CancelToken, automatic: bool) {
        let settings = (self.settings_provider)().normalized();
        let timeout = Duration::from_secs(settings.execution_timeout_minutes.max(1) as u64 * 60);

        // 保留執行中目標至收尾（含執行後重讀），避免重疊排程再次呼叫。
        struct Reservation<'a>(&'a Inner, &'a [CliKind]);
        impl Drop for Reservation<'_> {
            fn drop(&mut self) {
                {
                    let mut active = lock(&self.0.active_targets);
                    for kind in self.1 {
                        active.remove(kind);
                    }
                }
                // 釋放目標後重新計算下一個刻度。
                self.0.signal_scheduler();
            }
        }
        let _reservation = Reservation(self, &targets);
        // 每個選定的 CLI 都在自己的執行緒獨立啟動，不會變成循序執行。
        let results: Option<Vec<CliRunResult>> = std::thread::scope(|scope| {
            let handles: Vec<_> = targets
                .iter()
                .map(|&target| {
                    let profile = settings.profile(target);
                    let options = RunOptions { timeout, token_saver_mode: settings.token_saver_mode, automatic };
                    let (job, cancel) = (&job, &cancel);
                    scope.spawn(move || {
                        if cancel.is_canceled()
                            || (automatic
                                && !calculator::in_detection_window(
                                    initial_time_of(job),
                                    job.end_time_of_day,
                                    &self.now(),
                                ))
                        {
                            None
                        } else {
                            self.run_target(job, target, &profile, options, cancel)
                        }
                    })
                })
                .collect();

            handles
                .into_iter()
                .map(|handle| handle.join().ok())
                .collect::<Option<Vec<_>>>()
                .map(|items| items.into_iter().flatten().collect())
        });

        let completed = match results {
            Some(results) => self.complete_job(job.id, Some(&results)),
            None => {
                self.raise_background_error(&CoreError::InvalidOperation("CLI 執行過程發生未預期的錯誤。".to_string()));
                self.complete_job(job.id, None)
            }
        };
        if let Err(error) = completed {
            self.raise_background_error(&error);
            return;
        }

        // 自動排程依設定間隔在尚未讀到倒數時讀取；手動執行後立即重讀並記住新倒數。
        if automatic {
            return;
        }
        if let Some(usage_reader) = &self.usage_reader {
            let delay = if self.clock.is_system() { Duration::from_secs(2) } else { Duration::from_millis(50) };
            if cancel.wait(delay) {
                return;
            }
            for &target in &targets {
                let Ok(value) = usage_reader.read(target, &settings.profile(target), &job.working_directory, &cancel)
                else {
                    return;
                };
                self.remember_reset(job.id, target, &value, &self.now());
            }
            self.raise_jobs_changed();
        }
    }

    /// 執行單一目標 CLI，並在開始／輸出／完成三個時機廣播即時活動事件，
    /// 供 UI 端的終端機面板即時顯示。事件廣播失敗不影響實際執行結果。
    fn run_target(
        &self,
        job: &ScheduledJob,
        target: CliKind,
        profile: &CliProfile,
        options: RunOptions,
        cancel: &CancelToken,
    ) -> Option<CliRunResult> {
        let activity = |kind: CliActivityKind, text: String| {
            self.raise_activity(&CliActivityEvent {
                cli: target,
                job_name: job.name.clone(),
                kind,
                text,
                timestamp: self.now(),
            });
        };
        debug_log::write("執行", format!("「{}」{} 開始執行，訊息：{}", job.name, target.short_name(), job.message));
        activity(CliActivityKind::Started, format!("開始執行，訊息：{}", job.message));

        let on_output = |is_error: bool, text: &str| {
            activity(if is_error { CliActivityKind::ErrorOutput } else { CliActivityKind::Output }, text.to_string());
        };
        // 活動事件可能由 UI 或其他監聽者處理一段時間，真正呼叫前再次確認。
        if cancel.is_canceled()
            || (options.automatic
                && !calculator::in_detection_window(initial_time_of(job), job.end_time_of_day, &self.now()))
        {
            debug_log::write("偵測", format!("{} 已離開偵測時段，不啟動 CLI", target.short_name()));
            return None;
        }
        let result = self.cli_runner.run(
            target,
            profile,
            &job.message,
            &job.working_directory,
            options.timeout,
            options.token_saver_mode,
            cancel,
            Some(&on_output),
        );

        debug_log::write(
            "執行",
            format!(
                "「{}」{} {}，結束碼 {}，錯誤：{}，日誌：{}",
                job.name,
                target.short_name(),
                if result.succeeded { "成功" } else { "失敗" },
                result.exit_code.map(|code| code.to_string()).unwrap_or_else(|| "無".to_string()),
                if result.error.is_empty() { "無" } else { &result.error },
                if result.log_path.is_empty() { "無" } else { &result.log_path }
            ),
        );
        if result.succeeded {
            activity(
                CliActivityKind::Completed,
                format!("執行完成，結束碼 {}", result.exit_code.map(|code| code.to_string()).unwrap_or_default()),
            );
        } else {
            activity(CliActivityKind::Failed, format!("執行失敗：{}", result.error));
        }
        Some(result)
    }

    /// 收尾一次執行：寫回結果並排到下一次讀取倒數（或下一次自動喚醒）的時點。
    /// `results` 為 None 代表執行過程發生錯誤。
    fn complete_job(&self, job_id: Guid, results: Option<&[CliRunResult]>) -> CoreResult<()> {
        let persistence = {
            let mut current = lock(&self.jobs);
            let mut jobs = current.clone();
            let Some(stored) = jobs.iter_mut().find(|item| item.id == job_id) else {
                return Ok(());
            };
            if results.is_none() && stored.status != ScheduleStatus::Running {
                return Ok(());
            }

            let now = self.now();
            stored.finished_at = Some(now);

            if let Some(results) = results {
                for result in results {
                    stored.last_results.retain(|existing| existing.cli != result.cli);
                    stored.last_results.push(result.clone());
                }
            }

            if stored.enabled {
                stored.scheduled_at = self.next_job_schedule_time(stored, &now)?;
                stored.status = ScheduleStatus::Pending;
            } else {
                stored.status = if results.is_some_and(|items| items.iter().all(|result| result.succeeded)) {
                    ScheduleStatus::Completed
                } else {
                    ScheduleStatus::Failed
                };
            }

            debug_log::write("執行", format!("完成 {}", describe_job(stored)));
            let persistence = self.job_store.save(&jobs);
            self.persistence_pending.store(persistence.is_err(), Ordering::SeqCst);
            // CLI 已實際完成，不能因磁碟故障回復成永遠 Running。
            *current = jobs;
            persistence
        };

        self.raise_jobs_changed();
        self.signal_scheduler();
        persistence
    }

    fn raise_jobs_changed(&self) {
        let listeners = lock(&self.jobs_changed).clone();
        for listener in listeners {
            listener(&());
        }
    }

    fn raise_background_error(&self, error: &CoreError) {
        debug_log::write("錯誤", format!("背景錯誤：{}", error.message()));
        let listeners = lock(&self.background_error).clone();
        for listener in listeners {
            listener(error);
        }
    }

    fn raise_activity(&self, activity: &CliActivityEvent) {
        let listeners = lock(&self.activity_logged).clone();
        for listener in listeners {
            listener(activity);
        }
    }
}

fn validate_job(job: &ScheduledJob) -> CoreResult<()> {
    calculator::validate_window(time::truncate_to_minutes(initial_time_of(job)), job.end_time_of_day)?;
    calculator::validate_interval(job.probe_interval_minutes)?;
    calculator::next_aligned_detection(
        time::truncate_to_minutes(initial_time_of(job)),
        job.end_time_of_day,
        &job.scheduled_at,
        job.probe_interval_minutes,
        true,
    )?;
    if job.initial_time_of_day.is_some_and(|start| !time::is_valid_time_of_day(start)) {
        return Err(CoreError::OutOfRange("開始時間必須介於 00:00 到 23:59。".to_string()));
    }
    if job.name.trim().is_empty() {
        return Err(CoreError::InvalidArgument("請輸入排程名稱。".to_string()));
    }
    if job.message.trim().is_empty() {
        return Err(CoreError::InvalidArgument("請輸入要傳送的簡短訊息。".to_string()));
    }
    if job.message.encode_utf16().count() > 50 {
        return Err(CoreError::InvalidArgument("為避免過量消耗 Token，喚醒訊息最多 50 個字元。".to_string()));
    }
    if job.targets.is_empty() {
        return Err(CoreError::InvalidArgument("至少選擇一個 CLI。".to_string()));
    }
    if job.working_directory.trim().is_empty() || !Path::new(&job.working_directory).is_dir() {
        return Err(CoreError::DirectoryNotFound("指定的工作目錄不存在。".to_string()));
    }
    Ok(())
}
