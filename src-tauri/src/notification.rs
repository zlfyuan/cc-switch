//! 本地通知模块
//!
//! 负责在以下场景下向用户发送 OS 级桌面通知：
//! - 配额使用率跨过 80% / 95% / 100% 阈值
//! - 配额即将重置（5 分钟前 + 重置瞬间）
//! - 自动故障转移切换供应商
//!
//! 设计要点：
//! - **去重状态持久化**：每个 (provider_id, tier_name, threshold) 组合每天最多推一次，
//!   状态写入 SQLite `settings` 表（key=`notification_dedup_state`），跨重启保留。
//! - **重置提醒调度器**：使用 tokio `time::sleep` 延迟触发，每个
//!   (provider_id, tier_name) 持有 `warn` + `reset` 两个 `AbortHandle`，
//!   注册时持久化到同一份 dedup state，进程重启时从 SQLite 重建未触发的任务。
//! - **AppHandle 通过 `OnceLock`** 注入（fire-and-forget 广播）。
//! - **NotificationState 通过 `app.manage(...)`** 持有可变 dedup 状态。
//! - **i18n**：通知文本通过 `crate::i18n::NotificationTexts` 按用户当前语言生成。
//!   即使窗口被隐藏也能正常发送本地化通知。

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::async_runtime::{self, AbortHandle};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;

use crate::database::Database;
use crate::i18n::{NotificationTexts, TextsLoader};
use crate::provider::UsageResult;
use crate::services::subscription::{QuotaTier, SubscriptionQuota};

// ============================================================================
// 常量
// ============================================================================

/// SQLite settings 表中保存 dedup state 的 key。
const DEDUP_STATE_KEY: &str = "notification_dedup_state";

/// 阈值告警的目标百分比（按升序触发）。
pub const THRESHOLDS: [u8; 3] = [80, 95, 100];

/// 重置提醒提前量（秒）。在 reset - N 秒时推送"即将重置"，整点再推"已重置"。
pub const RESET_WARN_LEAD_SECONDS: i64 = 5 * 60;

/// 同一 provider+tier 的"重置任务对"在 N 秒以内的变化视为抖动，跳过 reschedule。
/// 避免 5min 一次的 quota 拉取反复 abort + spawn。
pub const RESCHEDULE_MIN_DELTA_SECONDS: i64 = 60;

/// `resetsAt` 已经过去 24 小时以上 → 启动时直接清理（窗口已彻底失效）。
const MISSED_WINDOW_GRACE_HOURS: i64 = 24;

// ============================================================================
// 持久化的 dedup state
// ============================================================================

/// 单个 (provider_id, tier_name) 的重置提醒记录。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScheduledReset {
    pub warn_at_epoch: i64,
    pub reset_at_epoch: i64,
    pub warn_fired: bool,
    pub reset_fired: bool,
    /// 缓存显示用的供应商名（启动恢复时使用，避免渲染出空字符串）。
    #[serde(default)]
    pub provider_name: String,
    /// 缓存显示用的 tier 短名（如 "5h"、"7d"、"Opus 7d"）。
    #[serde(default)]
    pub tier_label: String,
}

/// 完整 dedup state —— 序列化到 SQLite `settings.notification_dedup_state`。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DedupState {
    /// key = "tier:<tier_name>:<threshold>"
    pub thresholds_fired: HashMap<String, String>,
    /// key = "<provider_id>:<tier_name>"
    pub resets_scheduled: HashMap<String, ScheduledReset>,
}

// ============================================================================
// 进程内状态
// ============================================================================

/// 全局 AppHandle 单例（fire-and-forget 广播）。
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// 通过 `app.manage(...)` 注入的可变状态。
pub struct NotificationState {
    /// 共享给 spawn 闭包，避免依赖 &NotificationState 的借用。
    pub db: Arc<Database>,
    pub dedup: Mutex<DedupState>,
    pub tasks: Mutex<HashMap<String, AbortHandle>>,
}

