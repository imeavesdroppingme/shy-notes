import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getCurrentWindow } from "@tauri-apps/api/window";

type AppInfo = {
  version: string;
  build_date: string;
};

const openExternal = (url: string) => (ev: Event) => {
  ev.preventDefault();
  void openUrl(url).catch((err) => console.warn("[shy-notes] openUrl failed", err));
};

document
  .getElementById("about-github")
  ?.addEventListener("click", openExternal("https://github.com/imeavesdroppingme/shy-notes"));
document
  .getElementById("about-hello")
  ?.addEventListener("click", openExternal("https://imeavesdropping.com"));

document.getElementById("close")?.addEventListener("click", () => {
  void getCurrentWindow().close();
});

void (async () => {
  const meta = document.getElementById("about-meta");
  if (!meta) return;
  try {
    const info = await invoke<AppInfo>("get_app_info");
    meta.textContent = `${info.version} · built ${info.build_date}`;
  } catch (err) {
    console.warn("[shy-notes] get_app_info failed", err);
    meta.textContent = "Version unknown";
  }
})();
