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

    if (isManifest || isLargeMedia) {
      recordDetection(details.tabId, { url: details.url, contentType, contentLength });
    }
  },
  { urls: ["<all_urls>"] },
  ["responseHeaders"]
);

chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (changeInfo.status === "loading" && changeInfo.url) {
    clearDetections(tabId);
  }
});

chrome.tabs.onRemoved.addListener((tabId) => {
  clearDetections(tabId);
});

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === "ping") {
    sendToNativeHost({ action: "ping" })
      .then(() => sendResponse({ ok: true }))
      .catch(() => sendResponse({ ok: false }));
    return true;
  }
  if (message.type === "getDetected") {
    getDetections(message.tabId).then(sendResponse);
    return true;
  }
  if (message.type === "sendDownload") {
    sendToNativeHost({ action: "add_download", url: message.url, filename: message.filename })
      .then((res) => sendResponse({ ok: true, res }))
      .catch((err) => sendResponse({ ok: false, error: String(err) }));
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
    if (pageUrl && isKnownVideoHost(pageUrl)) {
      sendToNativeHost({ action: "add_download", url: pageUrl })
        .then((res) => sendResponse({ ok: true, res }))
        .catch((err) => sendResponse({ ok: false, error: String(err?.message || err) }));
      return true;
    }
    (async () => {
      const detected = await getDetections(tabId);
      if (detected && detected.length > 0) {
        try {
          const best = pickBestDetection(detected);
          const res = await sendToNativeHost({ action: "add_download", url: best.url });
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

async function recordDetection(tabId, entry) {
  const key = storageKey(tabId);
  const store = await chrome.storage.session.get(key);
  const list = store[key] || [];
  if (list.some((e) => e.url === entry.url)) return;
  list.push(entry);
  await chrome.storage.session.set({ [key]: list });
  // The tab can close between the network response that triggered this and
  // this update running -- both calls throw "No tab with id" in that case,
  // which is harmless (nothing left to badge) but surfaces as an unhandled
  // promise rejection if not swallowed.
  chrome.action.setBadgeText({ tabId, text: String(list.length) }).catch(() => {});
  chrome.action.setBadgeBackgroundColor({ tabId, color: "#3b82f6" }).catch(() => {});
}

async function getDetections(tabId) {
  const key = storageKey(tabId);
  const store = await chrome.storage.session.get(key);
  return store[key] || [];
}

async function clearDetections(tabId) {
  await chrome.storage.session.remove(storageKey(tabId));
  await chrome.storage.session.remove(candidateKey(tabId));
  chrome.action.setBadgeText({ tabId, text: "" }).catch(() => {});
}

const MAX_CANDIDATES = 8;

async function recordCandidate(tabId, entry) {
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
        reject(new Error(chrome.runtime.lastError.message + " (is the ODM desktop app installed and running?)"));
        return;
      }
      if (response && response.ok === false) {
        reject(new Error(response.error || "ODM rejected the request"));
        return;
      }
      resolve(response);
    });
  });
}
