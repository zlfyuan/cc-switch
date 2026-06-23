import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { notificationApi } from "@/lib/api/notification";

/**
 * 处理"启用通知"开关时的权限请求流程：
 *
 * 1. UI 调用 `request()`，弹出系统权限对话框（仅 macOS/Windows 第一次有）
 * 2. 用户授权 → 返回 true → 通知已开启
 * 3. 用户拒绝 → 返回 false → 调用方应回滚 toggle 状态
 *
 * 同时查询初始权限状态，便于在用户已授权时直接跳过弹窗。
 */
export function useNotificationPermission() {
  const { t } = useTranslation();
  const [granted, setGranted] = useState<boolean | null>(null);

  useEffect(() => {
    let mounted = true;
    notificationApi
      .isPermissionGranted()
      .then((v) => {
        if (mounted) setGranted(v);
      })
      .catch(() => {
        if (mounted) setGranted(false);
      });
    return () => {
      mounted = false;
    };
  }, []);

  const request = useCallback(async (): Promise<boolean> => {
    try {
      const ok = await notificationApi.requestPermission();
      setGranted(ok);
      if (ok) {
        toast.success(
          t("notifications.permissionGranted", {
            defaultValue: "通知权限已授予",
          }),
        );
      } else {
        toast.error(
          t("notifications.permissionDenied", {
            defaultValue:
              "通知权限被拒绝。请在系统设置中手动开启 CC Switch 的通知权限。",
          }),
        );
      }
      return ok;
    } catch (e) {
      toast.error(
        t("notifications.permissionPrompted", {
          defaultValue: "请求通知权限失败: {{error}}",
          error: (e as Error)?.message ?? String(e),
        }),
      );
      return false;
    }
  }, [t]);

  return { granted, request };
}