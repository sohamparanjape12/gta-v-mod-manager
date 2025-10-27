import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Store } from "@tauri-apps/plugin-store";
import "./App.css";
import { Button } from "./components/ui/button";
import { ModeToggle } from "./components/mode-toggle";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "./components/ui/table";
import { Car, CircleQuestionMark, Download, ExternalLink, Loader2, Motorbike, Plane, RefreshCcw, RotateCcw, Settings, Trash2 } from "lucide-react";
import AddMods from "./components/AddMods";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "./components/ui/alert-dialog";
import { Field, FieldSeparator } from "./components/ui/field";

type ModInfo = {
  name: string;
  path: string;
  size_mb: number;
  modified: string;
  has_readme: boolean;
};

type ExtractResult = {
  success: boolean;
  error?: string | null;
  mod_name?: string | null;
  preview_image?: string | null;
  mod_type?: string | null;
};

function prettifyWindowsPath(path: string): string {
    let p = path.replace('/', '\\');
    if (p.length > 2 && p[1] === ':' && p[2] === '\\') {
        p = p[0].toUpperCase() + p.slice(1);
    }
    console.log("Prettified path:", p);
    return p;
  }


export default function Home() {
  const [gtaPath, setGtaPath] = useState<string>("");
  const [displayGtaPath, setDisplayGtaPath] = useState<string>("");
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [pathConfirmed, setPathConfirmed] = useState<boolean>(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);

  const [loading, setLoading] = useState(false);

  const [mods, setMods] = useState<ModInfo[]>([]);
  const [extractResults, setExtractResults] = useState<Record<string, ExtractResult>>({});

  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [modToDelete, setModToDelete] = useState<string | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);

  const [dynamicMaxHeight, setDynamicMaxHeight] = useState("60vh");


  useEffect(() => {
    const handleResize = () => {
      const newHeight = window.innerHeight - 290;
      setDynamicMaxHeight(`${newHeight}px`);
    };

    window.addEventListener("resize", handleResize);
    handleResize();
    return () => {
      window.removeEventListener("resize", handleResize);
    };
  }, []);

  const loadMods = async () => {
    setLoading(true);
    try {
      const result = await invoke<ModInfo[]>("list_valid_mods", { gtaPath });
      setMods(result);
      localStorage.setItem("installedMods", JSON.stringify(result));
      localStorage.setItem("modsCachedAt", Date.now().toString());
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    const prevent = (e: any) => {
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("dragover", prevent);
    window.addEventListener("drop", prevent);
    return () => {
      window.removeEventListener("dragover", prevent);
      window.removeEventListener("drop", prevent);
    };
  }, []);


  useEffect(() => {
    if (!gtaPath) return;
    // Try mods list cache first
    setDisplayGtaPath(prettifyWindowsPath(gtaPath));
    const cachedMods = localStorage.getItem("installedMods");
    if (cachedMods) {
      setMods(JSON.parse(cachedMods));
    } else {
      loadMods();
    }
  }, [gtaPath]);


  const handleExtract = async (modPath: string) => {
    const tauriRoot = ".";
    const dlcRpfPath = modPath + "/dlc.rpf";
    const result = await invoke<ExtractResult>("extract_mod_metadata", { dlcRpfPath, tauriRoot });
    setExtractResults((prev) => ({ ...prev, [modPath]: result }));

    // Save per-mod metadata to localStorage
    const storedMeta = localStorage.getItem("modExtractMeta");
    const metaObj = storedMeta ? JSON.parse(storedMeta) : {};
    metaObj[modPath] = result;
    localStorage.setItem("modExtractMeta", JSON.stringify(metaObj));
  };

  useEffect(() => {
    const metaCache = localStorage.getItem("modExtractMeta");
    if (metaCache) {
      setExtractResults(JSON.parse(metaCache));
    }
  }, []);


  const handleManualSelect = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    if (typeof selected === 'string') {
      const path = await invoke<string>("validate_gta_path", { path: selected });
      if (path) {
        const store = await Store.load(".settings.dat");
        await store.set("gta-path", path);
        await store.set("path-confirmed", false); // New path requires new confirmation
        await store.save();
        setGtaPath(path);
        setPathConfirmed(false);
      } else {
        alert("Invalid GTA V directory, GTA5.exe missing!");
      }
    }
  };

  useEffect(() => {
    const loadState = async () => {
      try {
        const store = await Store.load(".settings.dat");
        const storedPath = await store.get<string>("gta-path");
        const isConfirmed = await store.get<boolean>("path-confirmed");

        if (storedPath) {
          const validatedPath = await invoke<string>("validate_gta_path", { path: storedPath });
          if (validatedPath) {
            setGtaPath(validatedPath);
            if (isConfirmed) {
              setPathConfirmed(true);
            }
            return;
          }
        }
        
        // If we are here, the stored path is invalid or doesn't exist.
        // Let's try to auto-detect.
        const detectedPath = await invoke<string>("detect_gta_path");
        if (detectedPath) {
          setGtaPath(detectedPath);
          await store.set("gta-path", detectedPath);
          await store.set("path-confirmed", false);
          await store.save();
        }

      } catch (error) {
        console.error("Error during initial load:", error);
      } finally {
        setIsLoading(false);
      }
    };

    loadState();
  }, []);

  const handleConfirmAndSave = async () => {
    const store = await Store.load(".settings.dat");
    await store.set("path-confirmed", true);
    await store.save();
    setPathConfirmed(true);
  };

  const handlePathChange = async () => {
    const store = await Store.load(".settings.dat");
    await store.delete('gta-path');
    await store.delete('path-confirmed');
    await store.save();
    setGtaPath('');
    setPathConfirmed(false);
    localStorage.removeItem("installedMods");
    localStorage.removeItem("modsCachedAt");
  };


  const handleDeleteMod = (modPath: string) => {
    setModToDelete(modPath);
    setShowDeleteConfirm(true);
  };

  const confirmDelete = async () => {
    if (!modToDelete) return;

    setIsDeleting(true);
    const modName = modToDelete.split(/[\\/]/).pop()?.replace(".zip", "");

    try {
      const result: any = await invoke("delete_mod_async", { modName, gtaPath });
      if (result.success) {
        setMods((prev) => prev.filter((mod) => mod.path !== modToDelete));
        alert(`Mod ${modName} deleted successfully!`);
      } else {
        alert(`Failed to delete mod: ${result.error || 'Unknown error'}`);
      }
    } catch (err) {
      alert(`An error occurred while deleting the mod: ${err}`);
    } finally {
      setIsDeleting(false);
      setShowDeleteConfirm(false);
      setModToDelete(null);
    }
  };

  const formatSize = (sizeInMb: number) => {
    if (sizeInMb >= 1024) {
      return `${(sizeInMb / 1024).toFixed(2)} GB`;
    }
    return `${sizeInMb} MB`;
  };

  if (isLoading) {
    return <Loader2 className="animate-spin m-4 mx-auto" />;
  }

  if (!gtaPath) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-0 pb-10">
        <h2 className="text-xl font-bold pb-4">GTA V Path Selection</h2>
        <p className="mb-2.5">Could not find GTA V. Please select the installation directory.</p>
        <Button onClick={handleManualSelect}>Select GTA V Folder</Button>
      </div>
    );
  }

  if (!pathConfirmed) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-0 pb-10">
        <h2 className="text-xl font-bold pb-4">Confirm GTA V Path</h2>
        <p>The following GTA V installation path was detected:</p>
        <p style={{ fontFamily: '"Cutive Mono", monospace' }} className="text-lg">{displayGtaPath}</p>
        <p>Is this correct?</p>
        <div className="flex gap-4 mt-2">
          <Button onClick={handleConfirmAndSave}>Yes, Continue</Button>
          <Button onClick={handlePathChange}>No, Change Path</Button>
        </div>
      </div>
    );
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '0px' }}>
      <div style={{ display: 'flex', padding: '10px 20px 8px 20px', alignItems: 'center', justifyContent: 'space-between', }} className="border-b border-gray-300 dark:border-gray-700">
        <div>
          <div className="flex gap-2">
            <h2 className="text-2xl font-bold">GTA V Mod Manager</h2>
            <p className="flex w-fit bg-[#e0ddd5] dark:bg-gray-800 px-1 py-0 rounded-sm text-xs self-end mb-1 font-semibold">By Soham P</p>
          </div>
          <div style={{ display: 'flex', flexDirection: 'row', gap: '2px', alignItems: 'center', justifyContent: 'flex-start', padding: '0px' }}>
            <div className="bg-[#e0ddd5] dark:bg-gray-800 px-1.5 py-0 rounded-sm">
              <p style={{ fontFamily: '"Cutive Mono", monospace' }} className="text-sm">{displayGtaPath}</p>
            </div>
            <Button variant={"link"} size={"sm"} className="text-blue" onClick={handlePathChange}>Change Path</Button>
          </div>
        </div>
        <Button variant={"outline"} size={"icon"} onClick={() => {setIsSettingsOpen(true);}}>
          <Settings />
        </Button>
      </div>


      <AddMods onModsInstalled={loadMods} />


      <div className="flex flex-col p-4 gap-2">
        <div className="flex items-center gap-2.5 px-2.5">
          <h2 className="text-lg font-semibold">Installed Mods</h2>
          <Button variant={"outline"} size={"icon-sm"} onClick={loadMods}><RefreshCcw /></Button>
        </div>
        <div className="rounded-md border" style={{ maxHeight: dynamicMaxHeight, overflowY: 'auto', scrollSnapType: 'y mandatory', scrollPaddingTop: '40px' }}>
          {
            loading ? (
              <Loader2 className="animate-spin m-4 mx-auto" />
            ) : mods.length === 0 ? (
              <p className="p-4 text-center text-sm text-muted-foreground">No mods installed.</p>
            ) : (
              <Table>
                <TableHeader className="sticky top-0 bg-background border-b border-gray-300 dark:border-gray-700 shadow" style={{ position: 'sticky', top: 0, backgroundColor: 'var(--background)' }}>
                  <TableRow className="bg-card-foreground/10">
                    <TableHead style={{ width: 60 }}></TableHead>
                    <TableHead>Name</TableHead>
                    <TableHead>Type</TableHead>
                    <TableHead>Size</TableHead>
                    <TableHead className="text-right"></TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {mods.map((mod) => {
                    const res = extractResults[mod.path];
                    return (
                      <TableRow key={mod.path} style={{ scrollSnapAlign: 'start' }}>
                        <TableCell>
                          {res?.preview_image ? (
                            <img src={`file://${res.preview_image}`} style={{ width: 60, height: 60, borderRadius: 8, objectFit: 'cover' }} />
                          ) : (
                            <div className="flex items-center justify-center">
                              {res?.mod_type === "motorcycle" ? <Motorbike size={18} /> : res?.mod_type === "car" ? <Car size={18} /> : res?.mod_type === "plane" ? <Plane size={18} /> : <CircleQuestionMark size={18} className="text-primary/50" />}
                            </div>
                          )}
                        </TableCell>
                        <TableCell className="font-medium">{res?.mod_name || mod.name}</TableCell>
                        <TableCell>
                          {res?.success && res.mod_type ? (res.mod_type.toLowerCase().replace(/\b(\w)/g, s => s.toUpperCase())) : "—"}
                          {res?.error && <div className="text-red-500">{res.error}</div>}
                        </TableCell>
                        <TableCell>{formatSize(mod.size_mb)}</TableCell>
                        <TableCell className="text-right space-x-2">
                          <Button variant={"outline"} size={"icon-sm"} onClick={() => handleExtract(mod.path)}>
                            {res?.success ? <RotateCcw /> : <Download />}
                          </Button>
                          <Button variant={"outline"} size={"icon-sm"} className="group" onClick={() => handleDeleteMod(mod.path)} disabled={isDeleting && modToDelete === mod.path}>
                            {isDeleting && modToDelete === mod.path ? <Loader2 className="animate-spin" /> : <Trash2 className="group-hover:text-red-500 transition-all ease duration-300" />}
                          </Button>
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            )
          }
        </div>
      </div>

      {
        isSettingsOpen && (
          <AlertDialog open={isSettingsOpen} onOpenChange={setIsSettingsOpen}>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Settings</AlertDialogTitle>
              </AlertDialogHeader>
              <Field className="flex flex-col items-center justify-center gap-4 p-4 pt-0 pb-0">
                <div className="flex flex-row items-center gap-4">
                  <p>Theme:</p>
                  <ModeToggle />
                </div>

                <FieldSeparator>About</FieldSeparator>

                <div className="text-left space-y-1 pb-2.5">
                  <p className="text-sm">Version 1.4.0</p>
                  <p className="text-sm">Developed by <span className="inline-flex flex-row items-center gap-2 font-semibold">Soham P  <a href="https://github.com/sohamparanjape12" target="_blank" rel="noopener noreferrer"><ExternalLink size={18} className="inline" /></a></span></p>
                  <p className="text-sm text-muted-foreground">Please ensure to backup the mods folder!</p>
                  <p className="text-xs text-muted-foreground">This tool is not affiliated with or endorsed by Rockstar Games.</p>
                </div>
                
                <FieldSeparator>Acknowledgements</FieldSeparator>

                <div className="text-center space-y-1 pt-2">
                  <ul style={{ fontFamily: '"Cutive Mono", monospace'}} className="list-disc list-inside text-left space-y-1 mb-2.5 text-sm">
                    <li>
                      CodeWalker (https://github.com/dexyfex/CodeWalker). CodeWalker is copyright dexyfex.
                    </li>
                    <li>
                      rpf-cli (https://github.com/VIRUXE/rpf-cli). rpf-cli is copyright VIRUXE.
                    </li>
                    <li>
                      unrar.exe (Copyright © Alexander Roshal, RARLAB).
                    </li>
                  </ul>
                </div>
              </Field>
              <AlertDialogFooter>
                <AlertDialogCancel onClick={() => setIsSettingsOpen(false)}>
                  Close
                </AlertDialogCancel>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        )
      }

      <AlertDialog open={showDeleteConfirm} onOpenChange={setShowDeleteConfirm}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Are you sure?</AlertDialogTitle>
            <AlertDialogDescription>
              This will permanently delete the mod from your game. This action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={confirmDelete}>
              {isDeleting ? <Loader2 className="animate-spin" /> : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}