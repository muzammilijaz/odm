// ODM background service worker.
//
// Two capture paths, mirroring the plan's Phase 4 design:
//  1. Right-click "Download with ODM" on any link (chrome.contextMenus) —
//     the primary, always-available interaction.
//  2. Passive stream sniffing (chrome.webRequest, read-only) for HLS/DASH
//     manifests and large video/audio responses, surfaced in the popup for
//     the user to add.
//
// Every capture is relayed to the ODM desktop app via Chrome's Native
// Messaging protocol, to a small native host binary which forwards to the
// app's local loopback HTTP API. See native-host/ and README.md for setup.

const NATIVE_HOST = "com.odm.nativehost";

// Mirrors odm-engine's KNOWN_VIDEO_HOSTS (crates/odm-engine/src/ytdlp.rs).
// Sites like YouTube multiplex video/audio through custom protocols (e.g.
// YouTube's `vnd.yt-ump`) that generic network-sniffing can't reassemble --
// confirmed live: sniffing only ever sees small `application/vnd.yt-ump`
// (0 content-length) and tiny audio fragments, never a downloadable whole
// file. For these hosts we skip sniffing and send the page URL itself,
// which ODM's backend routes to yt-dlp instead of the generic engine.
const KNOWN_VIDEO_HOSTS = [
  "youtube.com",
  "youtu.be",
  "tiktok.com",
  "instagram.com",
  "facebook.com",
  "fb.watch",
  "twitter.com",
  "x.com",
  "vimeo.com",
  "dailymotion.com",
  "twitch.tv",
  "soundcloud.com",
  "reddit.com",
];

function isKnownVideoHost(url) {
  try {
    const host = new URL(url).hostname;
    return KNOWN_VIDEO_HOSTS.some((known) => host === known || host.endsWith(`.${known}`));
  } catch {
    return false;
  }
}

const STREAM_CONTENT_TYPES = [
  "video/",
  "audio/",
  "application/vnd.apple.mpegurl",
  "application/x-mpegurl",
  "application/dash+xml",
];

const MIN_STREAM_BYTES = 200 * 1024; // skip tiny thumbnails/preview fragments

function singleVideoUrl(value) {
  try {
    const url = new URL(value);
    const youtube = url.hostname === "youtu.be" || url.hostname === "youtube.com" || url.hostname.endsWith(".youtube.com");
    if (youtube && url.pathname !== "/playlist") {
      url.searchParams.delete("list");
      url.searchParams.delete("index");
      url.searchParams.delete("start_radio");
      return url.href;
    }
  } catch {}
  return value;
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "odm-download-link",
    title: "Download with ODM",
    contexts: ["link"],
  });
  chrome.contextMenus.create({
    id: "odm-download-media",
    title: "Download media with ODM",
    contexts: ["image", "video", "audio"],
  });
});

// Auto-capture: reroute every ordinary browser download (a direct link click,
// not just right-click "Download with ODM") to the ODM desktop app, taking
// over the browser's own download manager. On by default; toggled from the
// popup and persisted in chrome.storage.local.
let autoCaptureEnabled = true;
chrome.storage.local.get({ autoCapture: true }, (s) => {
  autoCaptureEnabled = s.autoCapture;
});
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && "autoCapture" in changes) {
    autoCaptureEnabled = changes.autoCapture.newValue;
  }
});

chrome.downloads.onCreated.addListener((item) => {
  if (!autoCaptureEnabled) return;
  // blob:/data: URLs are only resolvable inside the page that generated them
  // (e.g. a client-side "export" button) -- handing them to the native host
  // as a bare URL wouldn't work, so let the browser handle those itself.
  if (!item.url || item.url.startsWith("blob:") || item.url.startsWith("data:")) return;

  sendToNativeHost({ action: "add_download", url: item.url, filename: item.filename || undefined })
    .then(() => {
      // Only cancel/erase Chrome's own download once ODM has confirmed it
      // accepted the URL -- if the desktop app isn't running, the download
      // proceeds in the browser as normal instead of getting lost.
      chrome.downloads.cancel(item.id, () => {
        chrome.downloads.erase({ id: item.id });
      });
    })
    .catch((err) => {
      console.warn("ODM: auto-capture failed, letting browser handle download", err);
    });
});

chrome.contextMenus.onClicked.addListener((info) => {
  const url = info.linkUrl || info.srcUrl;
  if (!url) return;
  sendToNativeHost({ action: "add_download", url }).catch((err) => {
    console.warn("ODM: failed to send download", err);
  });
});

chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0) return;
    const headers = details.responseHeaders || [];
    const contentType = (headers.find((h) => h.name.toLowerCase() === "content-type")?.value || "").toLowerCase();
    const contentLength = parseInt(headers.find((h) => h.name.toLowerCase() === "content-length")?.value || "0", 10);

    const isManifest = contentType.includes("mpegurl") || contentType.includes("dash+xml") || details.url.includes(".m3u8") || details.url.includes(".mpd");
    const isLargeMedia = STREAM_CONTENT_TYPES.some((t) => contentType.startsWith(t)) && contentLength >= MIN_STREAM_BYTES;

    // Temporary diagnostic: remember anything that *looks* like a video/media
    // fetch (by URL or resourceType) even if it doesn't pass the filter, in a
    // small rolling per-tab buffer. Surfaced directly in the "no stream
    // detected" error (see downloadDetectedVideo below) so it's visible in
    // the page's own console instead of requiring the separate service
    // worker DevTools window.
    if (details.type === "media" || details.url.includes("videoplayback") || contentType.startsWith("video/") || contentType.startsWith("audio/")) {
      recordCandidate(details.tabId, {
        url: details.url.slice(0, 100),
        type: details.type,
        contentType,
        contentLength,
        passed: isManifest || isLargeMedia,
      });
    }

    const capturedCdnMedia = /\.(googlevideo\.com|fbcdn\.net|cdninstagram\.com|tiktok\.com|tiktokcdn\.com)$/.test(new URL(details.url).hostname) &&
      (contentType.startsWith("video/") || contentType.startsWith("audio/"));
    if (isManifest || isLargeMedia || capturedCdnMedia) {
      recordDetection(details.tabId, { url: details.url, contentType, contentLength, capturedAt: Date.now(), frameId: details.frameId });
    }
  },
  { urls: ["<all_urls>"] },
  ["responseHeaders"]
);

chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.status === "loading" || changeInfo.url) {
    clearDetections(tabId).catch(console.warn);
  }
});

chrome.tabs.onRemoved.addListener((tabId) => {
  clearDetections(tabId).catch(console.warn);
});

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === "ping") {
    sendToNativeHost({ action: "ping" })
      .then(() => sendResponse({ ok: true }))
      .catch(() => sendResponse({ ok: false }));
    return true;
  }
  if (message.type === "getDetected") {
    getDetections(message.tabId).then(sendResponse).catch(error => sendResponse({ ok: false, error: String(error) }));
    return true;
  }
  if (message.type === "sendDownload") {
    sendToNativeHost({ action: "add_download", url: message.url, filename: message.filename, quality: message.quality || "default" })
      .then((res) => sendResponse({ ok: true, res }))
      .catch((err) => sendResponse({ ok: false, error: String(err) }));
    return true;
  }
  if (message.type === "getVideoQualities") {
    const pageUrl = message.pageUrl || sender.tab?.url;
    if (!pageUrl || !isKnownVideoHost(pageUrl)) {
      sendResponse({ ok: true, heights: [] });
      return false;
    }
    sendToNativeHost({ action: "probe_video", url: singleVideoUrl(pageUrl) })
      .then((res) =>
        sendResponse({
          ok: true,
          title: res?.qualities?.title || "Video",
          heights: Array.isArray(res?.qualities?.heights) ? res.qualities.heights : [],
        })
      )
      .catch((err) => sendResponse({ ok: false, error: String(err?.message || err), heights: [] }));
    return true;
  }
  if (message.type === "getBrowserMedia") {
    getDetections(sender.tab?.id).then((entries) => sendResponse(entries.filter((entry) =>
      Date.now() - (entry.capturedAt || 0) < 120000 && entry.frameId === sender.frameId &&
      entry.contentType?.startsWith("video/")
    ).slice(-8)));
    return true;
  }
  // Sent by content.js's floating "Download this video" overlay button —
  // picks the best stream detected so far on this tab (largest by
  // Content-Length, favoring manifests since those resolve to the full
  // stream rather than one fragment) and sends it.
  if (message.type === "downloadDetectedVideo") {
    const tabId = sender.tab?.id;
    if (tabId === undefined) {
      sendResponse({ ok: false, error: "no active tab" });
      return false;
    }
    const pageUrl = message.pageUrl || sender.tab?.url;
    const quality = String(message.quality ?? message.selectedQuality ?? "best");
    if (pageUrl && isKnownVideoHost(pageUrl)) {
      getDetections(tabId).then((entries) => sendToNativeHost({ action: "add_download", url: singleVideoUrl(pageUrl), quality,
        fallback_url: message.mediaUrl, fallback_audio: matchingAudio(message.mediaUrl, entries, sender.frameId) }))
        .then((res) => sendResponse({ ok: true, res }))
        .catch((err) => sendResponse({ ok: false, error: String(err?.message || err) }));
      return true;
    }
    (async () => {
      const detected = await getDetections(tabId);
      if (detected && detected.length > 0) {
        try {
          const best = pickBestDetection(detected);
          const res = await sendToNativeHost({ action: "add_download", url: best.url, quality });
          sendResponse({ ok: true, res });
        } catch (err) {
          sendResponse({ ok: false, error: String(err?.message || err) });
        }
        return;
      }
      const candidates = await getCandidates(tabId);
      const summary =
        candidates.length === 0
          ? "no media-looking network requests were seen at all for this tab (webRequest may not be triggering)"
          : `saw ${candidates.length} candidate request(s) but none passed the filter: ` +
            candidates.map((c) => `[${c.type} ${c.contentType || "(no content-type)"} ${c.contentLength}b]`).join(", ");
      sendResponse({ ok: false, error: `No downloadable stream detected yet -- ${summary}` });
    })();
    return true;
  }
  return false;
});

