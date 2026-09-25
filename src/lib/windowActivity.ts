import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { focusManager } from "@tanstack/react-query";
import { useSyncExternalStore } from "react";

let initialized = false;
const subscribe = (onChange: () => void) => focusManager.subscribe(onChange);
const isActive = () => focusManager.isFocused();

export function useWindowActive() {
  return useSyncExternalStore(subscribe, isActive, () => true);
}

export function initializeWindowActivity() {
  if (initialized) return;
  initialized = true;
  // Native focus events also cover minimizing and hiding the window to the tray.
  // Pause background queries instead of running a decorative heartbeat timer.
  focusManager.setEventListener((setFocused) => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const update = () => {
      if (!disposed) {
        setFocused(
          document.visibilityState !== "hidden" && document.hasFocus(),
        );
      }
    };
    update();
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    document.addEventListener("visibilitychange", update);
    if (isTauri()) {
      void getCurrentWindow()
        .onFocusChanged(({ payload }) => {
          if (!disposed)
            setFocused(payload && document.visibilityState !== "hidden");
        })
        .then((off) => {
          if (disposed) off();
          else unlisten = off;
        })
        .catch((error) =>
          console.error("Failed to observe window focus:", error),
        );
    }
    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
      document.removeEventListener("visibilitychange", update);
    };
  });
}
