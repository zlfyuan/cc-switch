# Local Notifications for Quota Limits & Reset Reminders

> 实现总结与设计文档 — 配合 plan 文件 `.claude/plans/declarative-baking-hartmanis.md` 阅读。

## 背景

CC Switch 已经在窗口内通过 `SubscriptionQuotaFooter` / `CodexOauthQuotaFooter` / `CopilotQuotaFooter` / `UsageFooter` 显示各 provider 的配额，但用户**不盯着窗口**时无任何通知渠道。

本次新增 OS 级桌面通知（Notification Center / Action Center / libnotify），覆盖三类场景：

1. **阈值告警** — 使用率跨过 80% / 95% / 100% 时推送
2. **重置提醒** — 重置前 5 分钟 + 重置瞬间各推一条
3. **自动切换提醒** — 配额耗尽自动切换到备用 provider 后通知

## 架构

**Rust 拥有投递 + 持久化；React 拥有 i18n 文案 + 设置 UI。**

- 新模块 `src-tauri/src/notification.rs` 负责：阈值去重、tokio 调度、AbortHandle 生命周期、SQLite 持久化、OS 投递
- 新模块 `src-tauri/src/i18n.rs` 提供 4 语言通知文案（zh / zh-TW / ja / en）
- `tauri-plugin-notification = "2"` 提供跨平台原生通知能力
- 设置 UI 新增 `<NotificationsSection />`（General tab）

### 关键设计取舍

| 取舍 | 决策 | 理由 |
|---|---|---|
| AppHandle 注入 | `OnceLock<AppHandle>`（fire-and-forget 广播） | 镜像 `usage_events.rs` 既有模式 |
| dedup 状态 | `app.manage(NotificationState{ db, dedup, tasks })` | 镜像 `CopilotAuthState`，可变状态走标准通道 |
| 调度器 | `tokio::spawn` + `AbortHandle` 句柄登记在 Mutex HashMap | 每次 quota 拉取可 cancel & reschedule；进程崩溃后从 SQLite 重建 |
| i18n | Rust 端 4 语言硬编码 `match` | 镜像 `tray.rs::TrayTexts::from_language`；窗口被隐藏时也能输出本地化文案 |
| 持久化 | SQLite `settings` 表 KV（key=`notification_dedup_state`） | 已有 `database/dao/settings.rs` 的 `set_setting` / `get_setting` 直接复用 |
| 重置提醒时机 | -5 min + 整点 | 用户偏好选项；与用户敲定 |
| 跨平台 plugin | `tauri-plugin-notification` v2 | 跨平台官方支持；macOS 自动弹权限、Linux libnotify、Windows Toast |
| 权限请求 UX | 开关 ON 才弹权限框 | 不在启动时骚扰用户；首次打开通知开关才请求 |
| 触发频率 | 5 min 一次 polling | 阈值告警用 UTC 日期去重；同一天同一档阈值只推一次 |

### 状态机

```
[setup] init(handle)
   └─> NotificationState::new(db) ──> app.manage()
   └─> bootstrap() ──> 从 SQLite 恢复未触发的 warn/reset 任务

[quota fetch] check_and_notify_subscription()
   ├─ 跨过 80/95/100 且今日未推 ──> deliver() ──> mark_threshold_fired()
   └─ 有 resetsAt 且在未来 ──> schedule_reset_pair()
       └─ 60s 抖动窗口检测 ──> abort 旧 + spawn 新 ──> 保存 SQLite

[reset task fires] fire_warn() / fire_reset()
   ├─ 标记 warn_fired/reset_fired 持久化
   └─ deliver()

[auto-switch with source="quota"] notify_auto_switched()
   └─ deliver()
```

## 触发点

| 命令 | 改动 |
|---|---|
| `get_subscription_quota` | 新增 `notification::check_and_notify_subscription` 调用 |
| `get_codex_oauth_quota` | **修复**：原本缺失的 `usage_cache.put_*` + `emit("usage-cache-updated")` + 新增 notification 调用 |
| `get_coding_plan_quota` | 加 `provider_id` / `provider_name` 参数（破坏性变更）+ notification 调用 |
| `get_balance` | 加 `provider_id` / `provider_name` 参数 + notification 调用 |
| `failover_switch::try_switch` | 加 `source: &str` 参数；`source == "quota"` 路径触发 `notify_auto_switched` |

所有 5 处 `try_switch` 调用点（4 处 forwarder + 1 处 proxy 恢复）传入 `"circuit_breaker"`。

## 用户可配置项（Settings）

```rust
pub enable_notifications: bool,           // 总开关
pub notify_on_threshold_reached: bool,    // 默认 true
pub notify_on_reset_approaching: bool,    // 默认 true
pub notify_on_auto_switch: bool,          // 默认 true
```

