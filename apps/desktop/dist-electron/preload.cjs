"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const electron_1 = require("electron");
// Expose protected methods that allow the renderer process to use
// the dialog API without exposing the entire Electron API
electron_1.contextBridge.exposeInMainWorld('electron', {
    dialog: {
        openFiles: async (options) => {
            const result = await electron_1.ipcRenderer.invoke('dialog:openFiles', options);
            return result;
        },
        openFolder: async (options) => {
            const result = await electron_1.ipcRenderer.invoke('dialog:openFolder', options);
            return result;
        },
    },
});
