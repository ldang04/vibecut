import { app, BrowserWindow, dialog, ipcMain } from 'electron';
import { spawn } from 'child_process';
import * as path from 'path';
import * as fs from 'fs';
import { fileURLToPath } from 'url';
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
let mainWindow = null;
let daemonProcess = null;
const isDev = process.env.NODE_ENV === 'development' || !app.isPackaged;
function createWindow() {
    const preloadPath = path.join(__dirname, 'preload.cjs');
    mainWindow = new BrowserWindow({
        width: 1200,
        height: 800,
        backgroundColor: '#1a1a1a', // Match app background color
        webPreferences: {
            nodeIntegration: false,
            contextIsolation: true,
            webSecurity: false, // Disable web security to allow localhost CORS requests
            preload: preloadPath,
        },
    });
    if (isDev) {
        // In development, load from Vite dev server
        mainWindow.loadURL('http://localhost:5173');
        mainWindow.webContents.openDevTools();
    }
    else {
        // In production, load from built files
        mainWindow.loadFile(path.join(__dirname, '../dist/index.html'));
    }
    mainWindow.on('closed', () => {
        mainWindow = null;
    });
}
function startDaemon() {
    if (!isDev) {
        // In production, daemon should be bundled with the app
        // For now, we'll require it to be running manually
        console.log('Production mode: daemon should be running separately');
        return;
    }
    // In development, spawn the daemon process
    // Try to find the daemon binary relative to the workspace root
    // From dist-electron/electron.js: up 3 levels to workspace root (vibecut/)
    const workspaceRoot = path.resolve(__dirname, '../../../');
    const daemonPath = path.join(workspaceRoot, 'target', 'debug', 'daemon');
    console.log(`[Daemon] Workspace root: ${workspaceRoot}`);
    console.log(`[Daemon] Daemon path: ${daemonPath}`);
    // Check if daemon binary exists
    if (!fs.existsSync(daemonPath)) {
        console.warn(`[Daemon] Binary not found at ${daemonPath}`);
        console.warn('[Daemon] Please build the daemon first: cargo build --bin daemon');
        return;
    }
    console.log(`[Daemon] Starting daemon from ${daemonPath}`);
    daemonProcess = spawn(daemonPath, [], {
        stdio: 'inherit',
        env: process.env,
        cwd: workspaceRoot,
    });
    daemonProcess.on('error', (err) => {
        console.error('Failed to start daemon:', err);
        console.log('Make sure the daemon is built: cargo build --bin daemon');
    });
    daemonProcess.on('exit', (code, signal) => {
        console.log(`Daemon process exited with code ${code} and signal ${signal}`);
    });
}
// Register IPC handlers for dialog
function setupIpcHandlers() {
    ipcMain.handle('dialog:openFiles', async (_event, options) => {
        const result = await dialog.showOpenDialog(mainWindow, {
            properties: options?.multiSelect ? ['openFile', 'multiSelections'] : ['openFile'],
            filters: options?.filters || [
                {
                    name: 'Video Files',
                    extensions: ['mp4', 'mov', 'avi', 'mkv', 'webm', 'm4v', 'flv', 'wmv', 'mpg', 'mpeg', '3gp'],
                },
                { name: 'All Files', extensions: ['*'] },
            ],
            title: options?.title || 'Select Files',
        });
        if (result.canceled) {
            return null;
        }
        return result.filePaths;
    });
    ipcMain.handle('dialog:openFolder', async (_event, options) => {
        const result = await dialog.showOpenDialog(mainWindow, {
            properties: ['openDirectory'],
            title: options?.title || 'Select Folder',
        });
        if (result.canceled) {
            return null;
        }
        return result.filePaths[0] || null;
    });
}
app.whenReady().then(() => {
    setupIpcHandlers();
    startDaemon();
    createWindow();
    app.on('activate', () => {
        if (BrowserWindow.getAllWindows().length === 0) {
            createWindow();
        }
    });
});
app.on('window-all-closed', () => {
    if (daemonProcess) {
        daemonProcess.kill();
        daemonProcess = null;
    }
    if (process.platform !== 'darwin') {
        app.quit();
    }
});
app.on('before-quit', () => {
    if (daemonProcess) {
        daemonProcess.kill();
        daemonProcess = null;
    }
});
