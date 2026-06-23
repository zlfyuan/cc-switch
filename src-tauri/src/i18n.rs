//! 通知文案多语言表
//!
//! 为 `notification` 模块提供按语言解析的通知文本。
//! 与 `tray.rs::TrayTexts` 同模式：4 语言硬编码 `match`，方便单点修改。
//!
//! 占位符约定（由 `notification::render_*` 替换）：
//! - `{{provider}}` —— 供应商名称
//! - `{{tier}}` —— 套餐短名（如 "5h"、"7d"、"Opus 7d"）
//! - `{{percent}}` —— 阈值百分比
//! - `{{from}}` / `{{to}}` —— 自动切换前后供应商名
//!
//! 文本长度上限建议：title ≤ 40 字符；body ≤ 100 字符（macOS Notification Center
//! / Windows Toast / GNOME Notifications 都会截断）。

use std::sync::OnceLock;

use crate::settings;

/// 通知文案集合。所有字段都是 `&'static str`，匹配 4 语言硬编码风格。
#[derive(Debug, Clone, Copy)]
pub struct NotificationTexts {
    pub threshold_title: &'static str,
    pub threshold_body: &'static str,
    pub reset_approaching_title: &'static str,
    pub reset_approaching_body: &'static str,
    pub reset_title: &'static str,
    pub reset_body: &'static str,
    pub auto_switched_title: &'static str,
    pub auto_switched_body: &'static str,
    pub exhausted_title: &'static str,
    pub exhausted_body: &'static str,
}

impl NotificationTexts {
    /// 4 语言 fallback：zh-TW / ja / en / 默认 zh。
    pub fn from_language(language: &str) -> Self {
        match language {
            "zh-TW" => Self::zh_tw(),
            "ja" => Self::ja(),
            "en" => Self::en(),
            _ => Self::zh(),
        }
    }

    fn zh() -> Self {
        Self {
            threshold_title: "配额提醒",
            threshold_body: "{{provider}} {{tier}} 已使用 {{percent}}%",
            reset_approaching_title: "即将重置",
            reset_approaching_body: "{{provider}} {{tier}} 将在 5 分钟后重置",
            reset_title: "已重置",
            reset_body: "{{provider}} {{tier}} 配额已重置",
            auto_switched_title: "已自动切换",
            auto_switched_body: "已从 {{from}} 切换到 {{to}}",
            exhausted_title: "{{provider}} 已用尽",
            exhausted_body: "{{provider}} 余额或额度已用尽，请充值或切换",
        }
    }

    fn zh_tw() -> Self {
        Self {
            threshold_title: "配額提醒",
            threshold_body: "{{provider}} {{tier}} 已使用 {{percent}}%",
            reset_approaching_title: "即將重置",
            reset_approaching_body: "{{provider}} {{tier}} 將在 5 分鐘後重置",
            reset_title: "已重置",
            reset_body: "{{provider}} {{tier}} 配額已重置",
            auto_switched_title: "已自動切換",
            auto_switched_body: "已從 {{from}} 切換到 {{to}}",
            exhausted_title: "{{provider}} 已用盡",
            exhausted_body: "{{provider}} 餘額或額度已用盡，請儲值或切換",
        }
    }

    fn ja() -> Self {
        Self {
            threshold_title: "クォータ警告",
            threshold_body: "{{provider}} {{tier}} が {{percent}}% 使用中",
            reset_approaching_title: "リセット間近",
            reset_approaching_body: "{{provider}} {{tier}} は 5 分後にリセット",
            reset_title: "リセット完了",
            reset_body: "{{provider}} {{tier}} のクォータがリセットされました",
            auto_switched_title: "自動切り替え",
            auto_switched_body: "{{from}} から {{to}} に切り替えました",
            exhausted_title: "{{provider}} が枯渇",
            exhausted_body: "{{provider}} の残高または割当が枯渇しました。チャージまたは切り替えが必要です",
        }
    }

    fn en() -> Self {
        Self {
            threshold_title: "Quota alert",
            threshold_body: "{{provider}} {{tier}} at {{percent}}%",
            reset_approaching_title: "Reset imminent",
            reset_approaching_body: "{{provider}} {{tier}} resets in 5 minutes",
            reset_title: "Quota reset",
            reset_body: "{{provider}} {{tier}} quota has reset",
            auto_switched_title: "Auto-switched",
            auto_switched_body: "Switched from {{from}} to {{to}}",
            exhausted_title: "{{provider}} exhausted",
            exhausted_body: "{{provider}} balance or credits exhausted. Top up or switch providers.",
        }
    }
}