impl NotificationState {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            dedup: Mutex::new(DedupState::default()),
            tasks: Mutex::new(HashMap::new()),
        }
    }
}

// ============================================================================
// 启动 / 持久化辅助
// ============================================================================

/// 在 `setup()` 中调用一次，注入 AppHandle。重复调用是 no-op。
pub fn init(handle: AppHandle) {
    if APP_HANDLE.set(handle).is_err() {
        log::debug!("notification::init 重复调用，已忽略");
    }
}

async fn load_dedup(db: &Database) -> DedupState {
    match db.get_setting(DEDUP_STATE_KEY) {
        Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_else(|e| {
            log::warn!("解析 notification_dedup_state 失败，使用默认值: {e}");
            DedupState::default()
        }),
        Ok(None) => DedupState::default(),
        Err(e) => {
            log::warn!("读取 notification_dedup_state 失败: {e}");
            DedupState::default()
        }
    }
}

async fn save_dedup(db: &Database, state: &DedupState) {
    match serde_json::to_string(state) {
        Ok(json) => {
            if let Err(e) = db.set_setting(DEDUP_STATE_KEY, &json) {
                log::warn!("写入 notification_dedup_state 失败: {e}");
            }
        }
        Err(e) => log::warn!("序列化 notification_dedup_state 失败: {e}"),
    }
}

/// 启动时由 `init(handle)` 之后调用一次：
/// 1. 加载 + 清理过期条目
/// 2. 对剩余未触发的条目重建 tokio 任务
pub async fn bootstrap(state: tauri::State<'_, NotificationState>) {
    let fresh = load_dedup(&state.db).await;
    let now = Utc::now().timestamp();
    let grace_cutoff = now - MISSED_WINDOW_GRACE_HOURS * 3600;

    let mut dedup = state.dedup.lock().await;
    let before = dedup.resets_scheduled.len();
    dedup
        .resets_scheduled
        .retain(|_k, v| v.reset_at_epoch > grace_cutoff);
    let pruned = before - dedup.resets_scheduled.len();
    if pruned > 0 {
        log::info!("[notification] 清理过期重置提醒 {pruned} 条");
    }
    let to_reschedule: Vec<(String, ScheduledReset)> = dedup
        .resets_scheduled
        .iter()
        .filter(|(_k, sr)| !(sr.warn_fired && sr.reset_fired))
        .map(|(k, sr)| (k.clone(), sr.clone()))
        .collect();
    let snapshot = dedup.clone();
    drop(dedup);
    save_dedup(&state.db, &snapshot).await;
    let _ = fresh;

    let rescheduled = to_reschedule.len();
    for (key, sr) in to_reschedule {
        let provider_name = sr.provider_name.clone();
        let tier_label = sr.tier_label.clone();
        spawn_reset_pair(state.db.clone(), key, sr, &provider_name, &tier_label);
    }
    if rescheduled > 0 {
        log::info!("[notification] 重建 {rescheduled} 条重置提醒任务");
    }
}

/// `save_settings` 写盘后调用一次。当前为无操作，保留入口便于未来扩展。
pub async fn on_settings_changed(_state: tauri::State<'_, NotificationState>) {
    // 当前设计上是无操作；保留入口以便未来扩展（例如：设置关闭时 abort 所有待发任务）。
}

// ============================================================================
// 公开入口：订阅额度
// ============================================================================

