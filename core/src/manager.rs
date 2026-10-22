//! 管理 AI 喚醒排程的初始化、掃描、手動觸發與背景執行。
//!
//! 只依賴 `DataStore`、`CliRunner`、`UsageReader` 與 `Clock`，
//! 不知道資料存在哪裡、CLI 怎麼啟動，也不知道現在幾點。

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use chrono::NaiveTime;

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
use crate::usage::{exhausted_until, is_counting_down, CliUsageAvailability, UsageReader};

/// 閒置時最長的等待間隔。
/// 排程器不每秒輪詢，而是直接睡到下一個到期時間；這個上限只是為了
/// 在系統休眠、時鐘調整或時區變更後仍能在可接受的時間內重新校準。
const MAX_IDLE_WAIT_SECONDS: i64 = 30;

/// 最短等待間隔，避免時間已到但狀態尚未落地時空轉。
const MIN_WAIT_MILLISECONDS: i64 = 200;

const QUOTA_WAKEUP_COOLDOWN_MINUTES: i64 = 2;

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

fn local_date(moment: &Moment) -> chrono::NaiveDate {
    time::local_naive(moment).date()
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
        "「{}」{} 開始 {start} 目標 [{}] 狀態 {}{} 下次 {}",
        job.name,
        job.recurrence.display_name(),
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
    last_quota_wakeup_by_cli: Mutex<HashMap<CliKind, Moment>>,
    last_quota_probe_at: Mutex<Option<Moment>>,

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
                last_quota_wakeup_by_cli: Mutex::new(HashMap::new()),
                last_quota_probe_at: Mutex::new(None),
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
            // 只保留自動模式。舊版本的單次、每日、每週與自動倒數模式一律遷移為自動模式，
            // 開始時間沿用原本的開始時間（每日等模式則取原排程的時分）。
            let initial_time = initial_time_of(job);
            job.recurrence = ScheduleRecurrence::Interval;
            job.initial_time_of_day = Some(initial_time);
            if !time::is_valid_time_of_day(initial_time) {
                return Err(CoreError::InvalidData("開始時間必須介於 00:00 到 23:59。".to_string()));
            }
            job.schedule_revision = SCHEDULE_REVISION;

            // 停用與終止狀態保留歷史時間；未到期的排程也不因重啟而提前。
            if job.enabled
                && matches!(job.status, ScheduleStatus::Pending | ScheduleStatus::Running)
                && job.scheduled_at <= now
            {
                job.scheduled_at = match job.finished_at {
                    Some(finished) if local_date(&finished) < local_date(&now) => {
                        // 上次執行在今日以前（昨天或更早）
                        // 今日的首次喚醒基準時間
                        let today_initial = calculator::get_previous_daily_occurrence(initial_time, &now)?;
                        if finished >= today_initial {
                            calculator::get_next_daily_occurrence(initial_time, &now)?
                        } else {
                            today_initial
                        }
                    }
                    // 上次執行在今天之內
                    Some(finished) => calculator::get_next_auto_interval_occurrence(initial_time, &finished)?,
                    // 從未執行過：設定為今日首次喚醒
                    None => calculator::get_previous_daily_occurrence(initial_time, &now)?,
                };
            }

            if job.status == ScheduleStatus::Running {
                // 上次是被強制關閉的，不可能還在跑。
                job.status = if job.enabled { ScheduleStatus::Pending } else { ScheduleStatus::Disabled };
                job.started_at = None;
            }
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
            let mut jobs = lock(&inner.jobs);
            let now = inner.now();
            let mut saved = job.snapshot();
            // 核心層只接受自動模式。
            let initial_time = initial_time_of(&saved);
            saved.recurrence = ScheduleRecurrence::Interval;
            saved.schedule_revision = SCHEDULE_REVISION;
            saved.initial_time_of_day = Some(initial_time);
            if saved.scheduled_at <= now {
                saved.scheduled_at = calculator::get_next_daily_occurrence(initial_time, &now)?;
            }
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
        }

        inner.raise_jobs_changed();
        inner.signal_scheduler();
        Ok(())
    }

    pub fn delete(&self, id: Guid) -> CoreResult<()> {
        let inner = &self.inner;
        inner.ensure_not_disposed()?;
        {
            let mut jobs = lock(&inner.jobs);
            if jobs.iter().any(|job| job.id == id && job.status == ScheduleStatus::Running) {
                return Err(CoreError::InvalidOperation("排程正在執行，暫時不能刪除。".to_string()));
            }
            if let Some(job) = jobs.iter().find(|job| job.id == id) {
                debug_log::write("排程", format!("刪除「{}」", job.name));
            }
            jobs.retain(|job| job.id != id);
            inner.job_store.save(&jobs)?;
        }

        inner.raise_jobs_changed();
        inner.signal_scheduler();
        Ok(())
    }

    pub fn run_now(&self, id: Guid) -> CoreResult<()> {
        let inner = &self.inner;
        inner.ensure_not_disposed()?;
        let execution = {
            let mut jobs = lock(&inner.jobs);
            let job = jobs
                .iter_mut()
                .find(|item| item.id == id)
                .ok_or_else(|| CoreError::InvalidOperation("找不到指定排程。".to_string()))?;
            if job.status == ScheduleStatus::Running {
                return Err(CoreError::InvalidOperation("排程已經在執行中。".to_string()));
            }

            job.enabled = true;
            job.status = ScheduleStatus::Running;
            job.started_at = Some(inner.now());
            job.finished_at = None;
            job.last_results = Vec::new();
            let execution = job.snapshot();
            inner.job_store.save(&jobs)?;
            execution
        };

        let now = inner.now();
        {
            let mut last_wakeup = lock(&inner.last_quota_wakeup_by_cli);
            for target in &execution.targets {
                last_wakeup.insert(*target, now);
            }
        }

        debug_log::write(
            "執行",
            format!("手動立即執行「{}」目標 [{}]", execution.name, kinds_text(&execution.targets)),
        );
        inner.raise_jobs_changed();
        let targets = execution.targets.clone();
        inner.start_execution(execution, targets, CancelToken::none());
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
            match self.scan_and_start_due_jobs() {
                Ok(next_delay) => self.wait_for_next_round(next_delay),
                Err(CoreError::Canceled) if self.stop.is_canceled() => break,
                Err(error) => {
                    self.raise_background_error(&error);
                    break;
                }
            }
        }
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
        for target in targets {
            last_wakeup.insert(*target, now);
        }
    }

    fn in_wakeup_cooldown(&self, kind: CliKind, now: &Moment) -> Option<Moment> {
        lock(&self.last_quota_wakeup_by_cli).get(&kind).copied().filter(|last| *now - *last < quota_wakeup_cooldown())
    }

    /// 掃描並啟動到期排程，回傳距離下一個到期時間還有多久。
    fn scan_and_start_due_jobs(self: &Arc<Self>) -> CoreResult<Span> {
        let mut due_jobs: Vec<(ScheduledJob, Vec<CliKind>)> = Vec::new();
        let mut interval_job_to_probe: Option<ScheduledJob> = None;
        let mut targets_to_probe: Vec<CliKind> = Vec::new();
        let mut settings_snapshot: Option<AppSettings> = None;
        let mut quota_managed_job_id: Option<Guid> = None;
        let probe_interval = calculator::auto_probe_interval();
        let mut next_delay;

        {
            let mut jobs = lock(&self.jobs);
            let now = self.now();
            if self.usage_reader.is_some() {
                settings_snapshot = Some((self.settings_provider)().normalized());
            }

            let mut last_quota_probe_at = lock(&self.last_quota_probe_at);

            let mut overdue: Vec<usize> = Vec::new();
            for (index, job) in jobs.iter().enumerate() {
                if !job.enabled || job.status != ScheduleStatus::Pending {
                    continue;
                }
                if let (Some(usage_reader), None) = (&self.usage_reader, quota_managed_job_id) {
                    let local_now = time::local_naive(&now);
                    let today_initial =
                        local_now.date().and_time(NaiveTime::MIN) + time::truncate_to_minutes(initial_time_of(job));
                    if local_now >= today_initial {
                        quota_managed_job_id = Some(job.id);

                        // 開始時間之後每 10 分鐘讀取一次所有目標 CLI 的倒數狀態；
                        // 當日第一次進入（含剛到開始時間、開機補做）立即讀取。
                        let today_initial = time::from_local_naive(today_initial);
                        let probe_all =
                            last_quota_probe_at.is_none_or(|last| last < today_initial || now - last >= probe_interval);

                        let mut due_targets = Vec::new();
                        for &target in &job.targets {
                            if due_targets.contains(&target) {
                                continue;
                            }
                            // 上次讀到的倒數已經結束時，不必等到下一個 10 分鐘。
                            let countdown_ended = usage_reader.latest_snapshot(target).is_some_and(|snapshot| {
                                snapshot
                                    .short_window_or_first()
                                    .and_then(|window| window.resets_at)
                                    .is_some_and(|reset| snapshot.observed_at < reset && reset <= now)
                            });
                            if probe_all || countdown_ended {
                                due_targets.push(target);
                            }
                        }

                        if probe_all {
                            *last_quota_probe_at = Some(now);
                        }
                        if !due_targets.is_empty() {
                            debug_log::write(
                                "自動模式",
                                format!(
                                    "「{}」讀取倒數 [{}]（{}）",
                                    job.name,
                                    kinds_text(&due_targets),
                                    if probe_all { "每 10 分鐘例行讀取" } else { "上次倒數已結束" }
                                ),
                            );
                            interval_job_to_probe = Some(job.snapshot());
                            targets_to_probe = due_targets;
                        }
                        continue;
                    }
                }

                if job.scheduled_at <= now {
                    overdue.push(index);
                }
            }

            if !overdue.is_empty() {
                // 電腦睡眠或程式關閉一段時間後，可能同時有多個排程逾時。
                // 只補做時間離現在最近的那一個，其餘直接推到下一次每日時點，
                // 避免同一輪一次觸發多個 CLI 呼叫。
                let mut primary = overdue[0];
                for &index in &overdue[1..] {
                    if jobs[index].scheduled_at > jobs[primary].scheduled_at {
                        primary = index;
                    }
                }

                for &index in &overdue {
                    let job = &mut jobs[index];
                    if index == primary {
                        debug_log::write("執行", format!("到期執行 {}", describe_job(job)));
                        due_jobs.push((job.snapshot(), job.targets.clone()));
                        job.status = ScheduleStatus::Running;
                        job.started_at = Some(now);
                    } else {
                        job.finished_at = Some(now);
                        job.scheduled_at = calculator::get_next_auto_interval_occurrence(initial_time_of(job), &now)?;
                        debug_log::write("執行", format!("同時逾時，略過補做並改排 {}", describe_job(job)));
                    }
                }

                self.job_store.save(&jobs)?;
            }

            next_delay = self.calculate_next_delay(&jobs, &now, quota_managed_job_id);
            if let (Some(_), None, Some(last)) = (quota_managed_job_id, &interval_job_to_probe, *last_quota_probe_at) {
                let until_next_quota_probe = (last + probe_interval) - now;
                if until_next_quota_probe < next_delay {
                    next_delay = until_next_quota_probe;
                }
            }
        }

        if !due_jobs.is_empty() {
            self.raise_jobs_changed();
            for (job, targets) in due_jobs {
                self.mark_wakeup(&targets, self.now());
                self.start_execution(job, targets, self.stop.clone());
            }
            return Ok(next_delay);
        }

        // 自動模式：在當日開始時間之後讀取目標 CLI 的倒數狀態，未倒數者立即呼叫
        let (Some(probe_job), Some(usage_reader), Some(settings)) =
            (interval_job_to_probe, &self.usage_reader, settings_snapshot)
        else {
            return Ok(next_delay);
        };
        if targets_to_probe.is_empty() {
            return Ok(next_delay);
        }

        let now = self.now();
        let mut non_counting_down = Vec::new();
        for kind in targets_to_probe {
            if let Some(last) = self.in_wakeup_cooldown(kind, &now) {
                debug_log::write(
                    "自動模式",
                    format!("{} 於 {} 剛呼叫過，冷卻中不重讀", kind.short_name(), local_text(&last)),
                );
                continue;
            }

            match usage_reader.read(kind, &settings.profile(kind), &probe_job.working_directory, &self.stop) {
                Ok(snapshot) => {
                    if snapshot.availability != CliUsageAvailability::Available {
                        debug_log::write("自動模式", format!("{} 額度無法讀取，不呼叫", kind.short_name()));
                    } else if let Some(reset) = exhausted_until(Some(&snapshot), &now) {
                        // 例如每週額度已用完：呼叫只會回 HTTP 429，等到重置後再說。
                        debug_log::write(
                            "自動模式",
                            format!("{} 額度已用盡，至 {} 重置前不呼叫", kind.short_name(), local_text(&reset)),
                        );
                    } else if is_counting_down(Some(&snapshot), &now) {
                        debug_log::write("自動模式", format!("{} 倒數中，不呼叫", kind.short_name()));
                    } else {
                        non_counting_down.push(kind);
                    }
                }
                Err(CoreError::Canceled) if self.stop.is_canceled() => return Err(CoreError::Canceled),
                // 忽略個別 CLI 額度探測異常
                Err(_) => {}
            }
        }

        if !non_counting_down.is_empty() {
            let job_to_start = {
                let mut jobs = lock(&self.jobs);
                match jobs
                    .iter_mut()
                    .find(|job| job.id == probe_job.id && job.enabled && job.status == ScheduleStatus::Pending)
                {
                    Some(stored) => {
                        stored.status = ScheduleStatus::Running;
                        stored.started_at = Some(now);
                        let snapshot = stored.snapshot();
                        self.job_store.save(&jobs)?;
                        Some(snapshot)
                    }
                    None => None,
                }
            };

            if let Some(job) = job_to_start {
                debug_log::write(
                    "自動模式",
                    format!("「{}」沒有倒數 [{}]，立即呼叫", job.name, kinds_text(&non_counting_down)),
                );
                self.mark_wakeup(&non_counting_down, now);
                self.raise_jobs_changed();
                self.start_execution(job, non_counting_down, self.stop.clone());
            }
        } else {
            // 全部都在倒數：排程時間改為下一次讀取倒數的時點（跨過 24:00 則為隔天的開始時間）。
            let probed_at = lock(&self.last_quota_probe_at).unwrap_or(now);
            let changed = {
                let mut jobs = lock(&self.jobs);
                match jobs
                    .iter_mut()
                    .find(|job| job.id == probe_job.id && job.enabled && job.status == ScheduleStatus::Pending)
                {
                    Some(stored) => {
                        let next = calculator::get_next_auto_probe_occurrence(initial_time_of(stored), &probed_at)?;
                        let changed = stored.scheduled_at != next;
                        debug_log::write(
                            "自動模式",
                            format!("「{}」沒有需要呼叫的 CLI，下次讀取 {}", stored.name, local_text(&next)),
                        );
                        if changed {
                            stored.scheduled_at = next;
                            self.job_store.save(&jobs)?;
                        }
                        changed
                    }
                    None => false,
                }
            };
            if changed {
                self.raise_jobs_changed();
            }
        }

        Ok(next_delay)
    }

    /// 計算距離最近一個待執行排程的等待時間。呼叫端必須已持有排程清單的鎖。
    fn calculate_next_delay(&self, jobs: &[ScheduledJob], now: &Moment, ignored_job_id: Option<Guid>) -> Span {
        let mut shortest = max_idle_wait();
        for job in jobs {
            if !job.enabled || job.status != ScheduleStatus::Pending {
                continue;
            }
            if ignored_job_id == Some(job.id) {
                continue;
            }

            shortest = shortest.min(job.scheduled_at - *now);

            // 自動模式：若目標 CLI 正在倒數，將倒數結束時刻納入喚醒時間
            if let Some(usage_reader) = &self.usage_reader {
                for &target in &job.targets {
                    let snapshot = usage_reader.latest_snapshot(target);
                    if !is_counting_down(snapshot.as_ref(), now) {
                        continue;
                    }
                    let resets_at = snapshot
                        .as_ref()
                        .and_then(|value| value.short_window_or_first())
                        .and_then(|window| window.resets_at);
                    if let Some(resets_at) = resets_at {
                        let until_reset = resets_at - *now;
                        if until_reset > Span::zero() && until_reset < shortest {
                            shortest = until_reset;
                        }
                    }
                }
            }
        }
        shortest
    }

    fn start_execution(self: &Arc<Self>, job: ScheduledJob, targets: Vec<CliKind>, cancel: CancelToken) {
        let inner = Arc::clone(self);
        let handle = std::thread::spawn(move || inner.execute_job(job, targets, cancel));
        let mut threads = lock(&self.threads);
        threads.retain(|existing| !existing.is_finished());
        threads.push(handle);
    }

    fn execute_job(&self, job: ScheduledJob, targets: Vec<CliKind>, cancel: CancelToken) {
        let settings = (self.settings_provider)().normalized();
        let timeout = Duration::from_secs(settings.execution_timeout_minutes.max(1) as u64 * 60);

        // 執行前：若支援用量讀取，且最新快照非近期取得，先讀取目標 CLI 的最新流量與倒數狀態
        if let Some(usage_reader) = &self.usage_reader {
            for &target in &targets {
                let stale = usage_reader
                    .latest_snapshot(target)
                    .is_none_or(|snapshot| self.now() - snapshot.observed_at > Span::seconds(10));
                if stale {
                    let _ = usage_reader.read(target, &settings.profile(target), &job.working_directory, &cancel);
                }
            }
        }

        // 每個選定的 CLI 都在自己的執行緒獨立啟動，不會變成循序執行。
        let results: Option<Vec<CliRunResult>> = std::thread::scope(|scope| {
            let handles: Vec<_> = targets
                .iter()
                .map(|&target| {
                    let profile = settings.profile(target);
                    let token_saver_mode = settings.token_saver_mode;
                    let (job, cancel) = (&job, &cancel);
                    scope.spawn(move || self.run_target(job, target, &profile, timeout, token_saver_mode, cancel))
                })
                .collect();

            handles.into_iter().map(|handle| handle.join().ok()).collect()
        });

        let completed = match results {
            Some(results) => self.complete_job(job.id, Some(&results)),
            None => Err(CoreError::InvalidOperation("CLI 執行過程發生未預期的錯誤。".to_string())),
        };
        if let Err(error) = completed {
            // 忽略次要的清理錯誤
            let _ = self.complete_job(job.id, None);
            self.raise_background_error(&error);
            return;
        }

        // 執行後：稍待讓 API / Language Server 扣抵額度並開啟新一輪倒數，自動捕捉最新倒數
        if let Some(usage_reader) = &self.usage_reader {
            let delay = if self.clock.is_system() { Duration::from_secs(2) } else { Duration::from_millis(50) };
            if cancel.wait(delay) {
                return;
            }
            for &target in &targets {
                if usage_reader.read(target, &settings.profile(target), &job.working_directory, &cancel).is_err() {
                    return;
                }
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
        timeout: Duration,
        token_saver_mode: bool,
        cancel: &CancelToken,
    ) -> CliRunResult {
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
        let result = self.cli_runner.run(
            target,
            profile,
            &job.message,
            &job.working_directory,
            timeout,
            token_saver_mode,
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
        result
    }

    /// 收尾一次執行：寫回結果並排到下一次讀取倒數（或下一次自動喚醒）的時點。
    /// `results` 為 None 代表執行過程發生錯誤。
    fn complete_job(&self, job_id: Guid, results: Option<&[CliRunResult]>) -> CoreResult<()> {
        {
            let mut jobs = lock(&self.jobs);
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
                // 能讀取額度時 10 分鐘後再讀取倒數；否則維持每 5 小時 1 分鐘。
                stored.scheduled_at = if self.usage_reader.is_some() {
                    calculator::get_next_auto_probe_occurrence(initial_time_of(stored), &now)?
                } else {
                    calculator::get_next_auto_interval_occurrence(initial_time_of(stored), &now)?
                };
                stored.status = ScheduleStatus::Pending;
            } else {
                stored.status = if results.is_some_and(|items| items.iter().all(|result| result.succeeded)) {
                    ScheduleStatus::Completed
                } else {
                    ScheduleStatus::Failed
                };
            }

            debug_log::write("執行", format!("完成 {}", describe_job(stored)));
            self.job_store.save(&jobs)?;
        }

        self.raise_jobs_changed();
        self.signal_scheduler();
        Ok(())
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
