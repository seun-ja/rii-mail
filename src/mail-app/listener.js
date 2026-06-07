import { listen } from "@tauri-apps/api/event";

export async function populationListener(onProgress) {
  const unlisten = await listen("population-progress", (event) => {
    const payload = Number(event.payload);
    const progress = Number.isFinite(payload) ? payload : 0;
    onProgress(progress);
  });

  return unlisten;
}