/// 由 subscription / codex_oauth / coding_plan 命令在写入 usage cache 之后调用。
pub async fn check_and_notify_subscription(
    app: &AppHandle,
    provider_id: &str,
    provider_name: &str,
    quota: &SubscriptionQuota,
) {
    let Some(state) = app.try_state::<NotificationState>() else {
        return;
    };
    if !notifications_enabled() {
        return;
    }

    let texts = TextsLoader::resolve_for_current_language().snapshot();
    let settings = crate::settings::get_settings();
    let today = utc_date_string(Utc::now());

    if settings.notify_on_threshold_reached {
        for tier in &quota.tiers {
            let to_fire = thresholds_to_fire(&state, tier, &today).await;
            for pct in to_fire {
                let (title, body) = render_threshold(&texts, provider_name, &tier.name, pct);
                deliver(app, &title, &body);
                mark_threshold_fired(&state, provider_id, &tier.name, pct, today.clone()).await;
            }
        }
    }

    if settings.notify_on_reset_approaching {
        for tier in &quota.tiers {
            let Some(resets_at) = tier.resets_at.as_deref() else {
                continue;
            };
            let Some(reset_epoch) = parse_iso8601_to_epoch(resets_at) else {
                continue;
            };
            let now = Utc::now().timestamp();
            if reset_epoch <= now {
                continue;
            }
            let warn_epoch = reset_epoch - RESET_WARN_LEAD_SECONDS;
            schedule_reset_pair(
                &state,
                provider_id,
                &tier.name,
                warn_epoch,
                reset_epoch,
                provider_name,
            )
            .await;
        }
    }
}

/// 由 `get_balance` 调用。`success == false` 触发"已用尽"通知（无阈值语义）。
pub async fn check_and_notify_balance(
    app: &AppHandle,
    provider_id: &str,
    provider_name: &str,
    result: &UsageResult,
) {
    let Some(state) = app.try_state::<NotificationState>() else {
        return;
    };
    if !notifications_enabled() {
        return;
    }
    if result.success {
        return;
    }

    let key = format!("balance:{provider_id}:exhausted");
    let today = utc_date_string(Utc::now());

    let already = {
        let dedup = state.dedup.lock().await;
        dedup.thresholds_fired.get(&key).map(String::as_str) == Some(today.as_str())
    };
    if already {
        return;
    }

    let texts = TextsLoader::resolve_for_current_language().snapshot();
    let (title, body) = render_exhausted(&texts, provider_name);
    deliver(app, &title, &body);

    let mut dedup = state.dedup.lock().await;
    dedup.thresholds_fired.insert(key, today);
    let snapshot = dedup.clone();
    drop(dedup);
    save_dedup(&state.db, &snapshot).await;
}

// ============================================================================
// 公开入口：自动故障转移
// ============================================================================

/// 由 `proxy/failover_switch::do_switch` 在成功切换后调用（仅 `source == "quota"` 路径）。
pub fn notify_auto_switched(provider_name: &str, new_provider_name: &str) {
    let Some(handle) = APP_HANDLE.get() else {
        return;
    };
    if !notifications_enabled() {
        return;
    }
    let settings = crate::settings::get_settings();
    if !settings.notify_on_auto_switch {
        return;
    }
    let texts = TextsLoader::resolve_for_current_language().snapshot();
    let (title, body) = render_auto_switched(&texts, provider_name, new_provider_name);
    deliver(handle, &title, &body);
}

// ============================================================================
// 阈值判定
// ============================================================================

async fn thresholds_to_fire(
    state: &tauri::State<'_, NotificationState>,
    tier: &QuotaTier,
    today: &str,
) -> Vec<u8> {
    let util = tier.utilization;
    let dedup = state.dedup.lock().await;
    THRESHOLDS
        .iter()
        .copied()
        .filter(|pct| util >= *pct as f64)
        .filter(|pct| {
            let key = threshold_key(&tier.name, *pct);
            dedup.thresholds_fired.get(&key).map(String::as_str) != Some(today)
        })
        .collect()
}

pub fn threshold_key(tier_name: &str, threshold: u8) -> String {
    format!("tier:{tier_name}:{threshold}")
}

pub fn reset_key(provider_id: &str, tier_name: &str) -> String {
    format!("{provider_id}:{tier_name}")
}

fn threshold_dedup_key(provider_id: &str, tier_name: &str, threshold: u8) -> String {
    format!("{}:{}", provider_id, threshold_key(tier_name, threshold))
}

