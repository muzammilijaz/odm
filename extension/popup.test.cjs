const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

function popup(sendMessage = async () => { throw Error('Message port closed'); }) {
  const elements = new Map();
  const getElementById = id => {
    if (!elements.has(id)) elements.set(id, {
      value:'', textContent:'', className:'', classList:{add(){},remove(){}},
      querySelector(){return {textContent:''};},
      addEventListener(type, callback){this[type] = callback;},
    });
    return elements.get(id);
  };
  const context = {
    URL, document:{getElementById},
    chrome:{
      runtime:{sendMessage,getManifest(){return {version:'1.1.0'};}},
      tabs:{async query(){return [{id:1}];}},
      storage:{local:{get(defaults,cb){cb(defaults);},async set(){}}},
    },
  };
  vm.runInNewContext(fs.readFileSync(__dirname + '/popup.js','utf8'),context);
  return {getElementById};
}

test('popup handles rejected messaging and retains the URL for retry', async () => {
  const {getElementById} = popup();
  await new Promise(resolve => setImmediate(resolve));
  getElementById('add-url').value = 'https://example.com/video.mp4';
  await getElementById('add-form').submit({preventDefault(){}});
  assert.equal(getElementById('add-url').value, 'https://example.com/video.mp4');
  assert.match(getElementById('status').textContent, /Could not connect.*Message port closed/);
  assert.equal(getElementById('status').className, 'status error');
});

test('rapid repeat submissions send once; successful or failed requests unlock retry', async () => {
  let calls = 0;
  let reply;
  const {getElementById: el} = popup(message => {
    if (message.type !== 'sendDownload') return Promise.resolve(message.type === 'ping' ? {ok:true} : []);
    calls++;
    return new Promise(resolve => { reply = resolve; });
  });
  await new Promise(resolve => setImmediate(resolve));
  const submit = () => el('add-form').submit({preventDefault(){}});
  el('add-url').value = 'https://example.com/video.mp4';
  const first = submit();
  await submit();
  assert.equal(calls, 1);
  reply({ok:false,error:'Offline'});
  await first;
  assert.equal(el('add-url').value,'https://example.com/video.mp4');
  const retry = submit();
  assert.equal(calls, 2);
  el('add-url').value = 'https://example.com/next.mp4';
  reply({ok:true});
  await retry;
  assert.equal(el('add-url').value,'https://example.com/next.mp4');
  el('add-url').value = 'https://example.com/video.mp4';
  const intentionalCopy = submit();
  assert.equal(calls, 3);
  reply({ok:true});
  await intentionalCopy;
  assert.equal(el('add-url').value,'');
});
