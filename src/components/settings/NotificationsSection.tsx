import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Bell, Clock, Shuffle, TrendingUp, Send } from "lucide-react";
import { toast } from "sonner";

import type { SettingsFormState } from "@/hooks/useSettings";
import { ToggleRow } from "@/components/ui/toggle-row";
import { Button } from "@/components/ui/button";
import { useNotificationPermission } from "@/hooks/useNotificationPermission";
import { notificationApi } from "@/lib/api/notification";

interface NotificationsSectionProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

export function NotificationsSection({
  settings,
  onChange,
}: NotificationsSectionProps) {
  const { t } = useTranslation();
  const { granted, request } = useNotificationPermission();

  const enabled = !!settings.enableNotifications;

  // 用户打开总开关 → 弹权限框；被拒则回滚
  const handleToggleEnabled = useCallback(
    async (next: boolean) => {
      if (next) {
        const ok = await request();
        if (ok) {
          onChange({ enableNotifications: true });
        } else {
          onChange({ enableNotifications: false });
        }
      } else {
        onChange({ enableNotifications: false });
      }
    },
    [onChange, request],
  );

  const handleTestSend = useCallback(async () => {
    try {
      await notificationApi.testNotification(
        t("settings.notifications.testTitle", {
          defaultValue: "CC Switch 测试通知",
        }),
        t("settings.notifications.testBody", {
          defaultValue: "如果你看到这条消息，OS 通知已正常工作。",
        }),
      );
      toast.success(
        t("settings.notifications.testSuccess", {
          defaultValue: "测试通知已发送",
        }),
      );
    } catch (e) {
      toast.error(
        t("settings.notifications.testFailed", {
          defaultValue: "发送失败: {{error}}",
          error: (e as Error)?.message ?? String(e),
        }),
      );
    }
  }, [t]);

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <Bell className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">
          {t("settings.notifications.title", { defaultValue: "通知" })}
        </h3>
      </div>

      <div className="space-y-3">
        <ToggleRow
          icon={<Bell className="h-4 w-4 text-blue-500" />}
          title={t("settings.notifications.enableNotifications", {
            defaultValue: "启用桌面通知",
          })}
          description={t("settings.notifications.enableNotificationsDescription", {
            defaultValue:
              "在系统通知中心显示配额提醒和重置提醒。需要授予系统通知权限。",
          })}
          checked={enabled}
          onCheckedChange={handleToggleEnabled}
        />

        {enabled && granted === false && (
          <div className="rounded-md border border-amber-500/40 bg-amber-500/5 px-3 py-2 text-xs text-amber-700 dark:text-amber-400">
            {t("settings.notifications.permissionWarning", {
              defaultValue:
                "系统通知权限未授予。请在系统设置 → 通知 → CC Switch 中开启。",
            })}
          </div>
        )}

        <ToggleRow
          icon={<TrendingUp className="h-4 w-4 text-orange-500" />}
          title={t("settings.notifications.notifyOnThresholdReached", {
            defaultValue: "配额阈值告警",
          })}
          description={t(
            "settings.notifications.notifyOnThresholdReachedDescription",
            {
              defaultValue:
                "使用率跨过 80% / 95% / 100% 时通知（每天每个阈值最多一次）",
            },
          )}
          checked={!!settings.notifyOnThresholdReached}
          onCheckedChange={(value) =>
            onChange({ notifyOnThresholdReached: value })
          }
          disabled={!enabled}
        />

        <ToggleRow
          icon={<Clock className="h-4 w-4 text-purple-500" />}
          title={t("settings.notifications.notifyOnResetApproaching", {
            defaultValue: "重置提醒",
          })}
          description={t(
            "settings.notifications.notifyOnResetApproachingDescription",
            {
              defaultValue:
                "配额重置前 5 分钟 + 重置瞬间各推送一条提醒",
            },
          )}
          checked={!!settings.notifyOnResetApproaching}
          onCheckedChange={(value) =>
            onChange({ notifyOnResetApproaching: value })
          }
          disabled={!enabled}
        />

        <ToggleRow
          icon={<Shuffle className="h-4 w-4 text-emerald-500" />}
          title={t("settings.notifications.notifyOnAutoSwitch", {
            defaultValue: "自动切换提醒",
          })}
          description={t("settings.notifications.notifyOnAutoSwitchDescription", {
            defaultValue:
              "配额耗尽自动切换到备用供应商时通知（仅在启用自动故障转移时有效）",
          })}
          checked={!!settings.notifyOnAutoSwitch}
          onCheckedChange={(value) => onChange({ notifyOnAutoSwitch: value })}
          disabled={!enabled}
        />

        <div className="pt-2">
          <Button
            variant="outline"
            size="sm"
            onClick={handleTestSend}
            disabled={!enabled}
            className="gap-2"
          >
            <Send className="h-3.5 w-3.5" />
            {t("settings.notifications.testButton", {
              defaultValue: "发送测试通知",
            })}
          </Button>
        </div>
      </div>
    </section>
  );
}