async fn mark_threshold_fired(
    state: &tauri::State<'_, NotificationState>,
    provider_id: &str,
    tier_name: &str,
    threshold: u8,
    today: String,
) {
    let key = threshold_dedup_key(provider_id, tier_name, threshold);
    let mut dedup = state.dedup.lock().await;
    dedup.thresholds_fired.insert(key, today);
    let snapshot = dedup.clone();
    drop(dedup);
    save_dedup(&state.db, &snapshot).await;
}

// ============================================================================
// 调度：重置提醒
// ============================================================================

/// 调度 (provider_id, tier_name) 的 warn + reset 两条提醒任务。
///
/// 若已存在相同 key 的任务且新时间差 < RESCHEDULE_MIN_DELTA_SECONDS 且 reset_at_epoch 不变，
/// 视为抖动，跳过。否则 abort 旧任务再 spawn 新的。
async fn schedule_reset_pair(
    state: &tauri::State<'_, NotificationState>,
    provider_id: &str,
    tier_name: &str,
    warn_epoch: i64,
    reset_epoch: i64,
    provider_name: &str,
) {
    let key = reset_key(provider_id, tier_name);

    let mut dedup = state.dedup.lock().await;
    let mut tasks = state.tasks.lock().await;

    if let Some(existing) = dedup.resets_scheduled.get(&key) {
        let delta = (existing.warn_at_epoch - warn_epoch).abs();
        if delta < RESCHEDULE_MIN_DELTA_SECONDS && existing.reset_at_epoch == reset_epoch {
            return;
        }
        if let Some(h) = tasks.remove(&format!("{key}:warn")) {
            h.abort();
        }
        if let Some(h) = tasks.remove(&format!("{key}:reset")) {
            h.abort();
        }
    }

    let sr = ScheduledReset {
        warn_at_epoch,
        reset_at_epoch,
        warn_fired: false,
        reset_fired: false,
        provider_name: provider_name.to_string(),
        tier_label: tier_name.to_string(),
    };
    dedup.resets_scheduled.insert(key.clone(), sr.clone());
    let snapshot = dedup.clone();
    drop(dedup);
    save_dedup(&state.db, &snapshot).await;

    let (warn_abort, reset_abort) =
        spawn_reset_pair(state.db.clone(), key.clone(), sr, provider_name, tier_name);
    tasks.insert(format!("{key}:warn"), warn_abort);
    tasks.insert(format!("{key}:reset"), reset_abort);
}

/// spawn warn + reset 一对 tokio 任务，返回两者的 AbortHandle。
fn spawn_reset_pair(
    db: Arc<Database>,
    key: String,
    sr: ScheduledReset,
    provider_name: &str,
    tier_name: &str,
) -> (AbortHandle, AbortHandle) {
    let now = Utc::now().timestamp();
    let warn_delay = (sr.warn_at_epoch - now).max(0) as u64;
    let reset_delay = (sr.reset_at_epoch - now).max(0) as u64;

    let texts = TextsLoader::resolve_for_current_language().snapshot();

    let key_w = key.clone();
    let tier_w = tier_name.to_string();
    let name_w = provider_name.to_string();
    let texts_w = texts.clone();
    let db_w = db.clone();
    let sr_w = sr.clone();
    let warn_abort = async_runtime::spawn(async move {
        if !sr_w.warn_fired && warn_delay > 0 {
            tokio_sleep_secs(warn_delay).await;
            fire_warn(&db_w, &key_w, &tier_w, &name_w, &texts_w).await;
        }
    });

    let key_r = key.clone();
    let tier_r = tier_name.to_string();
    let name_r = provider_name.to_string();
    let texts_r = texts;
    let db_r = db;
    let sr_r = sr;
    let reset_abort = async_runtime::spawn(async move {
        if !sr_r.reset_fired && reset_delay > 0 {
            tokio_sleep_secs(reset_delay).await;
            fire_reset(&db_r, &key_r, &tier_r, &name_r, &texts_r).await;
        }
    });

    (warn_abort, reset_abort)
}

