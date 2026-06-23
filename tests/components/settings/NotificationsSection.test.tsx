import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { NotificationsSection } from "@/components/settings/NotificationsSection";

const isPermissionGrantedMock = vi.fn();
const requestPermissionMock = vi.fn();
const testNotificationMock = vi.fn();

vi.mock("@/lib/api/notification", () => ({
  notificationApi: {
    isPermissionGranted: (...args: unknown[]) => isPermissionGrantedMock(...args),
    requestPermission: (...args: unknown[]) => requestPermissionMock(...args),
    testNotification: (...args: unknown[]) => testNotificationMock(...args),
    settingsChanged: vi.fn(),
  },
}));

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string }) =>
      opts?.defaultValue ?? key,
    i18n: { language: "en" },
  }),
}));

function renderWithQueryClient(ui: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={client}>{ui}</QueryClientProvider>);
}

beforeEach(() => {
  isPermissionGrantedMock.mockReset();
  requestPermissionMock.mockReset();
  testNotificationMock.mockReset();
  toastSuccessMock.mockReset();
  toastErrorMock.mockReset();
});

const baseSettings = {
  showInTray: true,
  minimizeToTrayOnClose: true,
  enableNotifications: false,
  notifyOnThresholdReached: true,
  notifyOnResetApproaching: true,
  notifyOnAutoSwitch: true,
  language: "en" as const,
};

describe("NotificationsSection", () => {
  it("renders four toggle rows", () => {
    isPermissionGrantedMock.mockResolvedValue(false);
    renderWithQueryClient(
      <NotificationsSection
        settings={baseSettings}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByText("Enable desktop notifications")).toBeInTheDocument();
    expect(screen.getByText("Quota threshold alerts")).toBeInTheDocument();
    expect(screen.getByText("Reset reminders")).toBeInTheDocument();
    expect(screen.getByText("Auto-switch notifications")).toBeInTheDocument();
  });

  it("disables sub-toggles when enableNotifications is false", () => {
    isPermissionGrantedMock.mockResolvedValue(false);
    renderWithQueryClient(
      <NotificationsSection
        settings={baseSettings}
        onChange={vi.fn()}
      />,
    );

    const subSwitches = screen
      .getAllByRole("switch")
      .filter((s) => (s as HTMLInputElement).disabled);
    expect(subSwitches.length).toBeGreaterThanOrEqual(3);
  });

  it("clicking the master toggle calls requestPermission and only enables on grant", async () => {
    isPermissionGrantedMock.mockResolvedValue(false);
    requestPermissionMock.mockResolvedValue(true);
    const onChange = vi.fn();

    renderWithQueryClient(
      <NotificationsSection
        settings={baseSettings}
        onChange={onChange}
      />,
    );

    const masterSwitch = screen
      .getAllByRole("switch")
      .find((s) => !(s as HTMLInputElement).disabled)!;

    await userEvent.click(masterSwitch);

    expect(requestPermissionMock).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith({ enableNotifications: true });
  });

  it("reverts to disabled when permission is denied", async () => {
    isPermissionGrantedMock.mockResolvedValue(false);
    requestPermissionMock.mockResolvedValue(false);
    const onChange = vi.fn();

    renderWithQueryClient(
      <NotificationsSection
        settings={baseSettings}
        onChange={onChange}
      />,
    );

    const masterSwitch = screen
      .getAllByRole("switch")
      .find((s) => !(s as HTMLInputElement).disabled)!;

    await userEvent.click(masterSwitch);

    expect(onChange).toHaveBeenCalledWith({ enableNotifications: false });
  });

  it("sends a test notification when the test button is clicked", async () => {
    isPermissionGrantedMock.mockResolvedValue(true);
    testNotificationMock.mockResolvedValue(undefined);

    renderWithQueryClient(
      <NotificationsSection
        settings={{ ...baseSettings, enableNotifications: true }}
        onChange={vi.fn()}
      />,
    );

    await userEvent.click(screen.getByText("Send test notification"));

    expect(testNotificationMock).toHaveBeenCalledTimes(1);
    expect(toastSuccessMock).toHaveBeenCalledTimes(1);
  });
});