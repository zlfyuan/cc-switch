import { renderHook, act, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { useNotificationPermission } from "@/hooks/useNotificationPermission";

const isPermissionGrantedMock = vi.fn();
const requestPermissionMock = vi.fn();

vi.mock("@/lib/api/notification", () => ({
  notificationApi: {
    isPermissionGranted: (...args: unknown[]) => isPermissionGrantedMock(...args),
    requestPermission: (...args: unknown[]) => requestPermissionMock(...args),
    testNotification: vi.fn(),
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

beforeEach(() => {
  isPermissionGrantedMock.mockReset();
  requestPermissionMock.mockReset();
  toastSuccessMock.mockReset();
  toastErrorMock.mockReset();
});

describe("useNotificationPermission", () => {
  it("queries isPermissionGranted on mount and stores result", async () => {
    isPermissionGrantedMock.mockResolvedValue(true);

    const { result } = renderHook(() => useNotificationPermission());

    await waitFor(() => {
      expect(result.current.granted).toBe(true);
    });

    expect(isPermissionGrantedMock).toHaveBeenCalledTimes(1);
  });

  it("stores false when permission is not granted", async () => {
    isPermissionGrantedMock.mockResolvedValue(false);

    const { result } = renderHook(() => useNotificationPermission());

    await waitFor(() => {
      expect(result.current.granted).toBe(false);
    });
  });

  it("stores false when isPermissionGranted throws", async () => {
    isPermissionGrantedMock.mockRejectedValue(new Error("network"));

    const { result } = renderHook(() => useNotificationPermission());

    await waitFor(() => {
      expect(result.current.granted).toBe(false);
    });
  });

  it("request() returns true on grant and shows success toast", async () => {
    isPermissionGrantedMock.mockResolvedValue(false);
    requestPermissionMock.mockResolvedValue(true);

    const { result } = renderHook(() => useNotificationPermission());

    let granted: boolean | undefined;
    await act(async () => {
      granted = await result.current.request();
    });

    expect(granted).toBe(true);
    expect(result.current.granted).toBe(true);
    expect(toastSuccessMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock).not.toHaveBeenCalled();
  });

  it("request() returns false on denial and shows error toast", async () => {
    isPermissionGrantedMock.mockResolvedValue(false);
    requestPermissionMock.mockResolvedValue(false);

    const { result } = renderHook(() => useNotificationPermission());

    let granted: boolean | undefined;
    await act(async () => {
      granted = await result.current.request();
    });

    expect(granted).toBe(false);
    expect(result.current.granted).toBe(false);
    expect(toastErrorMock).toHaveBeenCalledTimes(1);
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });

  it("request() catches thrown errors and shows error toast", async () => {
    isPermissionGrantedMock.mockResolvedValue(false);
    requestPermissionMock.mockRejectedValue(new Error("boom"));

    const { result } = renderHook(() => useNotificationPermission());

    let granted: boolean | undefined;
    await act(async () => {
      granted = await result.current.request();
    });

    expect(granted).toBe(false);
    expect(toastErrorMock).toHaveBeenCalledTimes(1);
  });
});