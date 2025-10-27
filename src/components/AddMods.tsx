import { useEffect, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Store } from "@tauri-apps/plugin-store";
import { Window } from "@tauri-apps/api/window";

import { Progress } from "./ui/progress";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "./ui/alert-dialog";

interface InstallResult {
  success: boolean;
  error?: string;
  mod_name?: string;
}

interface ProgressPayload {
  current: number;
  total: number;
  message: string;
}

interface AddModsProps {
  onModsInstalled: () => void;
}

export default function AddMods({ onModsInstalled }: AddModsProps) {
  const [pendingFiles, setPendingFiles] = useState<string[]>([]);
  const [showConfirm, setShowConfirm] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState(0);
  const [progressMessage, setProgressMessage] = useState("");
  const [gtaPath, setGtaPath] = useState<string>("");

  const appWindow = new Window("main");

  useEffect(() => {
    // Load GTA path once
    (async () => {
      const store = await Store.load(".settings.dat");
      const path = await store.get<string>("gta-path");
      if (path) setGtaPath(path);
    })();

    const win = getCurrentWebviewWindow();

    // Listen for drag/drop events
    const dropUnlisten = win.onDragDropEvent((event) => {
      if (event.payload.type === "drop") {
        const files = event.payload.paths as string[];
        const supported = files.filter(
          (f) => f.toLowerCase().endsWith(".zip") || f.toLowerCase().endsWith(".rar")
        );
        
        if (supported.length > 0) {
          // Store files and show confirmation dialog
          setPendingFiles(supported);
          setShowConfirm(true);
        } else if (files.length > 0) {
          alert("Unsupported file type. Only .zip and .rar are supported.");
        }
      }
    });

    // Listen for progress events from backend
    const progressUnlisten = listen<ProgressPayload>("install-progress", (event) => {
      const { current, total, message } = event.payload;
      setProgress((current / total) * 100);
      setProgressMessage(message);
    });

    return () => {
      dropUnlisten.then((fn) => fn());
      progressUnlisten.then((fn) => fn());
    };
  }, []);

  const handleInstall = async () => {
    if (!gtaPath) {
      alert("GTA path not set. Please configure it in settings.");
      setShowConfirm(false);
      setPendingFiles([]);
      return;
    }

    // Close dialog and start installation
    setShowConfirm(false);
    setInstalling(true);
    setProgress(0);

    for (const file of pendingFiles) {
      try {
        appWindow.setTitle("GTA V Mod Manager - Installing Mods...");
        const result = await invoke<InstallResult>("install_mod_archive_async", {
          path: file,
          gtaPath: gtaPath,
        });
        
        if (result.success) {
          console.log(`Successfully installed: ${result.mod_name || file}`);
        } else {
          alert(`Failed to install ${file}: ${result.error || "Unknown error"}`);
        }
        appWindow.setTitle("GTA V Mod Manager");
      } catch (err) {
        alert(`Error installing ${file}: ${err}`);
      }
    }

    // Reset state
    setInstalling(false);
    setPendingFiles([]);
    setProgress(0);
    setProgressMessage("");
    
    onModsInstalled();
  };

  const handleCancel = () => {
    setShowConfirm(false);
    setPendingFiles([]);
  };

  return (
    <>
      {/* Drop zone visual feedback */}
      <div
        style={{
          border: installing ? "2px solid var(--primary)" : "2px dashed #ccc",
          padding: 32,
          margin: 16,
          borderRadius: 12,
          textAlign: "center",
          marginBottom: 0,
          backgroundColor: installing ? "rgba(59, 130, 246, 0.05)" : "transparent",
        }}
        className="flex flex-col items-center justify-center gap-0"
      >
        <b>
          {installing 
            ? "Installing mods..." 
            : "Drag & drop mod ZIPs or RARs anywhere to install"}
        </b>
        <span style={{ fontSize: "0.9em", opacity: 0.7 }}>
          Fast installation
        </span>
      </div>

      {/* Progress indicator */}
      {installing && (
        <div className="p-4 border rounded-lg mb-4">
          <Progress value={progress} className="mb-2" />
          <p className="text-sm text-muted-foreground">{progressMessage}</p>
        </div>
      )}

      {/* Confirmation dialog */}
      <AlertDialog open={showConfirm} onOpenChange={setShowConfirm}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Confirm Mod Installation</AlertDialogTitle>
            <AlertDialogDescription>
              Install {pendingFiles.length} mod{pendingFiles.length > 1 ? "s" : ""}?
              <ul className="mt-2 text-xs space-y-1">
                {pendingFiles.map((f, i) => (
                  <li key={i} className="truncate">
                    📦 {f.split(/[\\/]/).pop()}
                  </li>
                ))}
              </ul>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={handleCancel}>
              Cancel
            </AlertDialogCancel>
            <AlertDialogAction onClick={handleInstall}>
              Install Now
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
