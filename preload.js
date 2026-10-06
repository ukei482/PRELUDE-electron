'use strict';
const { contextBridge, ipcRenderer } = require('electron');

const on = (channel, cb) => {
  const h = (_e, payload) => cb(payload);
  ipcRenderer.on(channel, h);
  return () => ipcRenderer.removeListener(channel, h);
};

contextBridge.exposeInMainWorld('preludeApi', {
  config: {
    get: () => ipcRenderer.invoke('config:get'),
    set: (p, v) => ipcRenderer.invoke('config:set', p, v),
    reset: () => ipcRenderer.invoke('config:reset'),
    onChanged: (cb) => on('config:changed', cb),
  },
  icons: () => ipcRenderer.invoke('icons:get'),
  win: {
    cmd: (c) => ipcRenderer.send('win:cmd', c),
    state: () => ipcRenderer.invoke('win:state'),
    onState: (cb) => on('win:state', cb),
  },
  web: {
    create: (id, url) => ipcRenderer.send('web:create', id, url),
    bounds: (id, rect) => ipcRenderer.send('web:bounds', id, rect),
    cmd: (id, cmd, arg) => ipcRenderer.send('web:cmd', id, cmd, arg),
    destroy: (id) => ipcRenderer.send('web:destroy', id),
    suspend: (v) => ipcRenderer.send('web:suspend', v),
    onState: (cb) => on('web:state', cb),
    onFavicon: (cb) => on('web:favicon', cb),
    onFocused: (cb) => on('web:focused', cb),
    onOpenRequest: (cb) => on('web:open-request', cb),
  },
  fs: {
    list: (dir, showHidden) => ipcRenderer.invoke('fs:list', dir, showHidden),
    open: (p) => ipcRenderer.invoke('fs:open', p),
    home: () => ipcRenderer.invoke('fs:home'),
    drives: () => ipcRenderer.invoke('fs:drives'),
  },
  dl: {
    choose: (id, dir) => ipcRenderer.invoke('dl:choose', id, dir),
    cancel: (id) => ipcRenderer.invoke('dl:cancel', id),
    onStart: (cb) => on('dl:start', cb),
    onProgress: (cb) => on('dl:progress', cb),
    onDone: (cb) => on('dl:done', cb),
    onSaved: (cb) => on('dl:saved', cb),
    onError: (cb) => on('dl:error', cb),
  },
  ws: {
    load: () => ipcRenderer.invoke('ws:load'),
    save: (data) => ipcRenderer.send('ws:save', data),
  },
  onShortcut: (cb) => on('shortcut', cb),
});
