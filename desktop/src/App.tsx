import { Fragment, useEffect, useMemo, useRef, useState, type ReactElement } from "react";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { openPath, openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { currentMonitor } from "@tauri-apps/api/window";
import { api } from "./api";
import { useTaskSpeeds } from "./useTaskSpeeds";
import type { Category, Task, TaskStatus, TaskWithSpeed } from "./types";
import logo from "./assets/odm-logo.png";
import "./App.css";

function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  return `${(bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

function formatSpeed(bytesPerSec: number): string {
  return bytesPerSec > 0 ? `${formatBytes(bytesPerSec)}/s` : "";
}

function useAppVersion(): string {
  const [version, setVersion] = useState("");
  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);
  return version;
}

/** Compares dotted-integer versions ("1.2.10" vs "1.3.0"); positive when `a` is newer. */
function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map((n) => parseInt(n, 10) || 0);
  const pb = b.split(".").map((n) => parseInt(n, 10) || 0);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const diff = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

const UPDATE_REPO = "muzammilijaz/odm";
const CHROME_EXTENSION_URL = "https://chromewebstore.google.com/detail/odm-open-download-manager/lfpiggopnkjdgedghgapjnmijgckebkd";
const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

const VIDEO_QUALITY_OPTIONS = [
  { value: "best", label: "Best available" },
  { value: "2160", label: "2160p (4K)" },
  { value: "1440", label: "1440p (2K)" },
  { value: "1080", label: "1080p (Full HD)" },
  { value: "720", label: "720p (HD)" },
  { value: "480", label: "480p" },
  { value: "360", label: "360p" },
  { value: "240", label: "240p" },
  { value: "144", label: "144p" },
] as const;

function qualityLabel(height: number | null): string {
  if (!height) return "Best";
  if (height === 2160) return "2160p (4K)";
  if (height === 1440) return "1440p (2K)";
  return `${height}p`;
}

function taskQualityLabel(task: Task): string {
  return qualityLabel(task.actual_video_quality ?? task.video_quality);
}

function compactQualityLabel(height: number): string {
  if (height === 2160) return "4K";
  if (height === 1440) return "2K";
  return `${height}p`;
}

function qualityFallbackMessage(task: Task): string | null {
  if (!task.video_quality || !task.actual_video_quality || task.video_quality === task.actual_video_quality) return null;
  return `${compactQualityLabel(task.video_quality)} not available — downloaded in ${compactQualityLabel(task.actual_video_quality)}`;
}

function sendClickableStartedNotification(body: string) {
  try {
    const notification = new window.Notification("Download started", { body, icon: logo });
    notification.onclick = () => {
      void api.showMainWindow();
      notification.close();
    };
  } catch {
    // Some platforms expose notifications only through Tauri's wrapper. The
    // custom ODM popup remains clickable on those platforms.
    sendNotification({ title: "Download started", body });
  }
}

function looksAutoRenamed(path: string): boolean {
  const filename = path.split(/[\\/]/).pop() || "";
  return /-\d+(?=\.[^.]+$|$)/.test(filename);
}

/** Polls the GitHub Releases API for a newer tag than the running app's
 * version. Silent on any failure (offline, rate-limited) -- this is a
 * courtesy notification, not something worth surfacing an error for. A
 * dismissed version is remembered in localStorage so it doesn't nag again
 * until an even newer release ships. */
function useUpdateCheck(currentVersion: string) {
  const [update, setUpdate] = useState<{ version: string; url: string } | null>(null);

  useEffect(() => {
    if (!currentVersion) return;
    let cancelled = false;

    async function check() {
      try {
        const res = await fetch(`https://api.github.com/repos/${UPDATE_REPO}/releases/latest`);
        if (!res.ok) return;
        const data = await res.json();
        const remote = String(data.tag_name || "").replace(/^v/i, "");
        if (!remote || compareVersions(remote, currentVersion) <= 0) return;
        if (cancelled) return;
        if (localStorage.getItem("odm-update-dismissed") === remote) return;
        setUpdate({ version: remote, url: data.html_url || `https://github.com/${UPDATE_REPO}/releases` });
      } catch {
        // offline or rate-limited -- try again next interval
      }
    }

    check();
    const interval = setInterval(check, UPDATE_CHECK_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [currentVersion]);

  function dismiss() {
    if (update) localStorage.setItem("odm-update-dismissed", update.version);
    setUpdate(null);
  }

  return { update, dismiss };
}

type Theme = "light" | "dark";
const THEME_STORAGE_KEY = "odm-theme";

/** Explicit light/dark choice, persisted across launches. Falls back to the
 * OS preference the first time the app runs (before any choice is saved). */
function useTheme(): [Theme, () => void] {
  const [theme, setTheme] = useState<Theme>(() => {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (stored === "light" || stored === "dark") return stored;
    return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  });

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  }, [theme]);

  function toggleTheme() {
    setTheme((t) => (t === "dark" ? "light" : "dark"));
  }

  return [theme, toggleTheme];
}

type NotifyKind = "start" | "complete" | "failed";
interface NotifyPrefs {
  start: boolean;
  complete: boolean;
  failed: boolean;
}
const NOTIFY_DEFAULTS: NotifyPrefs = { start: false, complete: true, failed: true };

/** OS-notification toggles, persisted via the same backend key/value store
 * cookies settings use. "start" defaults off since a batch of downloads
 * kicking off at once would otherwise spam the notification tray. */
function useNotificationPrefs() {
  const [prefs, setPrefs] = useState<NotifyPrefs>(NOTIFY_DEFAULTS);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    Promise.all([api.getSetting("notify_start"), api.getSetting("notify_complete"), api.getSetting("notify_failed")])
      .then(([start, complete, failed]) => {
        setPrefs({
          start: start === null ? NOTIFY_DEFAULTS.start : start === "true",
          complete: complete === null ? NOTIFY_DEFAULTS.complete : complete === "true",
          failed: failed === null ? NOTIFY_DEFAULTS.failed : failed === "true",
        });
      })
      .finally(() => setLoaded(true));
  }, []);

  async function setPref(kind: NotifyKind, value: boolean) {
    setPrefs((p) => ({ ...p, [kind]: value }));
    await api.setSetting(`notify_${kind}`, value ? "true" : "false");
  }

  return { prefs, setPref, loaded };
}

function NotificationSettings({ prefs, setPref, loaded }: { prefs: NotifyPrefs; setPref: (kind: NotifyKind, value: boolean) => void; loaded: boolean }) {
  if (!loaded) return null;
  const rows: Array<{ key: NotifyKind; label: string; hint: string }> = [
    { key: "start", label: "Download started", hint: "Notify when a download begins" },
    { key: "complete", label: "Download completed", hint: "Notify when a download finishes successfully" },
    { key: "failed", label: "Download failed", hint: "Notify when a download fails" },
  ];

  return (
    <div className="category">
      {rows.map((r) => (
        <label key={r.key} className="modal__checkbox" style={{ justifyContent: "space-between" }}>
          <span>
            <strong>{r.label}</strong>
            <div className="category__folder">{r.hint}</div>
          </span>
          <input type="checkbox" checked={prefs[r.key]} onChange={(e) => setPref(r.key, e.currentTarget.checked)} />
        </label>
      ))}
    </div>
  );
}

interface DialogPrefs {
  /** Show the category/save-path preview in the Add Download modal before
   * a download actually starts. */
  showStartDetails: boolean;
  /** Show the blocking "Download complete" dialog when a task finishes
   * (in addition to the toast + OS notification, which have their own
   * independent toggles). */
  showCompleteDialog: boolean;
}
const DIALOG_PREF_DEFAULTS: DialogPrefs = { showStartDetails: true, showCompleteDialog: true };

/** Persisted the same way as notification prefs -- backend key/value store. */
function useDialogPrefs() {
  const [prefs, setPrefs] = useState<DialogPrefs>(DIALOG_PREF_DEFAULTS);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    Promise.all([api.getSetting("dialog_start_details"), api.getSetting("dialog_complete")])
      .then(([startDetails, complete]) => {
        setPrefs({
          showStartDetails: startDetails === null ? DIALOG_PREF_DEFAULTS.showStartDetails : startDetails === "true",
          showCompleteDialog: complete === null ? DIALOG_PREF_DEFAULTS.showCompleteDialog : complete === "true",
        });
      })
      .finally(() => setLoaded(true));
  }, []);

  async function setPref(key: keyof DialogPrefs, value: boolean) {
    setPrefs((p) => ({ ...p, [key]: value }));
    await api.setSetting(key === "showStartDetails" ? "dialog_start_details" : "dialog_complete", value ? "true" : "false");
  }

  return { prefs, setPref, loaded };
}

