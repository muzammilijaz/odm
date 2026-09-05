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
  let qualityMenu = null;
  let hiddenFor = null; // video element the user explicitly closed the badge for
  let rafId = null;
  let probeGeneration = 0;
  let probeTimer = null;
  let downloadPending = false;

  const RECONNECT_MESSAGE = "Refresh this page to reconnect ODM. If needed, enable or reload the extension first.";

  // A page can outlive its extension context after an update/reload. Guard
  // both the call and its asynchronous callback, not just runtime.id.
  function sendToBackground(message, callback) {
    let settled = false;
    const finish = (response) => {
      if (settled) return;
      settled = true;
      callback(response);
    };
    try {
      const runtime = globalThis.chrome?.runtime;
      if (!runtime?.id || typeof runtime.sendMessage !== "function") {
        finish({ ok: false, error: RECONNECT_MESSAGE });
        return;
      }
      runtime.sendMessage(message, (response) => {
        let error;
        try {
          // Read lastError inside the callback to acknowledge Chrome errors.
          error = runtime.lastError?.message;
          if (!globalThis.chrome?.runtime?.id || /context invalidated/i.test(error || "")) {
            error = RECONNECT_MESSAGE;
          }
        } catch {
          error = RECONNECT_MESSAGE;
        }
        finish(error ? { ok: false, error } : response);
      });
    } catch {
      finish({ ok: false, error: RECONNECT_MESSAGE });
    }
  }

  function qualityLabel(height) {
    if (height === 2160) return "2160p (4K)";
    if (height === 1440) return "1440p (2K)";
    return `${height}p`;
  }

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
      const area = Math.max(0, Math.min(rect.right, innerWidth) - Math.max(rect.left, 0)) *
        Math.max(0, Math.min(rect.bottom, innerHeight) - Math.max(rect.top, 0));
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
      <span class="odm-badge-chevron">▾</span>
      <span class="odm-badge-close" title="Hide">&times;</span>
    `;
    el.addEventListener("click", (e) => {
      if (e.target.closest(".odm-badge-close")) {
        hiddenFor = currentVideo;
        el.style.display = "none";
        closeQualityMenu();
        return;
      }
      toggleQualityMenu(el);
    });
    document.documentElement.appendChild(el);
    console.log("[ODM] badge created for video element", currentVideo);
    return el;
  }

  function toggleQualityMenu(el) {
    if (downloadPending) return;
    if (qualityMenu?.classList.contains("odm-quality-menu--open")) {
      closeQualityMenu();
      return;
    }
    if (!qualityMenu) {
      qualityMenu = document.createElement("div");
      qualityMenu.id = "odm-quality-menu";
      qualityMenu.addEventListener("click", (event) => event.stopPropagation());
      document.documentElement.appendChild(qualityMenu);
    }
    qualityMenu.className = "odm-quality-menu--open";
    qualityMenu.innerHTML = '<div class="odm-quality-loading">Checking available resolutions…</div>';
    positionQualityMenu();

    const generation = ++probeGeneration;
    const pageUrl = location.href;
    const finish = (response) => {
      if (generation !== probeGeneration || pageUrl !== location.href || !qualityMenu?.classList.contains("odm-quality-menu--open")) return;
      clearTimeout(probeTimer);
      ++probeGeneration;
      const probed = Array.isArray(response?.heights) ? response.heights.filter((h) => Number.isInteger(h) && h > 0) : [];
      renderQualityMenu(el, [...new Set(probed)].sort((a, b) => b - a), !response?.ok);
      if (!response?.ok) {
        const notice = document.createElement("div");
        notice.className = "odm-quality-heading";
        notice.textContent = response?.error || "Could not inspect qualities. Try Best available; sign-in may be required in ODM Settings.";
        qualityMenu.appendChild(notice);
      }
      positionQualityMenu();
    };
    probeTimer = setTimeout(() => finish({ ok: false, error: "Quality check timed out. Try Best available or retry." }), 22000);
    sendToBackground({ type: "getVideoQualities", pageUrl }, finish);
  }

  function renderQualityMenu(el, heights, usedFallback) {
    qualityMenu.innerHTML = "";
    const heading = document.createElement("div");
    heading.className = "odm-quality-heading";
    heading.textContent = usedFallback ? "Choose quality (availability checked at download)" : "Choose video quality";
    qualityMenu.appendChild(heading);

    const options = [{ value: "best", label: "Best available" }, ...heights.map((height) => ({ value: String(height), label: qualityLabel(height) }))];
    for (const option of options) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "odm-quality-option";
      button.innerHTML = `<span>${option.label}</span><span class="odm-quality-format">MP4</span>`;
      button.addEventListener("click", () => {
        closeQualityMenu();
        requestDownload(el, option.value, option.label);
      });
      qualityMenu.appendChild(button);
    }
  }

  function closeQualityMenu() {
    ++probeGeneration;
    clearTimeout(probeTimer);
    qualityMenu?.classList.remove("odm-quality-menu--open");
  }

  function requestDownload(el, quality, label) {
    if (downloadPending) return;
    downloadPending = true;
    setBadgeState(el, "sending");
    // The desktop automatically tries this player's stream if extraction
    // fails. Never substitute an arbitrary feed preload from the tab.
    const mediaUrl = currentVideo?.currentSrc || currentVideo?.src || "";
    sendToBackground({ type: "downloadDetectedVideo", pageUrl: location.href, quality, selectedQuality: quality, mediaUrl }, (response) => {
      downloadPending = false;
      if (response?.ok) {
        const acceptedQuality = response?.res?.task?.video_quality;
        if (quality !== "best" && Number(acceptedQuality) !== Number(quality)) {
          const message = `ODM received ${acceptedQuality ? `${acceptedQuality}p` : "Best"} instead of ${label}. Reload the extension and page.`;
          console.warn("[ODM] quality was not preserved:", { requested: quality, accepted: acceptedQuality });
          setBadgeState(el, "error", message);
          setTimeout(() => setBadgeState(el, "idle"), 5000);
          return;
        }
        setBadgeState(el, "sent", label);
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
      label.textContent = message ? `Added · ${message}` : "Added to ODM";
    } else if (state === "error") {
      el.classList.add("odm-state-error");
      label.textContent = message === RECONNECT_MESSAGE ? "Refresh page to reconnect ODM" : "Download failed";
      el.title = message || "Download failed";
    } else {
      label.textContent = "Download this video";
    }
  }

  function positionQualityMenu() {
    if (!qualityMenu?.classList.contains("odm-quality-menu--open") || !badge) return;
    const rect = badge.getBoundingClientRect();
    qualityMenu.style.top = `${Math.min(rect.bottom + 6, window.innerHeight - qualityMenu.offsetHeight - 8)}px`;
    qualityMenu.style.left = `${Math.max(8, Math.min(rect.right - qualityMenu.offsetWidth, window.innerWidth - qualityMenu.offsetWidth - 8))}px`;
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
    positionQualityMenu();
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

  document.addEventListener("click", (event) => {
    if (qualityMenu?.classList.contains("odm-quality-menu--open") && !badge?.contains(event.target) && !qualityMenu.contains(event.target)) {
      closeQualityMenu();
    }
  });

  window.addEventListener("beforeunload", () => {
    if (rafId) cancelAnimationFrame(rafId);
  });
})();
