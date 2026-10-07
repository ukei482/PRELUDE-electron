'use strict';
// オーバーレイ窓(トーストなど)用の最小のAPI
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('overlayApi', {
  theme: () => ipcRenderer.invoke('overlay:theme'),
  toasts: () => ipcRenderer.invoke('notify:toasts'),
  onToasts: (cb) => ipcRenderer.on('notify:toasts', (_e, list) => cb(list)),
  size: (kind, h) => ipcRenderer.send('overlay:size', kind, h),
  action: (id, key) => ipcRenderer.send('toast:action', id, key),
  dismiss: (id) => ipcRenderer.send('toast:dismiss', id),
});