/// 按需加载当前语言的通知文案快照。
///
/// `notification::check_and_notify_*` 在每次触发时调用 `snapshot()`，
/// 得到的是 `&'static NotificationTexts`（因为 `from_language` 返回的
/// 都是 `Self::xxx()` 常量函数产生的 `'static` 数据）。
///
/// 使用 OnceLock 缓存最近一次解析结果，避免每条通知都跑 match。
pub struct TextsLoader;

impl TextsLoader {
    fn resolve(language: &str) -> &'static NotificationTexts {
        // 静态缓存：每次进程最多解析一次语言对应的表项。
        static CACHE: OnceLock<parking_lot_compat::LangCache> = OnceLock::new();
        let cache = CACHE.get_or_init(parking_lot_compat::LangCache::new);
        cache.get_or_resolve(language)
    }

    /// 取当前 settings.language 对应的 NotificationTexts 引用。
    pub fn resolve_for_current_language() -> LangRef {
        let lang = settings::get_settings().language.as_deref().unwrap_or("zh");
        LangRef(Self::resolve(lang))
    }
}

/// 持有 `&'static NotificationTexts` 的 RAII 句柄，方便调用方写
/// `texts.snapshot()` 而非 `&*texts`。
pub struct LangRef(&'static NotificationTexts);

impl LangRef {
    pub fn snapshot(&self) -> &'static NotificationTexts {
        self.0
    }
}

impl std::ops::Deref for LangRef {
    type Target = NotificationTexts;
    fn deref(&self) -> &NotificationTexts {
        self.0
    }
}

// 简单的语言缓存实现（不引入 parking_lot 或 dashmap 等新依赖）。
mod parking_lot_compat {
    use std::sync::Mutex;

    pub struct LangCache {
        inner: Mutex<Option<(String, &'static super::NotificationTexts)>>,
    }

    impl LangCache {
        pub fn new() -> Self {
            Self {
                inner: Mutex::new(None),
            }
        }

        pub fn get_or_resolve(&self, language: &str) -> &'static super::NotificationTexts {
            let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((cached_lang, cached)) = guard.as_ref() {
                if *cached_lang == language {
                    return cached;
                }
            }
            let resolved = super::NotificationTexts::from_language(language);
            // SAFETY: `from_language` 返回的所有 `Self::xxx()` 都是字面量
            // `&'static str` 包装的 `NotificationTexts`，等价于 `'static`。
            let leaked: &'static super::NotificationTexts = Box::leak(Box::new(resolved));
            *guard = Some((language.to_string(), leaked));
            leaked
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_language_handles_all_supported() {
        for lang in ["zh", "zh-TW", "ja", "en", "fr"] {
            let t = NotificationTexts::from_language(lang);
            assert!(!t.threshold_title.is_empty());
            assert!(!t.threshold_body.is_empty());
            assert!(!t.reset_approaching_body.is_empty());
            assert!(!t.exhausted_body.is_empty());
        }
    }

    #[test]
    fn unknown_language_falls_back_to_zh() {
        let zh = NotificationTexts::from_language("zh");
        let fr = NotificationTexts::from_language("fr");
        assert_eq!(zh.threshold_title, fr.threshold_title);
    }

    #[test]
    fn bodies_under_120_chars() {
        // 防止长文案在通知中心被截断。替换后检查。
        let t = NotificationTexts::from_language("zh");
        let long_provider = "x".repeat(50);
        for body in [
            t.threshold_body,
            t.reset_approaching_body,
            t.reset_body,
            t.auto_switched_body,
            t.exhausted_body,
        ] {
            let filled = body
                .replace("{{provider}}", &long_provider)
                .replace("{{tier}}", "5h")
                .replace("{{percent}}", "100")
                .replace("{{from}}", &long_provider)
                .replace("{{to}}", &long_provider);
            assert!(
                filled.chars().count() <= 120,
                "body too long ({} chars): {filled}",
                filled.chars().count()
            );
        }
    }

    #[test]
    fn cache_returns_same_reference_for_same_language() {
        let a = TextsLoader::resolve("en");
        let b = TextsLoader::resolve("en");
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn cache_switches_when_language_changes() {
        let _a = TextsLoader::resolve("zh");
        let b = TextsLoader::resolve("en");
        assert_eq!(b.threshold_title, "Quota alert");
    }
}