function pickBestDetection(detected) {
  const manifest = detected.find((e) => e.url.includes(".m3u8") || e.url.includes(".mpd"));
  if (manifest) return manifest;
  return detected.reduce((best, e) => ((e.contentLength || 0) > (best.contentLength || 0) ? e : best), detected[0]);
}

function matchingAudio(mediaUrl, entries, frameId) {
  try {
    const video = new URL(mediaUrl);
    const id = video.searchParams.get("id");
    if (!video.hostname.endsWith(".googlevideo.com") || !id) return undefined;
    return entries.find((entry) => {
      const audio = new URL(entry.url);
      return entry.frameId === frameId && Date.now() - (entry.capturedAt || 0) < 120000 &&
        audio.hostname.endsWith(".googlevideo.com") && audio.searchParams.get("id") === id &&
        (entry.contentType?.startsWith("audio/") || audio.searchParams.get("mime")?.startsWith("audio/"));
    })?.url;
  } catch { return undefined; }
}

// Serialize mutations so a pending capture cannot restore old URLs after
// navigation clears the tab, or overwrite a simultaneously captured stream.
const tabStorageJobs = new Map();
function storageJob(tabId, operation) {
  const job = (tabStorageJobs.get(tabId) || Promise.resolve()).then(operation);
  const settled = job.catch(error => console.warn("ODM: media storage failed", error));
  tabStorageJobs.set(tabId, settled);
  settled.then(() => { if (tabStorageJobs.get(tabId) === settled) tabStorageJobs.delete(tabId); });
  return job;
}
function recordDetection(tabId, entry) { return storageJob(tabId, () => updateDetection(tabId, entry)); }
function recordCandidate(tabId, entry) { return storageJob(tabId, () => updateCandidate(tabId, entry)); }
function clearDetections(tabId) { return storageJob(tabId, () => removeDetections(tabId)); }

async function updateDetection(tabId, entry) {
  const key = storageKey(tabId);
  const store = await chrome.storage.session.get(key);
  const list = store[key] || [];
  const previous = list.findIndex((e) => e.url === entry.url);
  if (previous >= 0) list.splice(previous, 1);
  list.push(entry);
  if (list.length > 100) list.splice(0, list.length - 100);
  await chrome.storage.session.set({ [key]: list });
  // The tab can close between the network response that triggered this and
  // this update running -- both calls throw "No tab with id" in that case,
  // which is harmless (nothing left to badge) but surfaces as an unhandled
  // promise rejection if not swallowed.
  chrome.action.setBadgeText({ tabId, text: String(list.length) }).catch(() => {});
  chrome.action.setBadgeBackgroundColor({ tabId, color: "#3b82f6" }).catch(() => {});
}

async function getDetections(tabId) {
  await tabStorageJobs.get(tabId);
  const key = storageKey(tabId);
  const store = await chrome.storage.session.get(key);
  return store[key] || [];
}

async function removeDetections(tabId) {
  await chrome.storage.session.remove(storageKey(tabId));
  await chrome.storage.session.remove(candidateKey(tabId));
  chrome.action.setBadgeText({ tabId, text: "" }).catch(() => {});
}

const MAX_CANDIDATES = 8;

async function updateCandidate(tabId, entry) {
  const key = candidateKey(tabId);
  const store = await chrome.storage.session.get(key);
  const list = store[key] || [];
  list.push(entry);
  while (list.length > MAX_CANDIDATES) list.shift();
  await chrome.storage.session.set({ [key]: list });
}

async function getCandidates(tabId) {
  const key = candidateKey(tabId);
  const store = await chrome.storage.session.get(key);
  return store[key] || [];
}

function storageKey(tabId) {
  return `detected:${tabId}`;
}

function candidateKey(tabId) {
  return `candidates:${tabId}`;
}

function sendToNativeHost(message) {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendNativeMessage(NATIVE_HOST, message, (response) => {
      if (chrome.runtime.lastError) {
        const nativeError = chrome.runtime.lastError.message || "Native Messaging failed";
        let hint = "Make sure the ODM desktop app is running.";
        if (nativeError.includes("host not found")) {
          hint = "Start the ODM desktop app once so it can register the browser connection, then retry.";
        } else if (nativeError.includes("forbidden")) {
          hint = "This extension ID is not allowed by the installed ODM native host; update or re-register ODM.";
        } else if (nativeError.includes("host has exited") || nativeError.includes("Failed to start")) {
          hint = "The ODM browser helper could not start; restart the ODM desktop app and retry.";
        }
        reject(new Error(`${nativeError} ${hint}`));
        return;
      }
      if (response?.ok === false) {
        reject(new Error(response.error || "ODM rejected the request"));
        return;
      }
      if (!response || response.ok !== true ||
          (message.action === "add_download" && (!response.task || typeof response.task !== "object" || Array.isArray(response.task)))) {
        reject(new Error("ODM returned an empty or invalid response. Check the download list before retrying."));
        return;
      }
      resolve(response);
    });
  });
}
