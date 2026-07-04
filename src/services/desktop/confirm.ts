import { confirm as tauriConfirm } from "@tauri-apps/plugin-dialog";

export async function confirmDesktopDialog(message: string): Promise<boolean> {
  return tauriConfirm(message);
}
