const listEl = document.getElementById("detected-list");
const statusEl = document.getElementById("status");
const formEl = document.getElementById("add-form");
const urlInput = document.getElementById("add-url");
const hostStatusEl = document.getElementById("host-status");
const qualityEl = document.getElementById("video-quality");

function setStatus(text, kind) {
  statusEl.textContent = text;
  statusEl.className = "status" + (kind ? ` ${kind}` : "");
}

function filenameFromUrl(url) {
  try {
    const path = new URL(url).pathname;
    const last = path.split("/").filter(Boolean).pop();
    return last || undefined;
  } catch {
    return undefined;
  }
}

const pendingDownloads = new Set();
async function sendDownload(url) {
  const key = JSON.stringify([url, qualityEl.value]);
  if (pendingDownloads.has(key)) return false;
  pendingDownloads.add(key);
  try {
  setStatus("Sending…");
  const response = await chrome.runtime.sendMessage({ type: "sendDownload", url, filename: filenameFromUrl(url), quality: qualityEl.value });
  if (response?.ok) {
    setStatus("Added to ODM.", "ok");
    setHostStatus(true);
    return true;
  } else {
    setStatus(response?.error || "Failed to send.", "error");
    if (response?.error && /desktop app/i.test(response.error)) setHostStatus(false);
  }
  } catch (error) {
    setStatus(`Could not connect to ODM: ${error?.message || error}. Reopen the extension and retry.`, "error");
    setHostStatus(false);
  } finally {
    pendingDownloads.delete(key);
  }
  return false;
}

function setHostStatus(online) {
  hostStatusEl.classList.remove("online", "offline");
  hostStatusEl.classList.add(online ? "online" : "offline");
  hostStatusEl.querySelector(".text").textContent = online ? "Connected" : "App offline";
}

async function checkHostStatus() {
  try {
  const response = await chrome.runtime.sendMessage({ type: "ping" });
  setHostStatus(!!response?.ok);
  } catch { setHostStatus(false); }
}

async function loadDetected() {
  try {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab) return;
  const detected = await chrome.runtime.sendMessage({ type: "getDetected", tabId: tab.id });
  if (detected?.ok === false) throw new Error(detected.error);

  listEl.innerHTML = "";
  if (!detected || detected.length === 0) {
    const li = document.createElement("li");
    li.className = "empty";
    li.textContent = "Nothing detected yet.";
    listEl.appendChild(li);
    return;
  }

  for (const entry of detected) {
    const li = document.createElement("li");
    const urlSpan = document.createElement("span");
    urlSpan.className = "url";
    urlSpan.title = entry.url;
    urlSpan.textContent = entry.url;

    const btn = document.createElement("button");
    btn.textContent = "Add";
    btn.onclick = () => sendDownload(entry.url);

    li.appendChild(urlSpan);
    li.appendChild(btn);
    listEl.appendChild(li);
  }
  } catch (error) {
    listEl.innerHTML = "";
    setStatus(`Could not load detected media: ${error?.message || error}`, "error");
  }
}

formEl.addEventListener("submit", async (e) => {
  e.preventDefault();
  const url = urlInput.value.trim();
  if (!url) return;
  if (await sendDownload(url) && urlInput.value.trim() === url) urlInput.value = "";
});

const autoCaptureEl = document.getElementById("auto-capture");
chrome.storage.local.get({ autoCapture: true }, (s) => {
  autoCaptureEl.checked = s.autoCapture;
});

chrome.storage.local.get({ videoQuality: "default" }, (s) => {
  qualityEl.value = s.videoQuality;
});
qualityEl.addEventListener("change", () => {
  chrome.storage.local.set({ videoQuality: qualityEl.value });
});
autoCaptureEl.addEventListener("change", () => {
  chrome.storage.local.set({ autoCapture: autoCaptureEl.checked });
});

document.getElementById("version").textContent = `v${chrome.runtime.getManifest().version}`;

checkHostStatus();
loadDetected();
