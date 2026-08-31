// Injects a floating "Download this video" overlay button on top of the
// page's main <video> element. Clicking it asks the background worker for
// the best stream URL detected for this tab (via webRequest sniffing in
// background.js) and sends it to ODM.

(() => {
  console.log("[ODM] content script loaded on", location.href);

  const DOWNLOAD_ICON_SVG =
    '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/></svg>';

  let currentVideo = null;
  let badge = null;
  let hiddenFor = null; // video element the user explicitly closed the badge for
  let rafId = null;

  // YouTube (and similarly, sites with hover-preview thumbnails) autoplay a
  // muted preview <video> inside home/search feed thumbnails on mouseover --
  // that used to get picked up as "the" video and put a download badge on
  // every thumbnail card. Restrict to the actual watch/shorts page there so
  // the badge only shows once a video is genuinely opened and playing.
  function isEligiblePage() {
    const host = location.hostname;
    if (host === "www.youtube.com" || host === "youtube.com" || host === "m.youtube.com") {
      return location.pathname === "/watch" || location.pathname.startsWith("/shorts/") || location.pathname.startsWith("/embed/");
    }
    return true;
  }

  function biggestVisibleVideo() {
    if (!isEligiblePage()) return null;
    const videos = Array.from(document.querySelectorAll("video"));
    let best = null;
    let bestArea = 0;
    for (const v of videos) {
      const rect = v.getBoundingClientRect();
      if (rect.width < 100 || rect.height < 80) continue; // skip tiny/hidden players
      const area = rect.width * rect.height;
      if (area > bestArea) {
        bestArea = area;
        best = v;
      }
    }
    return best;
  }

  function createBadge() {
    const el = document.createElement("div");
    el.id = "odm-download-badge";
    el.innerHTML = `
      <span class="odm-badge-icon">${DOWNLOAD_ICON_SVG}</span>
      <span class="odm-badge-label">Download this video</span>
      <span class="odm-badge-close" title="Hide">&times;</span>
    `;
    el.addEventListener("click", (e) => {
      if (e.target.closest(".odm-badge-close")) {
        hiddenFor = currentVideo;
        el.style.display = "none";
        return;
      }
      requestDownload(el);
    });
    document.documentElement.appendChild(el);
    console.log("[ODM] badge created for video element", currentVideo);
    return el;
  }

  function requestDownload(el) {
    // If the extension was reloaded (e.g. during development) after this
    // page loaded, `chrome.runtime` is torn down and calling into it throws
    // instead of erroring gracefully -- detect that and ask for a refresh
    // rather than crashing.
    if (!chrome?.runtime?.id) {
      setBadgeState(el, "error", "extension was reloaded");
      console.warn("[ODM] extension context invalidated -- refresh this page to reconnect.");
      setTimeout(() => setBadgeState(el, "idle"), 3000);
      return;
    }
    setBadgeState(el, "sending");
    chrome.runtime.sendMessage({ type: "downloadDetectedVideo", pageUrl: location.href }, (response) => {
      if (chrome.runtime.lastError) {
        console.error("[ODM] sendMessage failed:", chrome.runtime.lastError.message);
        setBadgeState(el, "error", chrome.runtime.lastError.message);
        setTimeout(() => setBadgeState(el, "idle"), 2500);
        return;
      }
      if (response?.ok) {
        setBadgeState(el, "sent");
      } else {
        console.warn("[ODM] download request failed:", response?.error);
        setBadgeState(el, "error", response?.error);
      }
      setTimeout(() => setBadgeState(el, "idle"), 2500);
    });
  }

  function setBadgeState(el, state, message) {
    el.classList.remove("odm-state-sending", "odm-state-sent", "odm-state-error");
    const label = el.querySelector(".odm-badge-label");
    if (state === "sending") {
      el.classList.add("odm-state-sending");
      label.textContent = "Sending...";
    } else if (state === "sent") {
      el.classList.add("odm-state-sent");
      label.textContent = "Added to ODM";
    } else if (state === "error") {
      el.classList.add("odm-state-error");
      label.textContent = message ? "Failed - see popup" : "Failed";
    } else {
      label.textContent = "Download this video";
    }
  }

  function positionBadge() {
    if (!badge || !currentVideo || !currentVideo.isConnected) return;
    if (hiddenFor === currentVideo) {
      badge.style.display = "none";
      return;
    }
    const rect = currentVideo.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) {
      badge.style.display = "none";
      return;
    }
    badge.style.display = "flex";
    badge.style.top = `${Math.max(rect.top + 8, 8)}px`;
    badge.style.left = `${Math.max(rect.right - badge.offsetWidth - 8, 8)}px`;
  }

  function tick() {
    try {
      const best = biggestVisibleVideo();
      if (best !== currentVideo) {
        currentVideo = best;
        if (currentVideo && !badge) {
          badge = createBadge();
        }
        if (currentVideo && hiddenFor && hiddenFor !== currentVideo) {
          hiddenFor = null; // a different/new video started - show the badge again
        }
      }
      if (currentVideo && badge) {
        positionBadge();
      } else if (badge) {
        badge.style.display = "none";
      }
    } catch (err) {
      console.error("[ODM] content script tick error:", err);
    }
    rafId = requestAnimationFrame(tick);
  }

  tick();

  window.addEventListener("beforeunload", () => {
    if (rafId) cancelAnimationFrame(rafId);
  });
})();
