import type { NotificationHealth } from "../types";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Banner } from "./Banner";

const SETTINGS_URL =
  "x-apple.systempreferences:com.apple.Notifications-Settings.extension";

export function NotificationBanner({ health }: { health: NotificationHealth }) {
  if (health.status !== "denied") return null;
  return (
    <Banner
      message={health.message}
      action={
        <button
          onClick={() => {
            openUrl(SETTINGS_URL).catch((e) => {
              console.error("failed to open notification settings", e);
            });
          }}
          className="shrink-0 rounded-md border border-hn-border bg-card px-3 py-1 text-[12px] font-semibold text-rust transition-colors hover:bg-paper"
        >
          Open Settings
        </button>
      }
    />
  );
}
