const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');

function worker(entries = [], nativeReply) {
  let listener;
  let sent;
  let updated;
  const event = () => ({ addListener() {} });
  const chrome = {
    runtime: {
      onInstalled: event(),
      onMessage: { addListener(fn) { listener = fn; } },
      sendNativeMessage(host, message, callback) {
        sent = message;
        callback(nativeReply ? nativeReply(message) : { ok: true, task: { video_quality: Number(message.quality) } });
      },
    },
    storage: {
      local: { get(defaults, fn) { fn(defaults); } },
      session: { async get(key) { return { [key]: entries }; }, async remove() { entries = []; } },
      onChanged: event(),
    },
    downloads: { onCreated: event() },
    contextMenus: { onClicked: event() },
    webRequest: { onHeadersReceived: event() },
    tabs: { onUpdated: { addListener(fn) { updated = fn; } }, onRemoved: event() },
    action: { async setBadgeText() {} },
  };
  vm.runInNewContext(fs.readFileSync(__dirname + '/background.js', 'utf8'), { chrome, URL, console });
  return {
    async request(message) { return new Promise(resolve => listener(message, { tab: { id: 1 }, frameId: 0 }, resolve)); },
    sent() { return sent; },
    navigate(change) { updated(1, change); },
  };
}

test('passes captured signed URL unchanged alongside the page and selected quality', async () => {
  const w = worker();
  const mediaUrl = 'https://video.fbcdn.net/clip.mp4?token=a%2Bb&expire=123';
  await w.request({ type: 'downloadDetectedVideo', pageUrl: 'https://www.facebook.com/reel/123', quality: '720', mediaUrl });
  assert.equal(w.sent().fallback_url, mediaUrl);
  assert.equal(w.sent().quality, '720');
  assert.equal(w.sent().url, 'https://www.facebook.com/reel/123');
});

test('empty and malformed native replies are rejected for download and connection checks', async () => {
  for (const reply of [undefined, null, {}, {ok:'true'}, {ok:false,error:'Rejected'}]) {
    const w = worker([], () => reply);
    assert.equal((await w.request({type:'ping'})).ok, false);
    const result = await w.request({type:'sendDownload',url:'https://example.com/file.mp4'});
    assert.equal(result.ok, false);
    assert.ok(result.error);
  }
  for (const task of [null, undefined, 'not a task', []]) {
    const w = worker([], () => ({ok:true, task}));
    assert.equal((await w.request({type:'sendDownload',url:'https://example.com/file.mp4'})).ok, false);
  }
  assert.equal((await worker([], () => ({ok:true})).request({type:'ping'})).ok, true);
});

test('single-video overlay strips playlist and Mix context but preserves video and quality', async () => {
  const w = worker();
  await w.request({type:'downloadDetectedVideo', pageUrl:'https://www.youtube.com/watch?v=abc&list=RDabc&index=3&start_radio=1', quality:'1080'});
  assert.equal(w.sent().url, 'https://www.youtube.com/watch?v=abc');
  assert.equal(w.sent().quality, '1080');
});

test('explicit playlist pasted in popup stays intact', async () => {
  const w = worker();
  const url = 'https://www.youtube.com/playlist?list=PLexample';
  await w.request({type:'sendDownload', url});
  assert.equal(w.sent().url, url);
});

test('both reload and URL-only SPA navigation clear old captures before reading', async () => {
  for (const change of [{status:'loading'}, {url:'https://www.youtube.com/watch?v=new'}]) {
    const w = worker([{url:'https://video.fbcdn.net/old.mp4'}]);
    w.navigate(change);
    const detected = await w.request({type:'getDetected', tabId:1});
    assert.equal(detected.length, 0);
  }
});

test('backup choices exclude old captures, audio-only responses and other frames', async () => {
  const fresh = { url: 'https://video.fbcdn.net/a.mp4', contentType: 'video/mp4', frameId: 0, capturedAt: Date.now() };
  const w = worker([fresh, { ...fresh, frameId: 2 }, { ...fresh, capturedAt: 1 }, { ...fresh, contentType: 'audio/mp4' }]);
  const result = await w.request({ type: 'getBrowserMedia' });
  assert.equal(result.length, 1);
  assert.equal(result[0].url, fresh.url);
});

test('YouTube audio is paired only with the same media ID and frame', async () => {
  const capturedAt = Date.now();
  const wrong = { url: 'https://r.googlevideo.com/videoplayback?id=other&mime=audio%2Fmp4', contentType: 'audio/mp4', frameId: 0, capturedAt };
  const right = { ...wrong, url: 'https://r.googlevideo.com/videoplayback?id=same&mime=audio%2Fmp4' };
  const w = worker([wrong, right]);
  await w.request({ type: 'downloadDetectedVideo', pageUrl: 'https://www.youtube.com/watch?v=abc', quality: 'best', mediaUrl: 'https://r.googlevideo.com/videoplayback?id=same&mime=video%2Fmp4' });
  assert.equal(w.sent().fallback_audio, right.url);
});