function DialogSettings({ prefs, setPref, loaded }: { prefs: DialogPrefs; setPref: (key: keyof DialogPrefs, value: boolean) => void; loaded: boolean }) {
  if (!loaded) return null;
  const rows: Array<{ key: keyof DialogPrefs; label: string; hint: string }> = [
    { key: "showStartDetails", label: "Show details before starting", hint: "Preview category & save folder in the Add Download box" },
    { key: "showCompleteDialog", label: "Show \"Download complete\" popup", hint: "Pop up a dialog with Open / Open Folder when a download finishes" },
  ];

  return (
    <div className="category">
      {rows.map((r) => (
        <label key={r.key} className="modal__checkbox" style={{ justifyContent: "space-between" }}>
          <span>
            <strong>{r.label}</strong>
            <div className="category__folder">{r.hint}</div>
          </span>
          <input type="checkbox" checked={prefs[r.key]} onChange={(e) => setPref(r.key, e.currentTarget.checked)} />
        </label>
      ))}
    </div>
  );
}

function formatAdded(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  const isToday = d.toDateString() === today.toDateString();
  const time = d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  return isToday ? `Today ${time}` : `${d.toLocaleDateString()} ${time}`;
}

function formatEta(task: TaskWithSpeed): string | null {
  if (task.status !== "Downloading" || !task.total_bytes || task.bytesPerSec <= 0) return null;
  const remaining = task.total_bytes - task.downloaded_bytes;
  if (remaining <= 0) return null;
  const totalSecs = Math.round(remaining / task.bytesPerSec);
  const m = Math.floor(totalSecs / 60);
  const s = totalSecs % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

function percentOf(task: Task): number | null {
  if (!task.total_bytes) return task.status === "Completed" ? 100 : null;
  return Math.min(100, Math.round((task.downloaded_bytes / task.total_bytes) * 100));
}

function filenameOf(task: Task): string {
  return task.dest_path.split(/[\\/]/).pop() || task.dest_path;
}

/** Prefers the upfront-fetched video title over the destination-path-derived
 * name -- for a known video site, `dest_path` is just a directory (see
 * `TaskManager::add_download`) until the download actually finishes, so
 * `filenameOf` alone would show the generic folder name the whole time. */
function displayNameOf(task: Task): string {
  return task.title || filenameOf(task);
}

// Small distinct color per category so the table's thumbnail chip reads at a
// glance -- mirrors the reference mockup's colored file-type squares.
const CATEGORY_THUMB: Record<string, { bg: string; icon: string }> = {
  video: { bg: "#0f172a", icon: "🎬" },
  music: { bg: "#2563eb", icon: "🎵" },
  compressed: { bg: "#7c3aed", icon: "🗜" },
  documents: { bg: "#0d9488", icon: "📄" },
  programs: { bg: "#475569", icon: "💽" },
};

function thumbFor(category: string | null) {
  return CATEGORY_THUMB[(category || "").toLowerCase()] || { bg: "#64748b", icon: "📦" };
}

type PopupType = "started" | "complete" | "failed";

const POPUP_SIZE: Record<PopupType, { width: number; height: number }> = {
  started: { width: 380, height: 112 },
  failed: { width: 380, height: 110 },
  complete: { width: 420, height: 410 },
};

// Vertical stacking slots so several popups spawned close together (a batch
// of downloads finishing around the same time) don't land on top of each
// other -- freed again once the corresponding window closes.
const usedPopupSlots = new Set<number>();
function acquirePopupSlot(): number {
  let i = 0;
  while (usedPopupSlots.has(i)) i++;
  usedPopupSlots.add(i);
  return i;
}

/** Spawns a small always-on-top, taskbar-less window anchored to the
 * bottom-right of the screen -- our own desktop-level notification, styled
 * with ODM's own design system, since Windows only shows properly-branded
 * native toasts for an installed app with a registered AUMID (an
 * unpackaged `tauri dev` build doesn't have one, so OS toasts silently
 * disappear there). Best-effort: never throws into the caller. */
async function spawnPopup(type: PopupType, fields: Record<string, string>) {
  try {
    const monitor = await currentMonitor();
    const scale = monitor?.scaleFactor || 1;
    const screenW = (monitor?.size.width || 1920) / scale;
    const screenH = (monitor?.size.height || 1080) / scale;
    const { width, height } = POPUP_SIZE[type];
    const margin = 20;
    const slot = acquirePopupSlot();
    const x = Math.round(screenW - width - margin);
    const y = Math.round(screenH - height - margin - 48 - slot * (height + 12));
    const label = `popup-${type}-${Date.now()}-${Math.floor(Math.random() * 10000)}`;
    const search = new URLSearchParams({ popup: type, ...fields }).toString();

    const win = new WebviewWindow(label, {
      url: `index.html?${search}`,
      title: "ODM",
      width,
      height,
      x,
      y,
      decorations: false,
      alwaysOnTop: true,
      skipTaskbar: true,
      resizable: false,
      // Windows/WebView2 doesn't reliably route mouse input to an
      // unfocused window -- clicks (even a plain checkbox toggle) silently
      // did nothing when this was `focus: false`. Letting it take focus on
      // creation costs a brief focus steal but keeps the popup interactive.
      shadow: true,
      visible: true,
    });
    win.once("tauri://destroyed", () => usedPopupSlots.delete(slot));
    win.once("tauri://error", () => usedPopupSlots.delete(slot));
  } catch {
    // best effort -- a failed popup should never block the download flow
  }
}

// One consistent outline icon set (Feather-style paths) for the sidebar --
// avoids mixing emoji/glyphs of different weights per the design system's
// icon guidance.
const ICON_PATHS: Record<string, ReactElement> = {
  download: (
    <>
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="7 10 12 15 17 10" />
      <line x1="12" y1="15" x2="12" y2="3" />
    </>
  ),
  "arrow-down": (
    <>
      <line x1="12" y1="5" x2="12" y2="19" />
      <polyline points="19 12 12 19 5 12" />
    </>
  ),
  "check-circle": (
    <>
      <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14" />
      <polyline points="22 4 12 14.01 9 11.01" />
    </>
  ),
  pause: (
    <>
      <rect x="6" y="4" width="4" height="16" rx="1" />
      <rect x="14" y="4" width="4" height="16" rx="1" />
    </>
  ),
  "alert-triangle": (
    <>
      <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0Z" />
      <line x1="12" y1="9" x2="12" y2="13" />
      <line x1="12" y1="17" x2="12.01" y2="17" />
    </>
  ),
  film: (
    <>
      <rect x="2" y="2" width="20" height="20" rx="2.18" ry="2.18" />
      <line x1="7" y1="2" x2="7" y2="22" />
      <line x1="17" y1="2" x2="17" y2="22" />
      <line x1="2" y1="12" x2="22" y2="12" />
      <line x1="2" y1="7" x2="7" y2="7" />
      <line x1="2" y1="17" x2="7" y2="17" />
      <line x1="17" y1="17" x2="22" y2="17" />
      <line x1="17" y1="7" x2="22" y2="7" />
    </>
  ),
  music: (
    <>
      <path d="M9 18V5l12-2v13" />
      <circle cx="6" cy="18" r="3" />
      <circle cx="18" cy="16" r="3" />
    </>
  ),
  "file-text": (
    <>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z" />
      <polyline points="14 2 14 8 20 8" />
      <line x1="16" y1="13" x2="8" y2="13" />
      <line x1="16" y1="17" x2="8" y2="17" />
    </>
  ),
  archive: (
    <>
      <polyline points="21 8 21 21 3 21 3 8" />
      <rect x="1" y="3" width="22" height="5" />
      <line x1="10" y1="12" x2="14" y2="12" />
    </>
  ),
  package: (
    <>
      <line x1="16.5" y1="9.4" x2="7.5" y2="4.21" />
      <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z" />
      <polyline points="3.27 6.96 12 12.01 20.73 6.96" />
      <line x1="12" y1="22.08" x2="12" y2="12" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="10" />
      <line x1="12" y1="16" x2="12" y2="12" />
      <line x1="12" y1="8" x2="12.01" y2="8" />
    </>
  ),
  sun: (
    <>
      <circle cx="12" cy="12" r="5" />
      <line x1="12" y1="1" x2="12" y2="3" />
      <line x1="12" y1="21" x2="12" y2="23" />
      <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
      <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
      <line x1="1" y1="12" x2="3" y2="12" />
      <line x1="21" y1="12" x2="23" y2="12" />
      <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
      <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
    </>
  ),
  moon: (
    <>
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79Z" />
    </>
  ),
};

function Icon({ name }: { name: keyof typeof ICON_PATHS }) {
  return (
    <svg className="sidebar__icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      {ICON_PATHS[name]}
    </svg>
  );
}

const STATUS_TAB_ICON: Record<TaskStatus | "All", keyof typeof ICON_PATHS> = {
  All: "download",
  Downloading: "arrow-down",
  Queued: "arrow-down",
  Completed: "check-circle",
  Paused: "pause",
  Failed: "alert-triangle",
  Cancelled: "alert-triangle",
};

const CATEGORY_ICON: Record<string, keyof typeof ICON_PATHS> = {
  video: "film",
  music: "music",
  compressed: "archive",
  documents: "file-text",
  programs: "package",
};

function categoryIcon(name: string): keyof typeof ICON_PATHS {
  return CATEGORY_ICON[name.toLowerCase()] || "file-text";
}

const STATUS_TABS: Array<{ key: TaskStatus | "All"; label: string }> = [
  { key: "All", label: "All" },
  { key: "Downloading", label: "Downloading" },
  { key: "Completed", label: "Completed" },
  { key: "Paused", label: "Paused" },
  { key: "Failed", label: "Failed" },
];

type SortKey = "added-desc" | "added-asc" | "name-asc" | "size-desc";

const SORT_OPTIONS: Array<{ key: SortKey; label: string }> = [
  { key: "added-desc", label: "Added (Newest)" },
  { key: "added-asc", label: "Added (Oldest)" },
  { key: "name-asc", label: "Name (A–Z)" },
  { key: "size-desc", label: "Size (Largest)" },
];

function sortTasks(tasks: TaskWithSpeed[], key: SortKey): TaskWithSpeed[] {
  const copy = [...tasks];
  switch (key) {
    case "added-asc":
      return copy.sort((a, b) => a.created_at.localeCompare(b.created_at));
    case "name-asc":
      return copy.sort((a, b) => displayNameOf(a).localeCompare(displayNameOf(b)));
    case "size-desc":
      return copy.sort((a, b) => (b.total_bytes || 0) - (a.total_bytes || 0));
    case "added-desc":
    default:
      return copy.sort((a, b) => b.created_at.localeCompare(a.created_at));
  }
}

function StatusPill({ task }: { task: TaskWithSpeed }) {
  const pct = percentOf(task);
  if (task.status === "Failed") {
    return (
      <span className="status-badge status-badge--failed" title={task.error_message || undefined}>
        ⚠ Failed
      </span>
    );
  }
  if (task.status === "Queued") {
    return <span className="status-badge status-badge--queued">Queued</span>;
  }
  if (task.status === "Cancelled") {
    return <span className="status-badge status-badge--queued">Cancelled</span>;
  }
  if (task.status === "Completed") {
    return <span className="status-badge status-badge--completed">✓ Completed</span>;
  }
  const indeterminate = task.status === "Downloading" && pct === null;
  return (
    <div className="status-progress">
      <div className="status-progress__row">
        <div className="progress">
          {indeterminate ? (
            <div className="progress__bar progress__bar--indeterminate" />
          ) : (
            <div className={`progress__bar progress__bar--${task.status.toLowerCase()}`} style={{ width: `${pct ?? 0}%` }} />
          )}
        </div>
        <span className="status-progress__pct">{pct === null ? (indeterminate ? "…" : "—") : `${pct}%`}</span>
      </div>
      <span className={task.status === "Paused" ? "status-progress__caption status-progress__caption--paused" : "status-progress__caption"}>
        {task.status}
      </span>
    </div>
  );
}

interface MenuAction {
  label: string;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
}

type MenuEntry = MenuAction | "separator";

export interface RowMenuCallbacks {
  onAction: (id: number, action: "pause" | "resume" | "cancel") => void;
  onRename: (task: TaskWithSpeed) => void;
  onRedownload: (task: TaskWithSpeed) => void;
  onDelete: (task: TaskWithSpeed) => void;
  onProperties: (task: TaskWithSpeed) => void;
}

// Shared action list for both the "⋮" row menu and the right-click context
// menu, grouped to mirror a familiar download-manager menu. Items only
// appear where the backend actually supports them -- no "Add to queue",
// "Refresh download address", or "On Double click" entries, since none of
// those have any backend concept behind them yet.
function buildRowMenuItems(task: TaskWithSpeed, cb: RowMenuCallbacks): MenuEntry[] {
  const isCompleted = task.status === "Completed";
  const isDownloading = task.status === "Downloading";
  const isPaused = task.status === "Paused";

  return [
    { label: "Open", onClick: () => void openPath(task.dest_path).catch(() => {}), disabled: !isCompleted },
    { label: "Open with…", onClick: () => void api.openWithDialog(task.dest_path).catch(() => {}), disabled: !isCompleted },
    { label: "Open folder", onClick: () => void revealItemInDir(task.dest_path).catch(() => {}) },
    "separator",
    { label: "Move/Rename", onClick: () => cb.onRename(task), disabled: !isCompleted },
    "separator",
    { label: "Redownload", onClick: () => cb.onRedownload(task), disabled: isDownloading },
    "separator",
    { label: "Resume Download", onClick: () => cb.onAction(task.id, "resume"), disabled: !isPaused },
    { label: "Stop Download", onClick: () => cb.onAction(task.id, "pause"), disabled: !isDownloading },
    "separator",
    { label: "Remove", onClick: () => cb.onDelete(task), danger: true },
    "separator",
    { label: "Properties", onClick: () => cb.onProperties(task) },
  ];
}

function MenuItems({ entries, onPick }: { entries: MenuEntry[]; onPick: (fn: () => void) => void }) {
  return (
    <>
      {entries.map((entry, i) =>
        entry === "separator" ? (
          <div key={`sep-${i}`} className="row-menu__separator" />
        ) : (
          <button
            key={entry.label}
            className={entry.danger ? "row-menu__danger" : undefined}
            disabled={entry.disabled}
            onClick={() => onPick(entry.onClick)}
          >
            {entry.label}
          </button>
        )
      )}
    </>
  );
}

function RowMenu({ task, menu }: { task: TaskWithSpeed; menu: RowMenuCallbacks }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function onDocClick(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, [open]);

  return (
    <div className="row-menu" ref={ref} onClick={(e) => e.stopPropagation()}>
      <button className="row-icon-btn" title="More" onClick={() => setOpen((o) => !o)}>
        ⋮
      </button>
      {open && (
        <div className="row-menu__popover">
          <MenuItems
            entries={buildRowMenuItems(task, menu)}
            onPick={(fn) => {
              setOpen(false);
              fn();
            }}
          />
        </div>
      )}
    </div>
  );
}

function ContextMenu({
  x,
  y,
  task,
  menu,
  onClose,
}: {
  x: number;
  y: number;
  task: TaskWithSpeed;
  menu: RowMenuCallbacks;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const entries = buildRowMenuItems(task, menu);

  useEffect(() => {
    function onDocDown(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    document.addEventListener("mousedown", onDocDown);
    document.addEventListener("keydown", onKey);
    window.addEventListener("scroll", onClose, true);
    return () => {
      document.removeEventListener("mousedown", onDocDown);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("scroll", onClose, true);
    };
  }, [onClose]);

  // Clamp so the menu never renders off the right/bottom edge of the window.
  const rows = entries.filter((e) => e !== "separator").length + entries.filter((e) => e === "separator").length * 0.4;
  const left = Math.min(x, window.innerWidth - 200);
  const top = Math.min(y, window.innerHeight - 8 - rows * 32);

  return (
    <div ref={ref} className="row-menu__popover context-menu" style={{ left, top }}>
      <MenuItems
        entries={entries}
        onPick={(fn) => {
          onClose();
          fn();
        }}
      />
    </div>
  );
}

function DownloadTableRow({
  task,
  selected,
  onSelect,
  menu,
  onContextMenu,
}: {
  task: TaskWithSpeed;
  selected: boolean;
  onSelect: () => void;
  menu: RowMenuCallbacks;
  onContextMenu: (e: React.MouseEvent, task: TaskWithSpeed) => void;
}) {
  const isDownloading = task.status === "Downloading";
  const isPaused = task.status === "Paused";
  const thumb = thumbFor(task.category);

  return (
    <tr className={`${selected ? "row--selected" : ""} ${task.playlist_group && task.status !== "Completed" ? "playlist-pending" : ""}`} onClick={onSelect} onContextMenu={(e) => onContextMenu(e, task)}>
      <td className="col-name">
        {task.thumbnail_url ? (
          <img className="thumb thumb--image" src={task.thumbnail_url} alt="" />
        ) : (
          <span className="thumb" style={{ background: thumb.bg }}>
            {thumb.icon}
          </span>
        )}
        <div className="col-name__text">
          <div className="row-name" title={task.url}>
            {displayNameOf(task)}
          </div>
          {qualityFallbackMessage(task) && <div className="quality-fallback">{qualityFallbackMessage(task)}</div>}
        </div>
      </td>
      <td
        className="col-quality"
        title={task.actual_video_quality && task.video_quality && task.actual_video_quality !== task.video_quality ? `${task.video_quality}p preferred; ${task.actual_video_quality}p selected` : undefined}
      >
        {isKnownVideoUrl(task.url) ? taskQualityLabel(task) : "—"}
      </td>
      <td className="col-size">{task.total_bytes ? formatBytes(task.total_bytes) : "—"}</td>
      <td className="col-status">
        <StatusPill task={task} />
      </td>
      <td className="col-speed">{formatSpeed(task.bytesPerSec) || "—"}</td>
      <td className="col-added">{formatAdded(task.created_at)}</td>
      <td className="col-actions" onClick={(e) => e.stopPropagation()}>
        {isDownloading && (
          <button className="row-icon-btn" title="Pause" onClick={() => menu.onAction(task.id, "pause")}>
            ⏸
          </button>
        )}
        {isPaused && (
          <button className="row-icon-btn" title="Resume" onClick={() => menu.onAction(task.id, "resume")}>
            ▶
          </button>
        )}
        {task.status === "Completed" && (
          <button
            className="row-icon-btn"
            title="Open Folder"
            onClick={() => {
              revealItemInDir(task.dest_path).catch(() => {});
            }}
          >
            ▶
          </button>
        )}
        <RowMenu task={task} menu={menu} />
      </td>
    </tr>
  );
}

function RightDetailsPanel({
  task,
  onClose,
  onAction,
}: {
  task: TaskWithSpeed;
  onClose: () => void;
  onAction: (id: number, action: "pause" | "resume" | "cancel") => void;
}) {
  const [tab, setTab] = useState<"details" | "progress">("details");
  const rawPct = percentOf(task);
  const indeterminate = task.status === "Downloading" && rawPct === null;
  const pct = rawPct ?? 0;
  const thumb = thumbFor(task.category);
  const cancellable = task.status === "Downloading" || task.status === "Paused" || task.status === "Queued";

  return (
    <aside className="details-side">
      <div className="details-side__preview" style={task.thumbnail_url ? undefined : { background: thumb.bg }}>
        {task.thumbnail_url ? (
          <img className="details-side__preview-image" src={task.thumbnail_url} alt="" />
        ) : (
          <span className="details-side__preview-icon">{thumb.icon}</span>
        )}
        <button className="details-side__close" onClick={onClose} title="Close">
          ×
        </button>
      </div>

      <div className="details-panel__tabs">
        <button className={tab === "details" ? "tab tab--active" : "tab"} onClick={() => setTab("details")}>
          Details
        </button>
        <button className={tab === "progress" ? "tab tab--active" : "tab"} onClick={() => setTab("progress")}>
          Progress
        </button>
      </div>

      {tab === "details" ? (
        <div className="details-side__body">
          <div className="details-grid">
            <span>Name:</span>
            <span className="details-grid__wrap">{displayNameOf(task)}</span>
            <span>URL:</span>
            <span className="details-grid__wrap">{task.url}</span>
            <span>Size:</span>
            <span>{task.total_bytes ? formatBytes(task.total_bytes) : "—"}</span>
            {isKnownVideoUrl(task.url) && (
              <>
                <span>Downloaded quality:</span>
                <span>{task.actual_video_quality ? qualityLabel(task.actual_video_quality) : taskQualityLabel(task)}</span>
                {task.video_quality && (
                  <>
                    <span>Requested quality:</span>
                    <span>{qualityLabel(task.video_quality)}</span>
                  </>
                )}
                {qualityFallbackMessage(task) && (
                  <>
                    <span>Fallback:</span>
                    <span className="quality-fallback">{qualityFallbackMessage(task)}</span>
                  </>
                )}
              </>
            )}
            <span>Downloaded:</span>
            <span>
              {formatBytes(task.downloaded_bytes)}
              {task.total_bytes ? ` (${pct}%)` : ""}
            </span>
            <span>Speed:</span>
            <span>{formatSpeed(task.bytesPerSec) || "—"}</span>
            {formatEta(task) && (
              <>
                <span>ETA:</span>
                <span>{formatEta(task)}</span>
              </>
            )}
            <span>Added:</span>
            <span>{formatAdded(task.created_at)}</span>
            <span>Save to:</span>
            <span className="details-grid__wrap">{task.dest_path}</span>
            {task.error_message && (
              <>
                <span>Error:</span>
                <span className="status-text--error">{task.error_message}</span>
              </>
            )}
          </div>
        </div>
      ) : (
        <div className="details-side__body details-side__body--centered">
          <div className="progress progress--wide">
            {indeterminate ? (
              <div className="progress__bar progress__bar--indeterminate" />
            ) : (
              <div className={`progress__bar progress__bar--${task.status.toLowerCase()}`} style={{ width: `${pct}%` }} />
            )}
          </div>
          <div className="details-progress-big">{indeterminate ? "…" : `${pct}%`}</div>
          <div className="muted">{indeterminate ? "Downloading (no byte-level progress reported)" : formatSpeed(task.bytesPerSec) || "Not transferring"}</div>
        </div>
      )}

      <div className="details-side__footer">
        <button
          type="button"
          className="btn"
          onClick={() => {
            openPath(task.dest_path).catch(() => {});
          }}
        >
          Open File
        </button>
        <button
          type="button"
          className="btn"
          onClick={() => {
            revealItemInDir(task.dest_path).catch(() => {});
          }}
        >
          Show in Folder
        </button>
        {cancellable && (
          <button type="button" className="btn btn--danger-outline" onClick={() => onAction(task.id, "cancel")}>
            Cancel
          </button>
        )}
      </div>
    </aside>
  );
}

// Mirrors odm-engine's KNOWN_VIDEO_HOSTS -- UI hint only (which known-site
// options to show); the backend is the authority on actual routing.
const KNOWN_VIDEO_HOSTS = ["youtube.com", "youtu.be", "tiktok.com", "instagram.com", "facebook.com", "twitter.com", "x.com", "vimeo.com", "dailymotion.com", "twitch.tv", "soundcloud.com", "reddit.com"];

function isKnownVideoUrl(url: string): boolean {
  try {
    const host = new URL(url).hostname;
    return KNOWN_VIDEO_HOSTS.some((known) => host === known || host.endsWith(`.${known}`));
  } catch {
    return false;
  }
}

/** Best-effort client-side guess of which category a URL will land in --
 * purely a preview for the "before you start" details box; the backend
 * (odm-engine) is the actual authority on file-extension -> category
 * routing once the download runs. */
function guessCategory(url: string, categories: Category[]): Category | null {
  try {
    const ext = new URL(url).pathname.split(".").pop()?.toLowerCase();
    if (ext) {
      const match = categories.find((c) => c.extensions.some((e) => e.toLowerCase().replace(/^\./, "") === ext));
      if (match) return match;
    }
  } catch {
    // not a parseable URL yet -- no guess
  }
  return null;
}

function AddDownloadModal({
  initialUrl,
  categories,
  showDetails,
  onToggleShowDetails,
  onClose,
  onAdded,
}: {
  initialUrl?: string;
  categories: Category[];
  showDetails: boolean;
  onToggleShowDetails: (show: boolean) => void;
  onClose: () => void;
  onAdded: () => void;
}) {
  const [url, setUrl] = useState(initialUrl || "");
  const [filename, setFilename] = useState("");
  const [playlist, setPlaylist] = useState(false);
  const [quality, setQuality] = useState("default");
  const [categoryName, setCategoryName] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const urls = url.split(/\r?\n/).map((value) => value.trim()).filter(Boolean);
  const primaryUrl = urls[0] || "";
  const isVideoSite = isKnownVideoUrl(primaryUrl);
  const looksLikePlaylist = /(?:[?&]list=|\/playlist(?:\/|$))/i.test(primaryUrl);
  const guessed = useMemo(() => guessCategory(url, categories), [url, categories]);
  const selectedCategory = categories.find((c) => c.name === categoryName) || guessed;

  useEffect(() => {
    if (guessed && !categoryName) setCategoryName(guessed.name);
  }, [guessed, categoryName]);

  useEffect(() => {
    // Pasting a YouTube playlist link opts into the explicit playlist mode;
    // users can still turn it off before submitting.
    if (looksLikePlaylist) setPlaylist(true);
  }, [looksLikePlaylist]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!url.trim() || busy) return;
    setBusy(true);
    setError(null);
    let added = 0;
    try {
      // Submit each pasted URL in order so a bulk paste becomes a queue of
      // independent tasks. A playlist URL remains one yt-dlp task that
      // processes its entries sequentially.
      for (const item of urls) {
        await api.addDownload(item, filename.trim() || undefined, playlist && isKnownVideoUrl(item), quality);
        added++;
      }
      onAdded();
      onClose();
    } catch (err) {
      // Retry only the failed and unsubmitted URLs, never the entries that
      // have already been accepted into the queue.
      setUrl(urls.slice(added).join("\n"));
      if (added > 0) onAdded();
      setError(`${added > 0 ? `${added} link(s) added. Remaining links kept for retry. ` : ""}${String(err)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <form className="modal" onClick={(e) => e.stopPropagation()} onSubmit={submit}>
        <div className="modal__header">
          <h2>Add Download</h2>
          <button type="button" className="modal__close" onClick={onClose}>
            ×
          </button>
        </div>

        <label className="modal__label">Enter URL</label>
        <textarea className="modal__url-list" autoFocus required rows={3} placeholder="Paste a link — or many, one per line. YouTube, Facebook, Instagram, TikTok." value={url} onChange={(e) => setUrl(e.currentTarget.value)} />

        {isVideoSite ? (
          <>
            <label className="modal__label">Video quality</label>
            <select className="toolbar__select" value={quality} onChange={(e) => setQuality(e.currentTarget.value)}>
              <option value="default">Use default from Settings</option>
              {VIDEO_QUALITY_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
            <div className="category__folder">Exact resolution is preferred; ODM falls back automatically if it is unavailable.</div>
          </>
        ) : (
          <>
            <label className="modal__label">File name (optional)</label>
            <input type="text" placeholder="Filename will be detected automatically" value={filename} onChange={(e) => setFilename(e.currentTarget.value)} />
          </>
        )}

        <label className="modal__checkbox" title={isVideoSite ? undefined : "Paste a YouTube playlist URL to enable this option"}>
          <input
            type="checkbox"
            checked={playlist}
            disabled={!isVideoSite}
            onChange={(e) => setPlaylist(e.currentTarget.checked)}
          />
          Download entire YouTube playlist
        </label>
        {isVideoSite && <div className="category__folder">We support YouTube playlist downloads. Paste a playlist URL to queue the complete playlist.</div>}
        {playlist && isVideoSite && <div className="category__folder">Each video is queued at the selected quality. ODM adds a short delay between requests to reduce burst traffic.</div>}
        {playlist && isVideoSite && <><label className="modal__label">Playlist folder name (optional)</label><input value={filename} onChange={e => setFilename(e.currentTarget.value)} placeholder="Use playlist title" /></>}
        {urls.length > 1 && <div className="category__folder">{urls.length} links will be added to the queue in order.</div>}

        {showDetails && (
          <div className="download-details">
            {!isVideoSite && (
              <>
                <label className="modal__label">Category</label>
                <select className="toolbar__select" value={categoryName} onChange={(e) => setCategoryName(e.currentTarget.value)}>
                  <option value="">Auto-detect</option>
                  {categories.map((c) => (
                    <option key={c.name} value={c.name}>
                      {c.name}
                    </option>
                  ))}
                </select>
              </>
            )}
            <div className="download-details__row">
              <span className="download-details__label">Save to:</span>
              <span className="download-details__value">
                {isVideoSite ? "Video" : selectedCategory ? `${selectedCategory.default_folder}/` : "Detected automatically"}
                {filename ? `${filename}` : ""}
              </span>
            </div>
          </div>
        )}

        <label className="modal__checkbox modal__checkbox--muted">
          <input type="checkbox" checked={!showDetails} onChange={(e) => onToggleShowDetails(!e.currentTarget.checked)} />
          Don't show download details before starting
        </label>

        {error && <div className="form-error">{error}</div>}

        <div className="modal__footer">
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="btn btn--primary" disabled={busy}>
            {busy ? (playlist ? "Fetching playlist videos…" : "Adding links…") : "Start Download"}
          </button>
        </div>
      </form>
    </div>
  );
}

function AboutModal({ onClose, version }: { onClose: () => void; version: string }) {
  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal modal--about" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h2>About ODM</h2>
          <button type="button" className="modal__close" onClick={onClose}>
            ×
          </button>
        </div>
        <div className="about__body">
          <img src={logo} alt="ODM" className="about__logo" />
          <div className="about__name">ODM — Open Download Manager</div>
          <div className="muted">{version ? `Version ${version}` : "Version —"}</div>
          <p className="muted about__desc">A fast, modern download manager for Windows — direct links, and video/audio via yt-dlp.</p>

          <div className="about__divider" />

          <div className="about__author">
            <div className="about__author-name">Muzammil Ijaz</div>
            <button type="button" className="link-btn" onClick={() => openUrl("https://github.com/muzammilijaz")}>
              github.com/muzammilijaz
            </button>
          </div>

          <button type="button" className="about__coffee" onClick={() => openUrl("https://muzammilijaz.gumroad.com/coffee")}>
            ☕ Support this project — Buy me a coffee
          </button>
          <button type="button" className="about__extension" onClick={() => openUrl(CHROME_EXTENSION_URL)}>
            Add the official Chrome extension
          </button>
        </div>
        <div className="modal__footer">
          <button type="button" className="btn btn--primary" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

function DeleteConfirmModal({
  count,
  playlistTitle,
  onCancel,
  onConfirm,
}: {
  count: number;
  playlistTitle?: string;
  onCancel: () => void;
  onConfirm: (deleteFile: boolean) => void;
}) {
  const [deleteFile, setDeleteFile] = useState(false);
  const plural = count > 1;

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal modal--delete" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h2>{playlistTitle ? `Delete playlist “${playlistTitle}”?` : `Delete ${plural ? `${count} downloads` : "download"}?`}</h2>
          <button type="button" className="modal__close" onClick={onCancel}>
            ×
          </button>
        </div>
        <p className="muted">This removes {plural ? "these downloads" : "this download"} from ODM's list.</p>
        {playlistTitle && <p className="muted">All {count} playlist entries will be removed, including hidden entries. Files stay on your PC unless you select the option below.</p>}
        <label className="modal__checkbox">
          <input type="checkbox" checked={deleteFile} onChange={(e) => setDeleteFile(e.currentTarget.checked)} />
          Also delete the file{plural ? "s" : ""} from my PC
        </label>
        {playlistTitle && deleteFile && <p className="muted">Downloaded files are permanently deleted. The playlist folder is removed when empty; any other files in it are kept.</p>}
        <div className="modal__footer">
          <button type="button" className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="btn btn--danger" onClick={() => onConfirm(deleteFile)}>
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}

// Mirrors yt-dlp's own --cookies-from-browser support list (confirmed via
// its --help; "safari" omitted since this is a Windows-only build for now).
const BROWSER_OPTIONS = [
  { label: "Off", value: "" },
  { label: "Chrome", value: "chrome" },
  { label: "Edge", value: "edge" },
  { label: "Firefox", value: "firefox" },
  { label: "Brave", value: "brave" },
  { label: "Opera", value: "opera" },
  { label: "Vivaldi", value: "vivaldi" },
  { label: "Chromium", value: "chromium" },
  { label: "Whale", value: "whale" },
];

function VideoQualitySetting() {
  const [quality, setQuality] = useState("best");
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    api
      .getSetting("video_quality")
      .then((value) => setQuality(value || "best"))
      .catch((error) => setError(`Could not load video quality: ${String(error)}`))
      .finally(() => setLoaded(true));
  }, []);

  if (!loaded) return null;

  return (
    <div className="category">
      <div className="category__header">
        <strong>Default video quality</strong>
        <select
          className="toolbar__select"
          value={quality}
          disabled={saving}
          onChange={async (event) => {
            const value = event.currentTarget.value;
            setSaving(true);
            try {
              await api.setSetting("video_quality", value);
              setQuality(value);
              setError("");
            } catch (error) {
              setError(`Could not save video quality: ${String(error)}`);
            } finally { setSaving(false); }
          }}
        >
          {VIDEO_QUALITY_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>
      <div className="category__folder">
        ODM first tries this resolution. If it is unavailable, it uses the nearest lower resolution, then the best format the site provides.
      </div>
      {error && <div className="form-error">{error}</div>}
    </div>
  );
}

function PlaylistQueueSetting() {
  const [value, setValue] = useState("1");
  const [error, setError] = useState("");
  useEffect(() => { api.getSetting("playlist_concurrent").then(v => setValue(v || "1")).catch(e => setError(String(e))); }, []);
  return <div className="category"><div className="category__header">
    <strong>Simultaneous playlist downloads</strong>
    <select className="toolbar__select" value={value} onChange={async e => {
      const next = e.currentTarget.value;
      try { await api.setSetting("playlist_concurrent", next); setValue(next); setError(""); } catch(e) { setError(String(e)); }
    }}>{[1,2,3,4].map(n => <option key={n} value={String(n)}>{n === 1 ? "1 — one at a time" : n}</option>)}</select>
  </div><div className="category__folder">Applies to newly added playlists. Downloads include a short delay between requests.</div>{error && <div className="form-error">{error}</div>}</div>;
}

function CookiesSetting() {
  const [browser, setBrowser] = useState("");
  const [cookiesFile, setCookiesFile] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [importing, setImporting] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([api.getSetting("cookies_browser"), api.getSetting("cookies_file")])
      .then(([b, f]) => {
        setBrowser(b || "");
        setCookiesFile(f || null);
      })
      .finally(() => setLoaded(true));
  }, []);

  async function onBrowserChange(value: string) {
    setBrowser(value);
    await api.setSetting("cookies_browser", value);
  }

  async function pickCookiesFile() {
    setImportError(null);
    try {
      const picked = await openFileDialog({
        multiple: false,
        filters: [{ name: "Cookies", extensions: ["txt"] }],
      });
      if (!picked || Array.isArray(picked)) return;
      setImporting(true);
      const stored = await api.importCookiesFile(picked);
      setCookiesFile(stored);
    } catch (err) {
      setImportError(String(err));
    } finally {
      setImporting(false);
    }
  }

  async function clearCookiesFile() {
    await api.clearCookiesFile();
    setCookiesFile(null);
  }

  if (!loaded) return null;

  return (
    <div className="category">
      <div className="category__header">
        <strong>Private / member-only videos</strong>
      </div>

      <label className="modal__checkbox">
        Browser cookies:
        <select className="toolbar__select" value={browser} onChange={(e) => onBrowserChange(e.currentTarget.value)} disabled={!!cookiesFile}>
          {BROWSER_OPTIONS.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </label>
      <div className="category__folder" style={{ marginTop: 4 }}>
        Sign-in cookies are read from this browser, so private, age-restricted and member-only videos can be downloaded. Nothing leaves your machine.
        <br />
        Close that browser before downloading — while it's running it locks its own cookie file.
      </div>

      <div className="category__header" style={{ marginTop: 14 }}>
        <strong>Your own cookies file</strong>
      </div>
      {cookiesFile ? (
        <div className="modal__checkbox">
          <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={cookiesFile}>
            Using: {cookiesFile}
          </span>
          <button type="button" className="btn" onClick={clearCookiesFile}>
            Remove
          </button>
        </div>
      ) : (
        <button type="button" className="btn" onClick={pickCookiesFile} disabled={importing}>
          {importing ? "Importing…" : "Choose file…"}
        </button>
      )}
      <div className="category__folder" style={{ marginTop: 4 }}>
        Export a cookies.txt from any browser (a "Get cookies.txt" extension does this) and load it here — useful when the account isn't signed into a
        browser on this machine, or when reading the browser directly doesn't work. A copy is kept inside ODM, so you can move or delete the original
        afterward. Takes priority over the browser option above.
      </div>
      {importError && <div className="form-error">{importError}</div>}
    </div>
  );
}

function CategoriesPanel({
  categories,
  onChange,
  notifyPrefs,
  onSetNotifyPref,
  notifyPrefsLoaded,
  dialogPrefs,
  onSetDialogPref,
  dialogPrefsLoaded,
}: {
  categories: Category[];
  onChange: () => void;
  notifyPrefs: NotifyPrefs;
  onSetNotifyPref: (kind: NotifyKind, value: boolean) => void;
  notifyPrefsLoaded: boolean;
  dialogPrefs: DialogPrefs;
  onSetDialogPref: (key: keyof DialogPrefs, value: boolean) => void;
  dialogPrefsLoaded: boolean;
}) {
  const [newExt, setNewExt] = useState<Record<string, string>>({});

  return (
    <div className="categories">
      <section className="settings-section">
        <h2 className="settings-section__title">Download dialogs</h2>
        <DialogSettings prefs={dialogPrefs} setPref={onSetDialogPref} loaded={dialogPrefsLoaded} />
      </section>

      <section className="settings-section">
        <h2 className="settings-section__title">Notifications</h2>
        <NotificationSettings prefs={notifyPrefs} setPref={onSetNotifyPref} loaded={notifyPrefsLoaded} />
      </section>

      <section className="settings-section">
        <h2 className="settings-section__title">Video quality</h2>
        <VideoQualitySetting />
        <PlaylistQueueSetting />
      </section>

      <section className="settings-section">
        <h2 className="settings-section__title">Sign-in & cookies</h2>
        <CookiesSetting />
      </section>

      <section className="settings-section">
        <h2 className="settings-section__title">File types & folders</h2>
        {categories.map((cat) => (
          <div key={cat.name} className="category">
            <div className="category__header">
              <strong>{cat.name}</strong>
              <span className="category__folder">→ {cat.default_folder}/</span>
            </div>
            <div className="category__extensions">
              {cat.extensions.map((ext) => (
                <span key={ext} className="chip">
                  {ext}
                  <button
                    className="chip__remove"
                    onClick={async () => {
                      await api.removeCategoryExtension(cat.name, ext);
                      onChange();
                    }}
                  >
                    ×
                  </button>
                </span>
              ))}
              <form
                className="chip-form"
                onSubmit={async (e) => {
                  e.preventDefault();
                  const ext = (newExt[cat.name] || "").trim();
                  if (!ext) return;
                  await api.addCategoryExtension(cat.name, ext);
                  setNewExt((s) => ({ ...s, [cat.name]: "" }));
                  onChange();
                }}
              >
                <input
                  placeholder="+ ext"
                  value={newExt[cat.name] || ""}
                  onChange={(e) => setNewExt((s) => ({ ...s, [cat.name]: e.currentTarget.value }))}
                />
              </form>
            </div>
          </div>
        ))}
      </section>
    </div>
  );
}

interface ToastItem {
  id: string;
  kind: "success" | "error";
  title: string;
  destPath?: string;
}

function ToastStack({ toasts, onDismiss }: { toasts: ToastItem[]; onDismiss: (id: string) => void }) {
  if (toasts.length === 0) return null;
  return (
    <div className="toast-stack">
      {toasts.map((t) => (
        <div key={t.id} className={`toast toast--${t.kind}`}>
          <span className="toast__icon">{t.kind === "success" ? "✓" : "⚠"}</span>
          <span className="toast__text">{t.title}</span>
          {t.kind === "success" && t.destPath && (
            <button
              className="toast__action"
              onClick={() => {
                revealItemInDir(t.destPath!).catch(() => {});
              }}
            >
              Open Folder
            </button>
          )}
          <button className="toast__close" onClick={() => onDismiss(t.id)} title="Dismiss">
            ×
          </button>
        </div>
      ))}
    </div>
  );
}

function Sidebar({
  statusTab,
  onStatusTab,
  categoryFilter,
  onCategoryFilter,
  categoryCounts,
  statusCounts,
  showSettings,
  onToggleSettings,
  onShowAbout,
  version,
  theme,
  onToggleTheme,
}: {
  statusTab: TaskStatus | "All";
  onStatusTab: (s: TaskStatus | "All") => void;
  categoryFilter: string | null;
  onCategoryFilter: (c: string | null) => void;
  categoryCounts: Array<{ name: string; count: number }>;
  statusCounts: Record<string, number>;
  showSettings: boolean;
  onToggleSettings: () => void;
  onShowAbout: () => void;
  version: string;
  theme: Theme;
  onToggleTheme: () => void;
}) {
  return (
    <aside className="sidebar">
      <div className="sidebar__header">
        <img src={logo} alt="ODM" className="sidebar__logo" />
        <div>
          <div className="sidebar__brand">ODM</div>
          <div className="sidebar__tagline">
            Open Download Manager{version ? <span className="sidebar__version"> · v{version}</span> : null}
          </div>
        </div>
      </div>

      <nav className="sidebar__nav">
        {STATUS_TABS.map((t) => (
          <button
            key={t.key}
            className={`sidebar__item${!showSettings && statusTab === t.key ? " sidebar__item--active" : ""}`}
            onClick={() => {
              onStatusTab(t.key);
              if (showSettings) onToggleSettings();
            }}
          >
            <span className="sidebar__item-label">
              <Icon name={STATUS_TAB_ICON[t.key]} />
              {t.key === "All" ? "All Downloads" : t.label}
            </span>
            <span className="sidebar__count">{statusCounts[t.key] ?? 0}</span>
          </button>
        ))}

        <div className="sidebar__label">Categories</div>
        {categoryCounts.map((c) => (
          <button
            key={c.name}
            className={`sidebar__item${!showSettings && categoryFilter === c.name ? " sidebar__item--active" : ""}`}
            onClick={() => {
              onCategoryFilter(categoryFilter === c.name ? null : c.name);
              if (showSettings) onToggleSettings();
            }}
          >
            <span className="sidebar__item-label">
              <Icon name={categoryIcon(c.name)} />
              {c.name}
            </span>
            <span className="sidebar__count">{c.count}</span>
          </button>
        ))}
      </nav>

      <div className="sidebar__bottom">
        <button
          className="sidebar__theme-toggle"
          onClick={onToggleTheme}
          title={theme === "dark" ? "Switch to Light Mode" : "Switch to Dark Mode"}
        >
          <Icon name={theme === "dark" ? "sun" : "moon"} />
          {theme === "dark" ? "Light Mode" : "Dark Mode"}
        </button>
        <button className={`sidebar__item${showSettings ? " sidebar__item--active" : ""}`} onClick={onToggleSettings}>
          <span className="sidebar__item-label">
            <Icon name="settings" />
            Settings
          </span>
        </button>
        <button className="sidebar__item" onClick={onShowAbout}>
          <span className="sidebar__item-label">
            <Icon name="info" />
            About ODM
          </span>
        </button>
      </div>
    </aside>
  );
}

function App() {
  const [collapsedPlaylists, setCollapsedPlaylists] = useState<Set<string>>(new Set());
  const [tasks, setTasks] = useState<Task[]>([]);
  const [categories, setCategories] = useState<Category[]>([]);
  const [showSettings, setShowSettings] = useState(false);
  const [showAbout, setShowAbout] = useState(false);
  const [showAddModal, setShowAddModal] = useState(false);
  const [pasteUrl, setPasteUrl] = useState("");
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; task: TaskWithSpeed } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<TaskWithSpeed[] | null>(null);
  const [deletePlaylistTitle, setDeletePlaylistTitle] = useState<string | undefined>();
  const [statusTab, setStatusTab] = useState<TaskStatus | "All">("All");
  const [categoryFilter, setCategoryFilter] = useState<string | null>(null);
  const [sortKey, setSortKey] = useState<SortKey>("added-desc");
  const [search, setSearch] = useState("");
  const [engineStatus, setEngineStatus] = useState<string | null>(null);
  const [updatingEngine, setUpdatingEngine] = useState(false);
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const tasksRef = useRef<Task[]>([]);
  const appVersion = useAppVersion();
  const { update: availableUpdate, dismiss: dismissUpdate } = useUpdateCheck(appVersion);
  const [theme, toggleTheme] = useTheme();
  const { prefs: notifyPrefs, setPref: setNotifyPref, loaded: notifyPrefsLoaded } = useNotificationPrefs();
  const notifyPrefsRef = useRef(notifyPrefs);
  useEffect(() => {
    notifyPrefsRef.current = notifyPrefs;
  }, [notifyPrefs]);
  const { prefs: dialogPrefs, setPref: setDialogPref, loaded: dialogPrefsLoaded } = useDialogPrefs();
  const dialogPrefsRef = useRef(dialogPrefs);
  useEffect(() => {
    dialogPrefsRef.current = dialogPrefs;
  }, [dialogPrefs]);

  useEffect(() => {
    (async () => {
      const granted = await isPermissionGranted().catch(() => false);
      if (!granted) await requestPermission().catch(() => {});
    })();
  }, []);

  function pushToast(toast: Omit<ToastItem, "id">) {
    const id = `${Date.now()}-${Math.random()}`;
    setToasts((ts) => [...ts, { ...toast, id }]);
    setTimeout(() => setToasts((ts) => ts.filter((t) => t.id !== id)), 6000);
  }

  async function handleUpdateYtdlp() {
    setUpdatingEngine(true);
    setEngineStatus(null);
    try {
      const result = await api.updateYtdlp();
      setEngineStatus(result || "Already up to date.");
    } catch (err) {
      setEngineStatus(`Update failed: ${err}`);
    } finally {
      setUpdatingEngine(false);
    }
  }

  async function handlePasteLink() {
    try {
      const text = await navigator.clipboard.readText();
      setPasteUrl(text.trim());
    } catch {
      setPasteUrl("");
    }
    setShowAddModal(true);
  }

  async function refreshCategories() {
    setCategories(await api.listCategories());
  }

  useEffect(() => {
    api
      .listDownloads()
      .then((t) => {
        tasksRef.current = t;
        setTasks(t);
      })
      .catch(() => {});
    refreshCategories().catch(() => {});

    const unlisten = listen<Task[]>("downloads-updated", (event) => {
      const previousById = new Map(tasksRef.current.map((t) => [t.id, t]));
      for (const task of event.payload) {
        const before = previousById.get(task.id);

        // "Started" covers a brand-new task that's already downloading and a
        // Queued -> Downloading transition, but not a Paused -> Downloading
        // resume (that's not really a "start" from the user's perspective).
        const started = task.status === "Downloading" && (!before || (before.status !== "Downloading" && before.status !== "Paused"));
        if (started && notifyPrefsRef.current.start) {
          const repeated =
            looksAutoRenamed(task.dest_path) ||
            tasksRef.current.some((existing) => existing.id !== task.id && existing.url === task.url);
          const warning = repeated ? "Already downloaded — saving as a renamed copy" : "";
          sendClickableStartedNotification(displayNameOf(task));
          spawnPopup("started", { name: displayNameOf(task), warning });
          if (repeated) pushToast({ kind: "error", title: warning });
        }

        if (before && before.status !== task.status) {
          if (task.status === "Completed") {
            const fallback = qualityFallbackMessage(task);
            if (dialogPrefsRef.current.showCompleteDialog) {
              const thumb = thumbFor(task.category);
              spawnPopup("complete", {
                name: displayNameOf(task),
                url: task.url,
                destPath: task.dest_path,
                totalBytes: String(task.total_bytes || 0),
                thumbBg: thumb.bg,
                thumbIcon: thumb.icon,
                fallback: fallback || "",
              });
            } else {
              pushToast({ kind: "success", title: fallback || `${displayNameOf(task)} completed`, destPath: task.dest_path });
            }
            if (notifyPrefsRef.current.complete) {
              sendNotification({ title: "Download completed", body: fallback ? `${displayNameOf(task)} — ${fallback}` : displayNameOf(task) });
            }
          } else if (task.status === "Failed") {
            pushToast({ kind: "error", title: `${displayNameOf(task)} failed — ${task.error_message || "connection interrupted"}` });
            if (notifyPrefsRef.current.failed) {
              sendNotification({ title: "Download failed", body: `${displayNameOf(task)} — ${task.error_message || "connection interrupted"}` });
              spawnPopup("failed", { name: displayNameOf(task), error: task.error_message || "connection interrupted" });
            }
          }
        }
      }
      tasksRef.current = event.payload;
      setTasks(event.payload);
    });
    return () => {
      unlisten.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const tasksWithSpeed = useTaskSpeeds(tasks);

  const searchFiltered = useMemo(() => {
    if (!search.trim()) return tasksWithSpeed;
    const needle = search.toLowerCase();
    return tasksWithSpeed.filter((t) => displayNameOf(t).toLowerCase().includes(needle) || t.url.toLowerCase().includes(needle));
  }, [tasksWithSpeed, search]);

  const categoryFiltered = useMemo(() => {
    if (!categoryFilter) return searchFiltered;
    return searchFiltered.filter((t) => t.category === categoryFilter);
  }, [searchFiltered, categoryFilter]);

  const visibleTasks = useMemo(() => {
    const byStatus = statusTab === "All" ? categoryFiltered : categoryFiltered.filter((t) => t.status === statusTab);
    return sortTasks(byStatus, sortKey);
  }, [categoryFiltered, statusTab, sortKey]);

  const statusCounts = useMemo(() => {
    const counts: Record<string, number> = { All: categoryFiltered.length };
    for (const s of ["Downloading", "Completed", "Paused", "Failed"] as TaskStatus[]) {
      counts[s] = categoryFiltered.filter((t) => t.status === s).length;
    }
    return counts;
  }, [categoryFiltered]);

  const categoryCounts = useMemo(
    () => categories.map((c) => ({ name: c.name, count: searchFiltered.filter((t) => t.category === c.name).length })),
    [categories, searchFiltered]
  );

  const selectedTask = tasksWithSpeed.find((t) => t.id === selectedId) || null;

  const activeCount = tasksWithSpeed.filter((t) => t.status === "Downloading").length;
  const totalSpeed = tasksWithSpeed.reduce((sum, t) => sum + (t.status === "Downloading" ? t.bytesPerSec : 0), 0);

  async function handleAction(id: number, action: "pause" | "resume" | "cancel") {
    if (action === "pause") await api.pauseDownload(id);
    if (action === "resume") await api.resumeDownload(id);
    if (action === "cancel") await api.cancelDownload(id);
  }

  async function refreshTasks() {
    const fresh = await api.listDownloads();
    tasksRef.current = fresh;
    setTasks(fresh);
  }

  async function handleRename(task: TaskWithSpeed) {
    try {
      const picked = await saveFileDialog({ defaultPath: task.dest_path });
      if (!picked) return;
      await api.renameDownload(task.id, picked);
      await refreshTasks();
    } catch (err) {
      pushToast({ kind: "error", title: `Rename failed — ${String(err)}` });
    }
  }

  async function handleRedownload(task: TaskWithSpeed) {
    try {
      await api.addDownload(task.url, undefined, task.allow_playlist, task.video_quality ? String(task.video_quality) : "best");
      await refreshTasks();
    } catch (err) {
      pushToast({ kind: "error", title: `Redownload failed — ${String(err)}` });
    }
  }

  async function handleDeleteConfirm(deleteFile: boolean) {
    const targets = deleteTarget || [];
    setDeleteTarget(null);
    setDeletePlaylistTitle(undefined);
    const errors: string[] = [];
    // Stop the whole group first so queued entries cannot start while its
    // earlier rows are being removed.
    for (const t of targets) {
      try { await api.cancelDownload(t.id); } catch (err) { errors.push(String(err)); }
    }
    for (const t of targets) {
      try { await api.deleteDownload(t.id, deleteFile); } catch (err) { errors.push(String(err)); }
    }
    if (errors.length) pushToast({ kind: "error", title: `Some items could not be deleted: ${errors[0]}` });
    if (selectedId && targets.some((t) => t.id === selectedId)) setSelectedId(null);
    await refreshTasks();
  }

  const rowMenu: RowMenuCallbacks = {
    onAction: handleAction,
    onRename: handleRename,
    onRedownload: handleRedownload,
    onDelete: (task) => setDeleteTarget([task]),
    onProperties: (task) => setSelectedId(task.id),
  };

  return (
    <main className="app">
      <div className="app-shell">
        <Sidebar
          statusTab={statusTab}
          onStatusTab={setStatusTab}
          categoryFilter={categoryFilter}
          onCategoryFilter={setCategoryFilter}
          categoryCounts={categoryCounts}
          statusCounts={statusCounts}
          showSettings={showSettings}
          onToggleSettings={() => setShowSettings((s) => !s)}
          onShowAbout={() => setShowAbout(true)}
          version={appVersion}
          theme={theme}
          onToggleTheme={toggleTheme}
        />

        <div className="main-content">
          {availableUpdate && (
            <div className="update-banner">
              <span>🎉 ODM v{availableUpdate.version} is available (you're on v{appVersion}).</span>
              <button type="button" className="btn btn--primary btn--sm" onClick={() => openUrl(availableUpdate.url)}>
                View release
              </button>
              <button type="button" className="update-banner__dismiss" onClick={dismissUpdate} aria-label="Dismiss">
                ×
              </button>
            </div>
          )}
          <header className="toolbar">
            <button
              className="btn btn--primary"
              onClick={() => {
                setPasteUrl("");
                setShowAddModal(true);
              }}
            >
              + Add Download
            </button>
            <button className="btn" onClick={handlePasteLink}>
              Paste Link
            </button>

            <input className="toolbar__search" type="search" placeholder="Search downloads…" value={search} onChange={(e) => setSearch(e.currentTarget.value)} />

            <div className="spacer" />
            {engineStatus && <span className="engine-status">{engineStatus}</span>}
            <button className="btn btn--chrome" onClick={() => openUrl(CHROME_EXTENSION_URL)} title="Install the official ODM browser extension">
              Add Chrome Extension
            </button>
            <button className="btn btn--coffee" onClick={() => openUrl("https://muzammilijaz.gumroad.com/coffee")} title="Support this project">
              ☕ Buy me a coffee
            </button>
            <button
              className="btn"
              onClick={handleUpdateYtdlp}
              disabled={updatingEngine}
              title="Site extractors break as sites change — update yt-dlp to fix broken video downloads"
            >
              {updatingEngine ? "⏳ Updating…" : "🔄 Update yt-dlp"}
            </button>
            <button className="btn" onClick={() => setShowSettings((s) => !s)}>
              ⚙ Settings
            </button>
          </header>

          {showSettings ? (
            <CategoriesPanel
              categories={categories}
              onChange={refreshCategories}
              notifyPrefs={notifyPrefs}
              onSetNotifyPref={setNotifyPref}
              notifyPrefsLoaded={notifyPrefsLoaded}
              dialogPrefs={dialogPrefs}
              onSetDialogPref={setDialogPref}
              dialogPrefsLoaded={dialogPrefsLoaded}
            />
          ) : (
            <>
              <div className="tabs-row">
                {STATUS_TABS.map((t) => (
                  <button key={t.key} className={statusTab === t.key ? "status-tab status-tab--active" : "status-tab"} onClick={() => setStatusTab(t.key)}>
                    {t.label} <span className="status-tab__count">{statusCounts[t.key] ?? 0}</span>
                  </button>
                ))}
                <div className="spacer" />
                <label className="sort-select">
                  Sort by:
                  <select value={sortKey} onChange={(e) => setSortKey(e.currentTarget.value as SortKey)}>
                    {SORT_OPTIONS.map((o) => (
                      <option key={o.key} value={o.key}>
                        {o.label}
                      </option>
                    ))}
                  </select>
                </label>
                <button
                  className="btn btn--ghost"
                  disabled={visibleTasks.length === 0}
                  onClick={() => setDeleteTarget(visibleTasks)}
                  title="Delete all downloads currently shown"
                >
                  Delete All
                </button>
              </div>

              <div className="content-row">
                <div className="table-wrap">
                  <table className="downloads-table">
                    <thead>
                      <tr>
                        <th className="col-name">Name</th>
                        <th className="col-quality">Quality</th>
                        <th className="col-size">Size</th>
                        <th className="col-status">Status</th>
                        <th className="col-speed">Speed</th>
                        <th className="col-added">Added</th>
                        <th className="col-actions" />
                      </tr>
                    </thead>
                    <tbody>
                      {visibleTasks.length === 0 && (
                        <tr>
                          <td colSpan={7} className="empty">
                            {tasks.length === 0 ? (
                              <>
                                <img src={logo} alt="" className="empty__icon-logo" />
                                <div className="empty__title">No downloads yet</div>
                                <div className="empty__subtitle">Start downloading videos, files, and media from your favorite websites.</div>
                                <button
                                  className="btn btn--primary"
                                  onClick={() => {
                                    setPasteUrl("");
                                    setShowAddModal(true);
                                  }}
                                >
                                  + Add Download
                                </button>
                              </>
                            ) : (
                              "No downloads match this filter."
                            )}
                          </td>
                        </tr>
                      )}
                      {visibleTasks.filter(task => !task.playlist_group).map((task) => (
                        <DownloadTableRow
                          key={task.id}
                          task={task}
                          selected={task.id === selectedId}
                          onSelect={() => setSelectedId(task.id === selectedId ? null : task.id)}
                          menu={rowMenu}
                          onContextMenu={(e, t) => {
                            e.preventDefault();
                            setContextMenu({ x: e.clientX, y: e.clientY, task: t });
                          }}
                        />
                      ))}
                      {[...new Set(visibleTasks.map(task => task.playlist_group).filter((group): group is string => Boolean(group)))].map(group => {
                        const all = tasks.filter(task => task.playlist_group === group);
                        const completed = all.filter(task => task.status === "Completed").length;
                        const queued = all.filter(task => task.status === "Queued").length;
                        const failed = all.filter(task => task.status === "Failed").length;
                        const downloading = all.filter(task => task.status === "Downloading").length;
                        const collapsed = collapsedPlaylists.has(group);
                        return <Fragment key={group}>
                          <tr className="playlist-group"><td colSpan={7}>
                            <button type="button" className="btn" aria-expanded={!collapsed} onClick={() => setCollapsedPlaylists(previous => {
                              const next = new Set(previous); if (next.has(group)) next.delete(group); else next.add(group); return next;
                            })}>{collapsed ? "▸" : "▾"} 📁 {all[0]?.playlist_title || "YouTube Playlist"}</button>
                            <span> {completed}/{all.length} completed · {all.length - completed} remaining · {queued} queued · {downloading} downloading{failed > 0 ? ` · ${failed} failed` : ""}</span>
                            <button type="button" className="btn btn--danger playlist-delete" onClick={() => {
                              setDeletePlaylistTitle(all[0]?.playlist_title || "YouTube Playlist");
                              setDeleteTarget(tasksWithSpeed.filter(task => task.playlist_group === group));
                            }}>Delete playlist</button>
                          </td></tr>
                          {!collapsed && visibleTasks.filter(task => task.playlist_group === group).sort((a,b) => a.id-b.id).map(task => <DownloadTableRow
                            key={task.id} task={task} selected={task.id === selectedId}
                            onSelect={() => setSelectedId(task.id === selectedId ? null : task.id)} menu={rowMenu}
                            onContextMenu={(e,t) => { e.preventDefault(); setContextMenu({x:e.clientX,y:e.clientY,task:t}); }}
                          />)}
                        </Fragment>;
                      })}
                    </tbody>
                  </table>
                </div>

                {selectedTask && <RightDetailsPanel task={selectedTask} onClose={() => setSelectedId(null)} onAction={handleAction} />}
              </div>

              <div className="stats-bar">
                <span>{activeCount > 0 ? `${activeCount} downloading` : "No active downloads"}</span>
                {totalSpeed > 0 && <span>⚡ {formatSpeed(totalSpeed)}</span>}
              </div>
            </>
          )}
        </div>
      </div>

      {showAddModal && (
        <AddDownloadModal
          initialUrl={pasteUrl}
          categories={categories}
          showDetails={dialogPrefs.showStartDetails}
          onToggleShowDetails={(show) => setDialogPref("showStartDetails", show)}
          onClose={() => setShowAddModal(false)}
          onAdded={() =>
            api.listDownloads().then((t) => {
              tasksRef.current = t;
              setTasks(t);
            })
          }
        />
      )}

      {showAbout && <AboutModal onClose={() => setShowAbout(false)} version={appVersion} />}

      {contextMenu && (
        <ContextMenu x={contextMenu.x} y={contextMenu.y} task={contextMenu.task} menu={rowMenu} onClose={() => setContextMenu(null)} />
      )}

      {deleteTarget && (
        <DeleteConfirmModal count={deleteTarget.length} playlistTitle={deletePlaylistTitle} onCancel={() => { setDeleteTarget(null); setDeletePlaylistTitle(undefined); }} onConfirm={handleDeleteConfirm} />
      )}

      <ToastStack toasts={toasts} onDismiss={(id) => setToasts((ts) => ts.filter((t) => t.id !== id))} />
    </main>
  );
}

export default App;
