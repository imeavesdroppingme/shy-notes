import { openUrl } from "@tauri-apps/plugin-opener";
import { getCurrentWindow } from "@tauri-apps/api/window";

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
