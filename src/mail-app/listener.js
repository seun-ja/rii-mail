export async function inboxFolderPopulationListener(onProgress, listen) {
  const unlisten = await listen("inbox-population-progress", (event) => {
    const payload = Number(event.payload);
    const progress = Number.isFinite(payload) ? payload : 0;
    onProgress(progress);
  });

  return unlisten;
}

export async function sentFolderPopulationListener(onProgress, listen) {
  const unlisten = await listen("sent-population-progress", (event) => {
    const payload = Number(event.payload);
    const progress = Number.isFinite(payload) ? payload : 0;
    onProgress(progress);
  });

  return unlisten;
}

export async function inboxInitialPopulationCompleted(callback, listen) {
  const unlisten = await listen("inbox-initial-population-complete", () => {
    callback();
  });

  return unlisten;
}

export async function sentInitialPopulationCompleted(callback, listen) {
  const unlisten = await listen("sent-initial-population-complete", () => {
    callback();
  });

  return unlisten;
}
