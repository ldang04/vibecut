import { contextBridge, ipcRenderer } from 'electron';

// Expose protected methods that allow the renderer process to use
// the dialog API without exposing the entire Electron API
contextBridge.exposeInMainWorld('electron', {
  dialog: {
    openFiles: async (options?: {
      multiSelect?: boolean;
      filters?: { name: string; extensions: string[] }[];
      title?: string;
    }): Promise<string[] | null> => {
      const result = await ipcRenderer.invoke('dialog:openFiles', options);
      return result;
    },
    openFolder: async (options?: {
      title?: string;
    }): Promise<string | null> => {
      const result = await ipcRenderer.invoke('dialog:openFolder', options);
      return result;
    },
  },
});