`#[serde(default)]` 保证 v3.16 之前用户的 `settings.json` 兼容加载。

## 新增/修改的文件

### 新增（8）

- `src-tauri/src/notification.rs` — 核心模块
- `src-tauri/src/i18n.rs` — 4 语言文案
- `src-tauri/src/commands/notification.rs` — Tauri 命令
- `src/lib/api/notification.ts` — 前端 invoke 封装
- `src/hooks/useNotificationPermission.ts` — 权限请求 hook
- `src/components/settings/NotificationsSection.tsx` — 设置面板
- `tests/hooks/useNotificationPermission.test.tsx`
- `tests/components/settings/NotificationsSection.test.tsx`

### 修改（15）

- `src-tauri/Cargo.toml` — 加 `tauri-plugin-notification = "2"`
- `package.json` — 加 `@tauri-apps/plugin-notification`
- `src-tauri/capabilities/default.json` — 加 4 个 notification 权限
- `src-tauri/src/lib.rs` — 注册插件、init、commands
- `src-tauri/src/settings.rs` — 4 个新字段
- `src-tauri/src/commands/mod.rs` — 模块导出
- `src-tauri/src/commands/{subscription,codex_oauth,coding_plan,balance}.rs`
- `src-tauri/src/proxy/failover_switch.rs` — `source` 参数 + `notify_auto_switched` 调用
- `src-tauri/src/proxy/forwarder.rs` — 4 处 `try_switch` 调用
- `src-tauri/src/commands/proxy.rs` — 1 处 `try_switch` 调用
- `src/types.ts` — `Settings` 接口加 4 字段
- `src/lib/api/subscription.ts` — API 签名加可选参数
- `src/components/UsageScriptModal.tsx` — 调用点传 provider id/name
- `src/components/settings/SettingsPage.tsx` — 插入 `<NotificationsSection />`
- `src/i18n/locales/{en,zh,ja,zh-TW}.json` — ~13 个键

## 已知遗留 / 后续优化

1. **`fire_warn`/`fire_reset` 绕过 in-memory mutex** — 直接读写 SQLite。功能正确，但每次触发重读 JSON。可后续把 `Arc<Mutex<DedupState>>` 注入 spawn 闭包优化。
2. **auto-switch 文案硬编码 "failover provider"** — 应该从 `failover_queue` DAO 取出下一个 provider 的真实 name。当前 `failover_switch::do_switch` 拿到的是 `provider_id`，不是 name。
3. **i18n.rs 的 `LangCache` 用 `Box::leak`** — 进程内最多泄露 ~200 字节 × 4 语言，可忽略。
4. **未实现"Do Not Disturb" 时段** — 用户夜间 22:00–08:00 仍会被吵醒。如有需求可在 settings 加 `notifyQuietHoursStart/End` 字段。
5. **未实现"通知点击跳转到对应 provider"** — 当前通知 fire-and-forget，无 action handler。

## 验证清单

### 编译

```bash
cargo check --manifest-path src-tauri/Cargo.toml
pnpm typecheck
```

### 单元测试

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib notification:: i18n::
pnpm test:unit
```

### 手动 E2E（按 plan 文件 verification 章节 10 项）

1. 权限 UX — 设置 → 通知 → 开总开关，验证 macOS 权限弹窗；拒绝后 toggle 自动回滚
2. 阈值告警 — 找一个低配 provider，观察跨过 80% 单次推送；同一天回落后再升不重复推
3. 重置提醒 — 找重置窗口 < 6 min 的 provider，验证 -5min + 整点两条
4. 重启持久化 — 调度后强杀进程，重启验证提醒仍按时触发
5. 自动切换 — 启用 auto-failover + 备份 provider，把当前推到 100%，验证 OS 通知 + 切换
6. 窗口隐藏投递 — 最小化或关闭主窗口，点"Send test notification"，验证通知中心显示
7. 清理 — 删除带调度任务的 provider，验证 AbortHandle 取消（不触发幽灵通知）
8. 权限拒绝回归 — 注释掉 `notification:allow-request-permission`，验证前端拿到明确错误（不静默失败）
9. i18n 一致性 — 切换 zh/en/ja，下一次触发通知文案跟当前语言
10. 轻量模式 — 进入轻量模式触发通知，验证仍能投递

## 相关链接

- Plan: `.claude/plans/declarative-baking-hartmanis.md`
- 既有事件模式: `src-tauri/src/usage_events.rs`（AppHandle 注入）
- 既有状态模式: `src-tauri/src/proxy/failover_switch.rs`（RwLock 保护去重）
- 既有 i18n 模式: `src-tauri/src/tray.rs::TrayTexts::from_language`
- tauri-plugin-notification 文档: <https://v2.tauri.app/plugin/notification/>