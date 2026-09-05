const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const fs = require('node:fs');

// Run the real content script, including menu and download click handlers.
function page(chrome) {
  const elements = [];
  function element() {
    const el = {
      style: {}, children: [], listeners: {}, className: '', textContent: '',
      offsetWidth: 200, offsetHeight: 100,
      appendChild(child) { this.children.push(child); },
      addEventListener(type, fn) { this.listeners[type] = fn; },
      getBoundingClientRect() { return { top: 0, left: 0, right: 640, bottom: 360, width: 640, height: 360 }; },
      querySelector() { return this.label ||= { textContent: '' }; },
      contains() { return false; },
      click() { this.listeners.click({ target: { closest() { return false; } }, stopPropagation() {} }); },
    };
    el.classList = {
      contains(name) { return el.className.split(' ').includes(name); },
      add(name) { el.className += ' ' + name; },
      remove(...names) { el.className = el.className.split(' ').filter(n => !names.includes(n)).join(' '); },
    };
    Object.defineProperty(el, 'innerHTML', { set(value) { this.children = []; this.html = value; } });
    elements.push(el);
    return el;
  }
  const video = Object.assign(element(), { isConnected: true, currentSrc: 'https://video.fbcdn.net/test.mp4' });
  const context = {
    chrome, console: { log() {}, warn() {}, error() {} },
    location: { href: 'https://www.facebook.com/reel/123', hostname: 'www.facebook.com', pathname: '/reel/123' },
    innerWidth: 1000, innerHeight: 800,
    document: { querySelectorAll: () => [video], createElement: element, documentElement: element(), addEventListener() {} },
    requestAnimationFrame() { return 1; }, cancelAnimationFrame() {},
    setTimeout() { return 1; }, clearTimeout() {}, addEventListener() {},
  };
  context.window = context;
  vm.runInNewContext(fs.readFileSync(__dirname + '/content.js', 'utf8'), context);
  return {
    context,
    badge: () => elements.find(el => el.id === 'odm-download-badge'),
    menu: () => elements.find(el => el.id === 'odm-quality-menu'),
    download() { this.menu().children.find(el => el.html?.includes('Best available')).click(); },
  };
}

test('overlay blocks repeat requests while pending and allows retry after an error', () => {
  let downloads = 0;
  let reply;
  const p = page({runtime:{id:'odm',sendMessage(message, cb) {
    if (message.type === 'getVideoQualities') cb({ok:true,heights:[1080]});
    else if (message.type === 'downloadDetectedVideo') { downloads++; reply = cb; }
    else cb([]);
  }}});
  p.badge().click();
  p.download();
  p.download();
  p.badge().click();
  assert.equal(downloads, 1);
  reply({ok:false,error:'Offline'});
  p.badge().click();
  p.download();
  assert.equal(downloads, 2);
  reply({ok:true});
});

for (const [name, chrome] of [
  ['missing chrome', undefined],
  ['missing runtime', {}],
  ['missing sendMessage', { runtime: { id: 'odm' } }],
  ['invalidated context', { runtime: { id: 'odm', sendMessage() { throw Error('Extension context invalidated.'); } } }],
]) {
  test(name + ' shows reconnect guidance without throwing', () => {
    const p = page(chrome);
    assert.doesNotThrow(() => p.badge().click());
    assert.ok(p.menu().children.some(el => el.textContent.includes('Refresh this page')));
    assert.doesNotThrow(() => p.download());
    assert.equal(p.badge().label.textContent, 'Refresh page to reconnect ODM');
  });
}

test('runtime disappearing before callback is handled safely', () => {
  let reply;
  const p = page({ runtime: { id: 'odm', sendMessage(message, cb) { reply = cb; } } });
  p.badge().click();
  p.context.chrome.runtime = undefined;
  assert.doesNotThrow(() => reply({ ok: true, heights: [1080] }));
  assert.ok(p.menu().children.some(el => el.textContent.includes('Refresh this page')));
});

test('lastError getter throwing after invalidation is handled safely', () => {
  const p = page({ runtime: { id: 'odm', get lastError() { throw Error('invalidated'); }, sendMessage(m, cb) { cb(); } } });
  assert.doesNotThrow(() => p.badge().click());
  assert.ok(p.menu().children.some(el => el.textContent.includes('Refresh this page')));
});

test('working messaging preserves selected resolution and captured URL', () => {
  let sent;
  const p = page({ runtime: { id: 'odm', sendMessage(message, cb) {
    if (message.type === 'getVideoQualities') cb({ ok: true, heights: [1080] });
    else if (message.type === 'getBrowserMedia') cb([]);
    else { sent = message; cb({ ok: true, res: { task: { video_quality: 1080 } } }); }
  } } });
  p.badge().click();
  p.menu().children.find(el => el.html?.includes('1080p')).click();
  assert.equal(sent.quality, '1080');
  assert.equal(sent.mediaUrl, 'https://video.fbcdn.net/test.mp4');
  assert.match(p.badge().label.textContent, /1080p/);
});

test('Best available sends the player fallback automatically without backup options', () => {
  const messages = [];
  const p = page({ runtime: { id: 'odm', sendMessage(message, cb) {
    messages.push(message);
    if (message.type === 'getVideoQualities') cb({ ok: false, error: 'Site extraction failed' });
    else if (message.type === 'downloadDetectedVideo') cb({ ok: true });
    else cb([{ url: 'https://video.fbcdn.net/unrelated-preload.mp4' }]);
  } } });
  p.badge().click();
  assert.equal(p.menu().children.filter(el => el.className === 'odm-quality-option').length, 1);
  assert.ok(!p.menu().children.some(el => /backup|quality unknown/i.test(el.textContent + (el.html || ''))));
  p.download();
  assert.ok(!messages.some(message => message.type === 'getBrowserMedia'));
  const download = messages.find(message => message.type === 'downloadDetectedVideo');
  assert.equal(download.quality, 'best');
  assert.equal(download.mediaUrl, 'https://video.fbcdn.net/test.mp4');
});

test('native host errors remain visible instead of becoming reconnect errors', () => {
  const runtime = { id: 'odm', sendMessage(message, cb) {
    this.lastError = { message: 'Specified native messaging host not found.' };
    cb();
    delete this.lastError;
  } };
  const p = page({ runtime });
  p.badge().click();
  p.download();
  assert.equal(p.badge().title, 'Specified native messaging host not found.');
});