async fn tokio_sleep_secs(secs: u64) {
    async_runtime::TokioHandle::current()
        .sleep(Duration::from_secs(secs))
        .await;
}

async fn fire_warn(
    db: &Arc<Database>,
    key: &str,
    tier_name: &str,
    provider_name: &str,
    texts: &NotificationTexts,
) {
    let Some(handle) = APP_HANDLE.get() else {
        return;
    };
    if !notifications_enabled() {
        return;
    }

    {
        let mut dedup_map = db
            .get_setting(DEDUP_STATE_KEY)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<DedupState>(&s).ok())
            .unwrap_or_default();
        if let Some(sr) = dedup_map.resets_scheduled.get_mut(key) {
            if sr.warn_fired {
                return;
            }
            sr.warn_fired = true;
        }
        save_dedup(db, &dedup_map).await;
    }

    let (title, body) = render_reset_approaching(texts, provider_name, tier_name);
    deliver(&handle, &title, &body);
}

async fn fire_reset(
    db: &Arc<Database>,
    key: &str,
    tier_name: &str,
    provider_name: &str,
    texts: &NotificationTexts,
) {
    let Some(handle) = APP_HANDLE.get() else {
        return;
    };
    if !notifications_enabled() {
        return;
    }

    {
        let mut dedup_map = db
            .get_setting(DEDUP_STATE_KEY)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<DedupState>(&s).ok())
            .unwrap_or_default();
        if let Some(sr) = dedup_map.resets_scheduled.get_mut(key) {
            if sr.reset_fired {
                return;
            }
            sr.reset_fired = true;
        }
        // 重置已经发生，清理该 key 的 dedup 条目，避免长时间累积。
        dedup_map.resets_scheduled.remove(key);
        save_dedup(db, &dedup_map).await;
    }

    let (title, body) = render_reset(texts, provider_name, tier_name);
    deliver(&handle, &title, &body);
}

// ============================================================================
// 通知投递
// ============================================================================

fn deliver(app: &AppHandle, title: &str, body: &str) {
    match app.notification().builder().title(title).body(body).show() {
        Ok(_) => log::debug!("[notification] sent: {title}"),
        Err(e) => log::warn!("[notification] send failed: {e}"),
    }
}

fn notifications_enabled() -> bool {
    crate::settings::get_settings().enable_notifications
}

// ============================================================================
// 文本渲染
// ============================================================================

fn render_threshold(
    t: &NotificationTexts,
    provider_name: &str,
    tier_name: &str,
    pct: u8,
) -> (String, String) {
    let title = t.threshold_title.replace("{{provider}}", provider_name);
    let body = t
        .threshold_body
        .replace("{{provider}}", provider_name)
        .replace("{{tier}}", tier_name)
        .replace("{{percent}}", &pct.to_string());
    (title, body)
}

fn render_exhausted(t: &NotificationTexts, provider_name: &str) -> (String, String) {
    let title = t.exhausted_title.replace("{{provider}}", provider_name);
    let body = t.exhausted_body.replace("{{provider}}", provider_name);
    (title, body)
}

fn render_auto_switched(t: &NotificationTexts, old: &str, new: &str) -> (String, String) {
    let title = t.auto_switched_title.to_string();
    let body = t
        .auto_switched_body
        .replace("{{from}}", old)
        .replace("{{to}}", new);
    (title, body)
}

fn render_reset_approaching(
    t: &NotificationTexts,
    provider_name: &str,
    tier_name: &str,
) -> (String, String) {
    let title = t
        .reset_approaching_title
        .replace("{{provider}}", provider_name);
    let body = t
        .reset_approaching_body
        .replace("{{provider}}", provider_name)
        .replace("{{tier}}", tier_name);
    (title, body)
}

