import { SvelteMap } from "svelte/reactivity";

export type ToastType = "error" | "info" | "success";

export interface Toast {
  id: string;
  message: string;
  type: ToastType;
  createdAt: number;
}

const toasts = new SvelteMap<string, Toast>();
const timers = new Map<string, ReturnType<typeof setTimeout>>();

let counter = 0;

/** Duration in ms before auto-dismiss. 0 = never. */
const DURATIONS: Record<ToastType, number> = {
  error: 6000,
  info: 4000,
  success: 3000,
};

export function addToast(
  message: string,
  type: ToastType = "error",
  duration?: number,
): string {
  const id = `toast-${++counter}`;
  toasts.set(id, { id, message, type, createdAt: Date.now() });

  const ms = duration ?? DURATIONS[type];
  if (ms > 0) {
    timers.set(
      id,
      setTimeout(() => removeToast(id), ms),
    );
  }

  return id;
}

export function removeToast(id: string): void {
  toasts.delete(id);
  const timer = timers.get(id);
  if (timer) {
    clearTimeout(timer);
    timers.delete(id);
  }
}

export function getToasts(): SvelteMap<string, Toast> {
  return toasts;
}
