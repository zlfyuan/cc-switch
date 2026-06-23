use crate::app_config::AppType;
use crate::services::subscription::SubscriptionQuota;
use crate::store::AppState;
use tauri::{Emitter, State};

/// 查询 Coding Plan 供应商（Kimi/Zhipu/MiniMax/ZenMux/Volcengine 等）的订阅额度。
///
/// `provider_id` / `provider_name` 由前端从当前 provider 上下文传入，用于通知 dedup key
/// 与显示文本；不传则使用 baseUrl hash 作为 fallback id。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_coding_plan_quota(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    base_url: String,
    api_key: String,
    // 火山方舟用控制面 AK/SK 签名查询用量；其他供应商不传，沿用 api_key。
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    provider_id: Option<String>,
    provider_name: Option<String>,
) -> Result<SubscriptionQuota, String> {
    let quota = crate::services::coding_plan::get_coding_plan_quota(
        &base_url,
        &api_key,
        access_key_id.as_deref(),
        secret_access_key.as_deref(),
    )
    .await?;

    let pid = provider_id.unwrap_or_else(|| format!("coding_plan:{}", short_hash(&base_url)));
    let pname = provider_name.unwrap_or_else(|| pid.clone());

    let payload = serde_json::json!({
        "kind": "subscription",
        "appType": AppType::Codex.as_str(),
        "data": &quota,
    });
    if let Err(e) = app.emit("usage-cache-updated", payload) {
        log::error!("emit usage-cache-updated (coding_plan) 失败: {e}");
    }
    state.usage_cache.put_subscription(AppType::Codex, quota.clone());
    crate::tray::schedule_tray_refresh(&app);

    if quota.success {
        crate::notification::check_and_notify_subscription(&app, &pid, &pname, &quota).await;
    }

    Ok(quota)
}

/// baseUrl 短哈希，作为无 provider_id 时的 fallback dedup key。
fn short_hash(s: &str) -> String {
    let mut h: u64 = 1469598103934665603;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{:x}", h & 0xffff_ffff)
}