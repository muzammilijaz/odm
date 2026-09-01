import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { api } from "./api";
import logo from "./assets/odm-logo.png";
import "./App.css";

/** These floating windows are spawned by the main window (see spawnPopup in
 * App.tsx) instead of relying on the OS toast tray -- Windows only shows
 * native toasts for properly-installed apps with a registered AUMID, which
 * an unpackaged dev build doesn't have, so OS notifications silently
 * disappear during `tauri dev`. A same-styled always-on-top window works
 * identically in dev and in the installed build. */

function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  return `${(bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

function closeThisWindow() {
  getCurrentWindow()
    .close()
    .catch(() => {});
}

function StartedPopup({ params }: { params: URLSearchParams }) {
  const name = params.get("name") || "Download";

  useEffect(() => {
    const t = setTimeout(closeThisWindow, 5000);
    return () => clearTimeout(t);
  }, []);

  return (
    <div className="popup-card popup-card--compact">
      <img src={logo} alt="" className="popup-card__logo" />
      <div className="popup-card__body">
        <div className="popup-card__title">Download started</div>
        <div className="popup-card__name" title={name}>
          {name}
        </div>
      </div>
      <button type="button" className="popup-card__close" onClick={closeThisWindow} title="Close">
        ×
      </button>
    </div>
  );
}

function FailedPopup({ params }: { params: URLSearchParams }) {
  const name = params.get("name") || "Download";
  const error = params.get("error") || "connection interrupted";

  return (
    <div className="popup-card popup-card--compact popup-card--danger">
      <span className="popup-card__status-icon">⚠</span>
      <div className="popup-card__body">
        <div className="popup-card__title">Download failed</div>
        <div className="popup-card__name" title={name}>
          {name}
        </div>
        <div className="popup-card__error" title={error}>
          {error}
        </div>
      </div>
      <button type="button" className="popup-card__close" onClick={closeThisWindow} title="Close">
        ×
      </button>
    </div>
  );
}

function CompletePopup({ params }: { params: URLSearchParams }) {
  const name = params.get("name") || "Download";
  const url = params.get("url") || "";
  const destPath = params.get("destPath") || "";
  const totalBytes = Number(params.get("totalBytes") || 0);
  const thumbBg = params.get("thumbBg") || "#64748b";
  const thumbIcon = params.get("thumbIcon") || "📦";
  const [dontShowAgain, setDontShowAgain] = useState(false);

  function close() {
    if (dontShowAgain) api.setSetting("dialog_complete", "false").catch(() => {});
    closeThisWindow();
  }

  return (
    <div className="popup-card popup-card--complete">
      <div className="modal__header">
        <h2>Download complete</h2>
        <button type="button" className="modal__close" onClick={close}>
          ×
        </button>
      </div>

      <div className="download-complete">
        <span className="thumb download-complete__icon" style={{ background: thumbBg }}>
          {thumbIcon}
        </span>
        <div className="download-complete__text">
          <div className="row-name" title={name}>
            {name}
          </div>
          <div className="muted">{totalBytes ? `Downloaded ${formatBytes(totalBytes)}` : "Download complete"}</div>
        </div>
      </div>

      <label className="modal__label">Address</label>
      <input type="text" readOnly value={url} onFocus={(e) => e.currentTarget.select()} />

      <label className="modal__label">The file saved as</label>
      <input type="text" readOnly value={destPath} onFocus={(e) => e.currentTarget.select()} />

      <label className="modal__checkbox modal__checkbox--muted">
        <input type="checkbox" checked={dontShowAgain} onChange={(e) => setDontShowAgain(e.currentTarget.checked)} />
        Don't show this dialog again
      </label>

      <div className="modal__footer">
        <button
          type="button"
          className="btn"
          onClick={() => {
            revealItemInDir(destPath).catch(() => {});
            close();
          }}
        >
          Open Folder
        </button>
        <button
          type="button"
          className="btn btn--primary"
          onClick={() => {
            openPath(destPath).catch(() => {});
            close();
          }}
        >
          Open
        </button>
      </div>
    </div>
  );
}

export default function Popup() {
  const params = new URLSearchParams(window.location.search);
  const type = params.get("popup");

  // Popup windows are a separate document from the main window, so they
  // don't inherit its `data-theme` attribute automatically -- but they do
  // share the same localStorage (same app origin), so read the saved choice
  // directly to stay visually consistent with whatever the main window is set to.
  useEffect(() => {
    try {
      const stored = localStorage.getItem("odm-theme");
      if (stored === "light" || stored === "dark") document.documentElement.setAttribute("data-theme", stored);
    } catch {
      // best effort
    }
  }, []);

  return (
    <div className="popup-root">
      {type === "complete" ? <CompletePopup params={params} /> : type === "failed" ? <FailedPopup params={params} /> : <StartedPopup params={params} />}
    </div>
  );
}
