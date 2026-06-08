import { listen } from "@tauri-apps/api/event";

export async function inboxPopulationListener(onProgress) {
  const unlisten = await listen("inbox-population-progress", (event) => {
    const payload = Number(event.payload);
    const progress = Number.isFinite(payload) ? payload : 0;
    onProgress(progress);
  });

  return unlisten;
}

export async function sentPopulaPopulationListener(onProgress) {
  const unlisten = await listen("sent-population-progress", (event) => {
    const payload = Number(event.payload);
    const progress = Number.isFinite(payload) ? payload : 0;
    onProgress(progress);
  });

  return unlisten;
}
