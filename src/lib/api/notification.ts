import { invoke } from "@tauri-apps/api/core";

export const notificationApi = {
  isPermissionGranted: (): Promise<boolean> =>
    invoke("is_notification_permission_granted"),
  requestPermission: (): Promise<boolean> =>
    invoke("request_notification_permission"),
  testNotification: (title: string, body: string): Promise<void> =>
    invoke("test_notification", { title, body }),
  settingsChanged: (): Promise<void> =>
    invoke("notification_settings_changed"),
};