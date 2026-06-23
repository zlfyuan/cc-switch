use crate::app_config::AppType;
use crate::provider::UsageResult;
use crate::store::AppState;
use tauri::{Emitter, State};

/// 查询余额型 provider（DeepSeek/SiliconFlow/StepFun/OpenRouter/Novita）的余额。
///
/// `provider_id` / `provider_name` 由前端从当前 provider 上下文传入，用于通知
/// dedup key 与显示文本；不传则使用 baseUrl 短哈希。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_balance(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    base_url: String,
    api_key: String,
    provider_id: Option<String>,
    provider_name: Option<String>,
) -> Result<UsageResult, String> {
    let result = crate::services::balance::get_balance(&base_url, &api_key).await?;

    let pid = provider_id.unwrap_or_else(|| format!("balance:{}", short_hash(&base_url)));
    let pname = provider_name.unwrap_or_else(|| pid.clone());

    // 与 subscription 路径对齐：写 usage_cache + emit `usage-cache-updated` + 通知调度
    let payload = serde_json::json!({
        "kind": "script",
        "appType": AppType::Codex.as_str(),
        "providerId": &pid,
        "data": &result,
    });
    if let Err(e) = app.emit("usage-cache-updated", payload) {
        log::error!("emit usage-cache-updated (balance) 失败: {e}");
    }
    state.usage_cache.put_script(AppType::Codex, pid.clone(), result.clone());
    crate::tray::schedule_tray_refresh(&app);

    // 余额语义：仅在 success=false 时触发"已用尽"通知
    crate::notification::check_and_notify_balance(&app, &pid, &pname, &result).await;

    Ok(result)
}

fn short_hash(s: &str) -> String {
    let mut h: u64 = 1469598103934665603;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{:x}", h & 0xffff_ffff)
}