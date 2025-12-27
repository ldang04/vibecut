export interface ElectronDialog {
  openFiles: (options?: {
    multiSelect?: boolean;
    filters?: { name: string; extensions: string[] }[];
    title?: string;
  }) => Promise<string[] | null>;
  openFolder: (options?: {
    title?: string;
  }) => Promise<string | null>;
}

export interface ElectronAPI {
  dialog: ElectronDialog;
}

declare global {
  interface Window {
    electron: ElectronAPI;
  }
}

