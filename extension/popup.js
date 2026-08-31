const listEl = document.getElementById("detected-list");
const statusEl = document.getElementById("status");
const formEl = document.getElementById("add-form");
const urlInput = document.getElementById("add-url");
const hostStatusEl = document.getElementById("host-status");

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

async function sendDownload(url) {
  setStatus("Sending…");
  const response = await chrome.runtime.sendMessage({ type: "sendDownload", url, filename: filenameFromUrl(url) });
  if (response?.ok) {
    setStatus("Added to ODM.", "ok");
    setHostStatus(true);
  } else {
    setStatus(response?.error || "Failed to send.", "error");
    if (response?.error && /desktop app/i.test(response.error)) setHostStatus(false);
  }
}

function setHostStatus(online) {
  hostStatusEl.classList.remove("online", "offline");
  hostStatusEl.classList.add(online ? "online" : "offline");
  hostStatusEl.querySelector(".text").textContent = online ? "Connected" : "App offline";
}

async function checkHostStatus() {
  const response = await chrome.runtime.sendMessage({ type: "ping" });
  setHostStatus(!!response?.ok);
}

async function loadDetected() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab) return;
  const detected = await chrome.runtime.sendMessage({ type: "getDetected", tabId: tab.id });

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
}

formEl.addEventListener("submit", async (e) => {
  e.preventDefault();
  const url = urlInput.value.trim();
  if (!url) return;
  await sendDownload(url);
  urlInput.value = "";
});

const autoCaptureEl = document.getElementById("auto-capture");
chrome.storage.local.get({ autoCapture: true }, (s) => {
  autoCaptureEl.checked = s.autoCapture;
});
autoCaptureEl.addEventListener("change", () => {
  chrome.storage.local.set({ autoCapture: autoCaptureEl.checked });
});

checkHostStatus();
loadDetected();
