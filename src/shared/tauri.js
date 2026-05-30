const tauri = typeof window !== "undefined" ? window.__TAURI__ : undefined;

export const tauriInvoke = tauri?.core?.invoke;
export const tauriListen = tauri?.event?.listen;
