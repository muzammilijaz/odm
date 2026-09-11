const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');

function worker(entries = [], nativeReply, overrides = {}) {
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
        chrome.runtime.lastError = typeof overrides.nativeLastError === 'function' ? overrides.nativeLastError(message) : overrides.nativeLastError;
        callback(nativeReply ? nativeReply(message) : { ok: true, task: { video_quality: Number(message.quality) } });
        delete chrome.runtime.lastError;
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
  vm.runInNewContext(fs.readFileSync(__dirname + '/background.js', 'utf8'), { chrome, URL, console, setTimeout, clearTimeout, AbortController, ...overrides });
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

test('Instagram profile/feed pages are not probed as single videos', async () => {
  let probes = 0;
  const w = worker([], message => {
    if (message.action === 'probe_video') probes++;
    return { ok: true, qualities: { heights: [1080] } };
  });
  const reply = await w.request({ type: 'getVideoQualities', pageUrl: 'https://www.instagram.com/example/?hl=en' });
  assert.equal(reply.ok, true);
  assert.equal(reply.heights.length, 0);
  assert.equal(probes, 0);
});

test('TikTok feed pages are not probed as single videos', async () => {
  let probes = 0;
  const w = worker([], message => {
    if (message.action === 'probe_video') probes++;
    return { ok: true, qualities: { heights: [1080] } };
  });
  const reply = await w.request({ type: 'getVideoQualities', pageUrl: 'https://www.tiktok.com/?lang=en' });
  assert.equal(reply.ok, true);
  assert.equal(reply.heights.length, 0);
  assert.equal(probes, 0);
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

test('broken native helper automatically connects locally and preserves download options', async () => {
  const calls = [];
  const w = worker([], undefined, {
    nativeLastError: {message:'Error when communicating with the native messaging host.'},
    async fetch(url, options) {
      calls.push({url, options});
      return {ok:true, async json() {
        return url.endsWith('/api/health') ? {app:'com.odm.app',protocol:1} : {id:42,video_quality:720};
      }};
    },
  });
  assert.equal((await w.request({type:'ping'})).ok, true);
  const reply = await w.request({type:'downloadDetectedVideo', pageUrl:'https://youtube.com/watch?v=abc', quality:'720', mediaUrl:'https://r.googlevideo.com/video'});
  assert.equal(reply.ok, true);
  const writes = calls.filter(call => call.options.method === 'POST');
  assert.equal(writes.length, 1);
  const body = JSON.parse(writes[0].options.body);
  assert.equal(body.quality, '720');
  assert.equal(body.fallback_url, 'https://r.googlevideo.com/video');
  assert.equal(writes[0].options.redirect, 'error');
});

test('unrelated local server never receives a download', async () => {
  let posts = 0;
  const w = worker([], () => { throw Error('Native helper unavailable'); }, {
    async fetch(url, options) {
      if (options.method === 'POST') posts++;
      return {ok:true, async json() {return {app:'another-app',protocol:1};}};
    },
  });
  assert.equal((await w.request({type:'sendDownload',url:'https://example.com/file.zip'})).ok,false);
  assert.equal(posts,0);
});

test('a lost native download response is never retried via localhost', async () => {
  let posts = 0;
  let localCalls = 0;
  const w = worker([], message => {
    if (message.action === 'ping') return {ok:true};
    posts++;
    throw Error('Pipe closed after acceptance');
  }, {async fetch() {localCalls++; throw Error('Must not retry');}});
  assert.equal((await w.request({type:'sendDownload',url:'https://example.com/file.zip'})).ok,false);
  assert.equal(posts,1);
  assert.equal(localCalls,0);
});

test('a lost local download response is not replayed', async () => {
  let posts = 0;
  const w = worker([], () => {throw Error('Native helper unavailable');}, {
    async fetch(url, options) {
      if (options.method === 'POST') {posts++; throw Error('Response lost');}
      return {ok:true,async json(){return {app:'com.odm.app',protocol:1};}};
    },
  });
  const reply = await w.request({type:'sendDownload',url:'https://example.com/file.zip'});
  assert.equal(reply.ok,false);
  assert.match(reply.error,/Check the download list/);
  assert.equal(posts,1);
});

test('quality lookup automatically uses the local connection', async () => {
  let request;
  const w = worker([], undefined, {
    nativeLastError: {message:'Specified native messaging host not found.'},
    async fetch(url, options) {
      if (options.method === 'POST') request = {url, body:JSON.parse(options.body)};
      return {ok:true,async json(){
        return url.endsWith('/api/health') ? {app:'com.odm.app',protocol:1} : {title:'Video',heights:[1080,720]};
      }};
    },
  });
  const reply = await w.request({type:'getVideoQualities',pageUrl:'https://youtube.com/watch?v=abc&list=RDabc'});
  assert.equal(reply.ok,true);
  assert.equal(JSON.stringify(reply.heights),'[1080,720]');
  assert.equal(request.url,'http://127.0.0.1:38019/api/video-qualities');
  assert.equal(request.body.url,'https://youtube.com/watch?v=abc');
});

test('both connections unavailable reports offline without sending a download', async () => {
  let requests = 0;
  const w = worker([], undefined, {
    nativeLastError: {message:'Error when communicating with the native messaging host.'},
    async fetch(url, options) { requests++; assert.equal(options.method,'GET'); throw Error('Connection refused'); },
  });
  const reply = await w.request({type:'ping'});
  assert.equal(reply.ok,false);
  assert.match(reply.error,/Open the updated ODM desktop app/);
  assert.equal(requests,1);
});

test('Store authorization or helper launch failure after ping safely switches to local transport', async () => {
  for (const error of [
    'Access to the specified native messaging host is forbidden.',
    'Specified native messaging host not found.',
    'Failed to start native messaging host.',
  ]) {
    let posts = 0;
    const w = worker([], () => ({ok:true}), {
      nativeLastError: message => message.action === 'ping' ? undefined : {message:error},
      async fetch(url, options) {
        if(options.method === 'POST') posts++;
        return {ok:true,async json(){return url.endsWith('/api/health') ? {app:'com.odm.app',protocol:1} : {id:1};}};
      },
    });
    assert.equal((await w.request({type:'sendDownload',url:'https://example.com/file.zip'})).ok,true,error);
    assert.equal(posts,1);
  }
});

test('communication failure after ping never replays a possibly accepted download', async () => {
  let localCalls = 0;
  const w = worker([], () => ({ok:true}), {
    nativeLastError: message => message.action === 'ping' ? undefined : {message:'Error when communicating with the native messaging host.'},
    async fetch() {localCalls++; throw Error('Must not retry');},
  });
  assert.equal((await w.request({type:'sendDownload',url:'https://example.com/file.zip'})).ok,false);
  assert.equal(localCalls,0);
});

test('Store and unpacked IDs remain allowed by installer and runtime registration', () => {
  const crypto = require('node:crypto');
  const manifest = JSON.parse(fs.readFileSync(__dirname + '/manifest.json','utf8'));
  const hash = crypto.createHash('sha256').update(Buffer.from(manifest.key,'base64')).digest('hex').slice(0,32);
  const devId = [...hash].map(c => String.fromCharCode(97 + parseInt(c,16))).join('');
  const host = JSON.parse(fs.readFileSync(__dirname + '/native-host-manifest/com.odm.nativehost.json','utf8'));
  const runtime = fs.readFileSync(__dirname + '/../desktop/src-tauri/src/native_messaging.rs','utf8');
  for(const id of ['lfpiggopnkjdgedghgapjnmijgckebkd', devId]) {
    const origin = `chrome-extension://${id}/`;
    assert.ok(host.allowed_origins.includes(origin),origin);
    assert.ok(runtime.includes(origin),origin);
  }
});

test('a failed read-only native quality probe safely retries over loopback', async () => {
  let probes = 0;
  const w = worker([], () => ({ok:true}), {
    nativeLastError: message => message.action === 'ping' ? undefined : {message:'Native host has exited.'},
    async fetch(url, options) {
      if(options.method === 'POST') probes++;
      return {ok:true,async json(){return url.endsWith('/api/health') ? {app:'com.odm.app',protocol:1} : {title:'Video',heights:[720]};}};
    },
  });
  const reply = await w.request({type:'getVideoQualities',pageUrl:'https://youtube.com/watch?v=abc'});
  assert.equal(reply.ok,true);
  assert.equal(JSON.stringify(reply.heights),'[720]');
  assert.equal(probes,1);
});