fn render_reset(t: &NotificationTexts, provider_name: &str, tier_name: &str) -> (String, String) {
    let title = t.reset_title.replace("{{provider}}", provider_name);
    let body = t
        .reset_body
        .replace("{{provider}}", provider_name)
        .replace("{{tier}}", tier_name);
    (title, body)
}

// ============================================================================
// 工具函数
// ============================================================================

pub fn parse_iso8601_to_epoch(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.timestamp())
}

pub fn utc_date_string(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%d").to_string()
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iso8601_handles_z_suffix() {
        assert_eq!(
            parse_iso8601_to_epoch("2026-06-22T12:00:00Z"),
            Some(1771243200)
        );
        assert_eq!(
            parse_iso8601_to_epoch("2026-06-22T12:00:00+00:00"),
            Some(1771243200)
        );
        assert!(parse_iso8601_to_epoch("not a date").is_none());
    }

    #[test]
    fn utc_date_string_is_stable() {
        let s = utc_date_string(Utc::now());
        assert_eq!(s.len(), 10);
        assert_eq!(&s[4..5], "-");
    }

    #[test]
    fn threshold_key_uses_tier_and_pct() {
        assert_eq!(threshold_key("five_hour", 80), "tier:five_hour:80");
        assert_eq!(threshold_key("seven_day", 100), "tier:seven_day:100");
    }

    #[test]
    fn reset_key_uses_provider_and_tier() {
        assert_eq!(
            reset_key("provider-123", "five_hour"),
            "provider-123:five_hour"
        );
    }

    #[test]
    fn threshold_constants_ascending() {
        let mut prev = 0u8;
        for t in THRESHOLDS {
            assert!(t > prev);
            prev = t;
        }
    }

    #[test]
    fn render_threshold_substitutes_placeholders() {
        let t = NotificationTexts {
            threshold_title: "Quota {{provider}}",
            threshold_body: "{{provider}} {{tier}} used {{percent}}%",
            reset_approaching_title: "",
            reset_approaching_body: "",
            reset_title: "",
            reset_body: "",
            auto_switched_title: "",
            auto_switched_body: "",
            exhausted_title: "",
            exhausted_body: "",
        };
        let (title, body) = render_threshold(&t, "acme", "five_hour", 80);
        assert_eq!(title, "Quota acme");
        assert_eq!(body, "acme five_hour used 80%");
    }

    #[test]
    fn render_auto_switched_substitutes_from_and_to() {
        let t = NotificationTexts {
            threshold_title: "",
            threshold_body: "",
            reset_approaching_title: "",
            reset_approaching_body: "",
            reset_title: "",
            reset_body: "",
            auto_switched_title: "Switched",
            auto_switched_body: "from {{from}} to {{to}}",
            exhausted_title: "",
            exhausted_body: "",
        };
        let (title, body) = render_auto_switched(&t, "A", "B");
        assert_eq!(title, "Switched");
        assert_eq!(body, "from A to B");
    }

    #[test]
    fn dedup_state_roundtrip() {
        let mut s = DedupState::default();
        s.thresholds_fired
            .insert("tier:five_hour:80".to_string(), "2026-06-22".to_string());
        s.resets_scheduled.insert(
            "pid:five_hour".to_string(),
            ScheduledReset {
                warn_at_epoch: 100,
                reset_at_epoch: 200,
                warn_fired: false,
                reset_fired: true,
            },
        );
        let json = serde_json::to_string(&s).unwrap();
        let back: DedupState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.thresholds_fired.len(), 1);
        let sr = back.resets_scheduled.get("pid:five_hour").unwrap();
        assert_eq!(sr.warn_at_epoch, 100);
        assert!(sr.reset_fired);
    }

    #[test]
    fn reschedule_min_delta_threshold() {
        assert!(RESCHEDULE_MIN_DELTA_SECONDS >= 10);
        assert!(RESCHEDULE_MIN_DELTA_SECONDS <= 600);
    }

    #[test]
    fn reset_warn_lead_is_5_minutes() {
        assert_eq!(RESET_WARN_LEAD_SECONDS, 300);
    }
}