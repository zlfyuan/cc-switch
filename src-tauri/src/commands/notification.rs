//! 通知相关 Tauri 命令
//!
//! - `request_notification_permission`：弹出 OS 权限对话框
//! - `is_notification_permission_granted`：检查权限状态
//! - `test_notification`：发一条测试通知（验证 OS 集成是否正常）
//! - `notification_settings_changed`：UI 切换通知偏好后由前端调用，触发
//!   `notification::on_settings_changed` 让内部状态感知

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::{NotificationExt, PermissionState};

/// 检查当前是否已授予通知权限。
#[tauri::command]
pub async fn is_notification_permission_granted(app: AppHandle) -> Result<bool, String> {
    match app.notification().permission_state() {
        Ok(PermissionState::Granted) => Ok(true),
        Ok(_) => Ok(false),
        Err(e) => Err(format!("查询权限状态失败: {e}")),
    }
}

/// 弹出系统通知权限对话框，返回最终是否授予。
#[tauri::command]
pub async fn request_notification_permission(app: AppHandle) -> Result<bool, String> {
    match app.notification().request_permission() {
        Ok(PermissionState::Granted) => Ok(true),
        Ok(_) => Ok(false),
        Err(e) => Err(format!("请求权限失败: {e}")),
    }
}

/// 发一条测试通知。返回 Result 用于 UI 反馈。
#[tauri::command]
pub async fn test_notification(
    app: AppHandle,
    title: String,
    body: String,
) -> Result<(), String> {
    app.notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
        .map_err(|e| format!("发送测试通知失败: {e}"))
}

/// 前端切换通知设置后调用，让 NotificationState 感知变更。
#[tauri::command]
pub async fn notification_settings_changed(
    app: AppHandle,
) -> Result<(), String> {
    if let Some(state) = app.try_state::<crate::notification::NotificationState>() {
        crate::notification::on_settings_changed(state).await;
    }
    Ok(())
}