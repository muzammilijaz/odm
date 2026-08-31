import { useEffect, useMemo, useRef, useState, type ReactElement } from "react";
import { listen } from "@tauri-apps/api/event";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
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
        <span className="status-progress__pct">{indeterminate ? "…" : `${pct}%`}</span>
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
    <tr className={selected ? "row--selected" : undefined} onClick={onSelect} onContextMenu={(e) => onContextMenu(e, task)}>
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
        </div>
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

function AddDownloadModal({ initialUrl, onClose, onAdded }: { initialUrl?: string; onClose: () => void; onAdded: () => void }) {
  const [url, setUrl] = useState(initialUrl || "");
  const [filename, setFilename] = useState("");
  const [playlist, setPlaylist] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isVideoSite = isKnownVideoUrl(url);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!url.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await api.addDownload(url.trim(), filename.trim() || undefined, playlist);
      onAdded();
      onClose();
    } catch (err) {
      setError(String(err));
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
        <input type="url" autoFocus required placeholder="https://example.com/video.mp4" value={url} onChange={(e) => setUrl(e.currentTarget.value)} />

        {isVideoSite ? (
          <label className="modal__checkbox">
            <input type="checkbox" checked={playlist} onChange={(e) => setPlaylist(e.currentTarget.checked)} />
            Download entire playlist (if this is a playlist URL)
          </label>
        ) : (
          <>
            <label className="modal__label">File name (optional)</label>
            <input type="text" placeholder="Filename will be detected automatically" value={filename} onChange={(e) => setFilename(e.currentTarget.value)} />
          </>
        )}

        {error && <div className="form-error">{error}</div>}

        <div className="modal__footer">
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" className="btn btn--primary" disabled={busy}>
            {busy ? "Adding…" : "+ Add Download"}
          </button>
        </div>
      </form>
    </div>
  );
}

function AboutModal({ onClose }: { onClose: () => void }) {
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
          <div className="muted">Version 0.1.0</div>
          <p className="muted about__desc">A fast, modern download manager for Windows — direct links, and video/audio via yt-dlp.</p>
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
  onCancel,
  onConfirm,
}: {
  count: number;
  onCancel: () => void;
  onConfirm: (deleteFile: boolean) => void;
}) {
  const [deleteFile, setDeleteFile] = useState(false);
  const plural = count > 1;

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal modal--delete" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h2>Delete {plural ? `${count} downloads` : "download"}?</h2>
          <button type="button" className="modal__close" onClick={onCancel}>
            ×
          </button>
        </div>
        <p className="muted">This removes {plural ? "these downloads" : "this download"} from ODM's list.</p>
        <label className="modal__checkbox">
          <input type="checkbox" checked={deleteFile} onChange={(e) => setDeleteFile(e.currentTarget.checked)} />
          Also delete the file{plural ? "s" : ""} from my PC
        </label>
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

function CategoriesPanel({ categories, onChange }: { categories: Category[]; onChange: () => void }) {
  const [newExt, setNewExt] = useState<Record<string, string>>({});

  return (
    <div className="categories">
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
}) {
  return (
    <aside className="sidebar">
      <div className="sidebar__header">
        <img src={logo} alt="ODM" className="sidebar__logo" />
        <div>
          <div className="sidebar__brand">ODM</div>
          <div className="sidebar__tagline">Open Download Manager</div>
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
  const [tasks, setTasks] = useState<Task[]>([]);
  const [categories, setCategories] = useState<Category[]>([]);
  const [showSettings, setShowSettings] = useState(false);
  const [showAbout, setShowAbout] = useState(false);
  const [showAddModal, setShowAddModal] = useState(false);
  const [pasteUrl, setPasteUrl] = useState("");
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; task: TaskWithSpeed } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<TaskWithSpeed[] | null>(null);
  const [statusTab, setStatusTab] = useState<TaskStatus | "All">("All");
  const [categoryFilter, setCategoryFilter] = useState<string | null>(null);
  const [sortKey, setSortKey] = useState<SortKey>("added-desc");
  const [search, setSearch] = useState("");
  const [engineStatus, setEngineStatus] = useState<string | null>(null);
  const [updatingEngine, setUpdatingEngine] = useState(false);
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const tasksRef = useRef<Task[]>([]);

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
        if (before && before.status !== task.status) {
          if (task.status === "Completed") {
            pushToast({ kind: "success", title: `${displayNameOf(task)} completed`, destPath: task.dest_path });
          } else if (task.status === "Failed") {
            pushToast({ kind: "error", title: `${displayNameOf(task)} failed — ${task.error_message || "connection interrupted"}` });
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
      await api.addDownload(task.url);
      await refreshTasks();
    } catch (err) {
      pushToast({ kind: "error", title: `Redownload failed — ${String(err)}` });
    }
  }

  async function handleDeleteConfirm(deleteFile: boolean) {
    const targets = deleteTarget || [];
    setDeleteTarget(null);
    for (const t of targets) {
      await api.deleteDownload(t.id, deleteFile).catch(() => {});
    }
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
        />

        <div className="main-content">
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
            <button
              className="btn"
              onClick={handleUpdateYtdlp}
              disabled={updatingEngine}
              title="Site extractors break as sites change — update yt-dlp to fix broken video downloads"
            >
              {updatingEngine ? "Updating…" : "Update yt-dlp"}
            </button>
            <button className="btn" onClick={() => setShowSettings((s) => !s)}>
              ⚙ Settings
            </button>
          </header>

          {showSettings ? (
            <CategoriesPanel categories={categories} onChange={refreshCategories} />
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
                          <td colSpan={6} className="empty">
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
                      {visibleTasks.map((task) => (
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
          onClose={() => setShowAddModal(false)}
          onAdded={() =>
            api.listDownloads().then((t) => {
              tasksRef.current = t;
              setTasks(t);
            })
          }
        />
      )}

      {showAbout && <AboutModal onClose={() => setShowAbout(false)} />}

      {contextMenu && (
        <ContextMenu x={contextMenu.x} y={contextMenu.y} task={contextMenu.task} menu={rowMenu} onClose={() => setContextMenu(null)} />
      )}

      {deleteTarget && (
        <DeleteConfirmModal count={deleteTarget.length} onCancel={() => setDeleteTarget(null)} onConfirm={handleDeleteConfirm} />
      )}

      <ToastStack toasts={toasts} onDismiss={(id) => setToasts((ts) => ts.filter((t) => t.id !== id))} />
    </main>
  );
}

export default App;
