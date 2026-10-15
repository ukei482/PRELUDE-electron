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
  wallpaper: () => ipcRenderer.invoke('wallpaper:get'),
  appIcon: (name) => ipcRenderer.invoke('icon:get', name),
  win: {
    cmd: (c) => ipcRenderer.send('win:cmd', c),
    state: () => ipcRenderer.invoke('win:state'),
    onState: (cb) => on('win:state', cb),
    screens: () => ipcRenderer.invoke('win:screens'),
    onScreens: (cb) => on('win:screens', cb),
  },
  // タブを別の画面の窓へ移す(Webページ・取り込んだアプリはそのまま)
  tab: {
    move: (to, data) => ipcRenderer.send('tab:move', to, data),
    evacuated: () => ipcRenderer.send('tab:evacuated'),
    onAdopt: (cb) => on('tab:adopt', cb),
    onEvacuate: (cb) => on('tab:evacuate', cb),
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
    onToChromium: (cb) => on('web:to-chromium', cb),
  },
  app: {
    supported: () => ipcRenderer.invoke('app:supported'),
    list: () => ipcRenderer.invoke('app:list'),
    browserCmd: (url, cls) => ipcRenderer.invoke('app:browser-cmd', url, cls),
    launch: (id, cmd, cls) => ipcRenderer.invoke('app:launch', id, cmd, cls),
    bounds: (id, rect) => ipcRenderer.send('app:bounds', id, rect),
    close: (id) => ipcRenderer.send('app:close', id),
    spawn: (cmd) => ipcRenderer.invoke('app:spawn', cmd),
    onExited: (cb) => on('app:exited', cb),
    onTitle: (cb) => on('app:title', cb),
  },
  sys: {
    status: () => ipcRenderer.invoke('sys:status'),
    audio: (cmd) => ipcRenderer.invoke('sys:audio', cmd),
    onStatus: (cb) => on('sys:status', cb),
    media: () => ipcRenderer.invoke('sys:media'),
    onMedia: (cb) => on('sys:media', cb),
    mediaCmd: (c) => ipcRenderer.invoke('media:cmd', c),
  },
  tray: {
    items: () => ipcRenderer.invoke('tray:items'),
    onItems: (cb) => on('tray:items', cb),
    menu: (id) => ipcRenderer.invoke('tray:menu', id),
    menuClick: (id, menuId) => ipcRenderer.send('tray:menu-click', id, menuId),
    activate: (id, x, y) => ipcRenderer.send('tray:activate', id, x, y),
    secondary: (id, x, y) => ipcRenderer.send('tray:secondary', id, x, y),
  },
  notify: {
    state: () => ipcRenderer.invoke('notify:state'),
    onState: (cb) => on('notify:state', cb),
    seen: () => ipcRenderer.send('notify:seen'),
    dnd: (v) => ipcRenderer.send('notify:dnd', !!v),
    remove: (id) => ipcRenderer.send('notify:remove', id),
    clear: () => ipcRenderer.send('notify:clear'),
  },
  quick: {
    get: () => ipcRenderer.invoke('quick:get'),
    do: (op, arg) => ipcRenderer.invoke('quick:do', op, arg),
  },
  session: {
    state: () => ipcRenderer.invoke('session:state'),
    returnToKde: () => ipcRenderer.invoke('session:return'),
  },
  update: {
    check: () => ipcRenderer.invoke('update:check'),
    run: () => ipcRenderer.invoke('update:run'),
  },
  fs: {
    list: (dir, showHidden) => ipcRenderer.invoke('fs:list', dir, showHidden),
    stats: (dir, names) => ipcRenderer.invoke('fs:stats', dir, names),
    watch: (id, dir) => ipcRenderer.send('fs:watch', id, dir),
    unwatch: (id) => ipcRenderer.send('fs:unwatch', id),
    onChanged: (cb) => on('fs:changed', cb),
    onProgress: (cb) => on('fs:progress', cb),
    open: (p) => ipcRenderer.invoke('fs:open', p),
    home: () => ipcRenderer.invoke('fs:home'),
    drives: () => ipcRenderer.invoke('fs:drives'),
    mkdir: (dir, name) => ipcRenderer.invoke('fs:mkdir', dir, name),
    rename: (full, name) => ipcRenderer.invoke('fs:rename', full, name),
    remove: (paths, permanent) => ipcRenderer.invoke('fs:remove', paths, permanent),
    transfer: (mode, paths, destDir) => ipcRenderer.invoke('fs:transfer', mode, paths, destDir),
    clipWrite: (paths, cut) => ipcRenderer.invoke('fs:clip-write', paths, cut),
    clipRead: () => ipcRenderer.invoke('fs:clip-read'),
    copyText: (s) => ipcRenderer.invoke('fs:copy-text', s),
    terminal: (dir) => ipcRenderer.invoke('fs:terminal', dir),
    terminalCmd: (dir) => ipcRenderer.invoke('fs:terminal-cmd', dir),
    props: (paths) => ipcRenderer.invoke('fs:props', paths),
    dirSize: (paths) => ipcRenderer.invoke('fs:dirsize', paths),
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
