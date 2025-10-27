use std::{fs};
#[cfg(windows)]
use winreg::RegKey;
#[cfg(windows)]
use winreg::enums::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use regex::Regex;
use std::process::Command;
use walkdir::WalkDir;
use zip::ZipArchive;
use std::fs::File;
use fs_extra::dir::{copy as copy_dir, CopyOptions};
use xmltree::{Element, XMLNode};
use tauri::Manager;
use tauri::Emitter;
use tauri::async_runtime;
use std::collections::HashSet;
use std::io::Write;
#[cfg(windows)]
use std::os::windows::process::CommandExt;


#[derive(Serialize)]
pub struct ModInfo {
    pub name: String,
    pub size_mb: f64,
    pub path: String,
    pub preview_image: Option<Vec<u8>>,
    pub mod_type: String,
}

#[derive(Serialize)]
pub struct InstallResult {
    pub success: bool,
    pub error: Option<String>,
    pub mod_name: Option<String>,
}

#[derive(Clone, Serialize)]
struct ProgressPayload {
    current: u32,
    total: u32,
    message: String,
}

#[tauri::command]
async fn install_mod_archive_async(
    app_handle: tauri::AppHandle,
    path: String,
    gta_path: String
) -> Result<InstallResult, String> {
    let tauri_root = ".";
    let gta_path_for_block = gta_path.clone();
    let app_handle_for_block = app_handle.clone();

    let install_result = async_runtime::spawn_blocking(move || {
        install_mod_archive_with_progress(app_handle_for_block, path, gta_path_for_block)
    }).await.map_err(|e| format!("Thread error: {:?}", e))??;

    sync_and_patch_dlclist(app_handle, gta_path, tauri_root).await?;

    Ok(install_result)
}

fn install_mod_archive_with_progress(
    app_handle: tauri::AppHandle,
    path: String,
    gta_path: String,
) -> Result<InstallResult, String> {
    let emit_progress = |current: u32, total: u32, msg: &str| {
        let _ = app_handle.emit("install-progress", ProgressPayload {
            current,
            total: 6,
            message: msg.to_string(),
        });
    };
    emit_progress(0, 6, "Starting installation...");

    let ext = Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let temp_dir = std::env::temp_dir()
        .join(format!("mod_install_temp_{:x}", md5::compute(path.as_bytes())));
    fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp dir: {e}"))?;

    emit_progress(1, 6, "Extracting archive...");

    if ext == "zip" {
        let archive_file = File::open(&path).map_err(|e| format!("Failed to open archive: {e}"))?;
        let mut archive = ZipArchive::new(archive_file).map_err(|e| format!("Failed to read zip: {e}"))?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
            let out_path = temp_dir.join(file.name());
            if file.is_dir() {
                fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            } else {
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let mut outfile = File::create(&out_path).map_err(|e| e.to_string())?;
                std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
            }
        }
    } else if ext == "rar" {
        // UnRAR.exe is assumed to be in your bin_dir!
        let unrar_exe = if cfg!(debug_assertions) {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| PathBuf::from("."));
            let bin_dir = exe_dir
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.join("bin"))
                .unwrap_or_else(|| PathBuf::from("bin"));
            bin_dir.join("UnRAR.exe")
        } else {
            app_handle.path()
                .resolve("bin/UnRAR.exe", tauri::path::BaseDirectory::Resource)
                .unwrap()
        };

        let mut cmd = Command::new(unrar_exe);
        cmd.args(&["x", &path, temp_dir.to_str().unwrap()]);

        // Hide CLI window in production
        #[cfg(windows)]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        let status = cmd.status().map_err(|e| format!("Failed to run UnRAR.exe: {e}"))?;


        if !status.success() {
            return Err("Failed to extract RAR archive".to_string());
        }
    } else {
        return Err("Unsupported archive type. Only zip and rar are supported.".to_string());
    }


    emit_progress(2, 6, "Finding mod folder...");

    let mut dlc_folder = None;
    for entry in WalkDir::new(&temp_dir).min_depth(1).max_depth(5) {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_name() == "dlc.rpf" {
            dlc_folder = entry.path().parent().map(|p| p.to_path_buf());
            break;
        }
    }
    let dlc_folder = dlc_folder.ok_or("No dlc.rpf found in extracted archive")?;

    emit_progress(3, 6, "Copying mod files...");

    let mods_dir = Path::new(&gta_path).join("mods/update/x64/dlcpacks");
    let mod_name = dlc_folder
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown_mod".to_string());
    let target_mod_dir = mods_dir.join(&mod_name);

    if target_mod_dir.exists() {
        fs::remove_dir_all(&target_mod_dir).ok();
    }
    let mut copy_opts = CopyOptions::new();
    copy_opts.overwrite = true;
    copy_opts.copy_inside = true;
    copy_dir(&dlc_folder, &mods_dir, &copy_opts)
        .map_err(|e| format!("Failed to copy mod folder: {e}"))?;

    emit_progress(4, 6, "Updating dlclist.xml...");
    emit_progress(6, 6, "Installation complete!");

    let _ = fs::remove_dir_all(&temp_dir);

    Ok(InstallResult {
        success: true,
        error: None,
        mod_name: Some(mod_name),
    })
}

#[derive(Serialize)]
pub struct DeleteResult {
    pub success: bool,
    pub error: Option<String>,
}


#[tauri::command]
async fn delete_mod_async(
    app_handle: tauri::AppHandle,
    mod_name: String,
    gta_path: String
) -> Result<DeleteResult, String> {
    let tauri_root = ".";
    let mod_dir = PathBuf::from(&gta_path).join("mods/update/x64/dlcpacks").join(&mod_name);
    fs::remove_dir_all(&mod_dir).ok();

    sync_and_patch_dlclist(app_handle, gta_path, tauri_root).await?;
    Ok(DeleteResult {
        success: true,
        error: None,
    })
}


pub fn safe_overwrite(path: &Path, buf: &[u8]) -> Result<(), String> {
    let temp_path = path.with_extension("xml.tmp");
    {
        let mut temp = fs::File::create(&temp_path).map_err(|e| format!("Failed to create temp file: {e}"))?;
        temp.write_all(buf).map_err(|e| format!("Failed to write temp file: {e}"))?;
        temp.flush().map_err(|e| format!("Failed to flush temp file: {e}"))?;
    }
    fs::rename(&temp_path, path).map_err(|e| format!("Failed to rename temp file: {e}"))?;
    Ok(())
}


pub fn sync_dlclist_with_dlcpacks_dir(
    dlclist_xml_path: &std::path::Path,
    dlcpacks_dir: &std::path::Path,
) -> Result<(), String> {

    const BASE_DLC_ITEMS: &[&str] = &[
        "mp2023_01",
        "mp2023_01_g9ec",
        "mp2023_02",
        "mp2023_02_g9ec",
        "mp2024_01",
        "mp2024_01_g9ec",
        "mp2024_02",
        "mp2024_02_g9ec",
        "mp2025_01",
        "mp2025_01_G9EC",
        "mpairraces",
        "mpapartment",
        "mpassault",
        "mpbattle",
        "mpbiker",
        "mpchristmas2",
        "mpchristmas2017",
        "mpchristmas2018",
        "mpchristmas3",
        "mpchristmas3_g9ec",
        "mpexecutive",
        "mpg9ec",
        "mpgunrunning",
        "mphalloween",
        "mpheist",
        "mpheist3",
        "mpheist4",
        "mpimportexport",
        "mpjanuary2016",
        "mplowrider",
        "mplowrider2",
        "mpluxe",
        "mpluxe2",
        "mppatchesng",
        "mpreplay",
        "mpsecurity",
        "mpsmuggler",
        "mpspecialraces",
        "mpstunt",
        "mpsum",
        "mpsum2",
        "mpSum2_G9EC",
        "mptuner",
        "mpvalentines2",
        "mpvinewood",
        "mpxmas_604490",
        "patch2023_01",
        "patch2023_01_g9ec",
        "patch2023_02",
        "patch2024_01",
        "patch2024_01_g9ec",
        "patch2024_02",
        "patch2025_01",
        "patchday10ng",
        "patchday11ng",
        "patchday12ng",
        "patchday13ng",
        "patchday14ng",
        "patchday15ng",
        "patchday16ng",
        "patchday17ng",
        "patchday18ng",
        "patchday19ng",
        "patchday1ng",
        "patchday20ng",
        "patchday21ng",
        "patchday22ng",
        "patchday23ng",
        "patchday24ng",
        "patchday25ng",
        "patchday26ng",
        "patchday27g9ecng",
        "patchday27ng",
        "patchday28g9ecng",
        "patchday28ng",
        "patchday2bng",
        "patchday2ng",
        "patchday3ng",
        "patchday4ng",
        "patchday5ng",
        "patchday6ng",
        "patchday7ng",
        "patchday8ng",
        "patchday9ng",
        "patchdayg9ecng",
    ];

    let xml = fs::read_to_string(dlclist_xml_path)
        .map_err(|e| format!("Failed to read dlclist.xml: {e}"))?;
    println!("[PATCH] Read XML, {} bytes", xml.len());
    let mut root = match Element::parse(xml.as_bytes()) {
        Ok(mut el) => {
            if el.get_mut_child("Paths").is_some() {
                el
            } else {
                let mut el = Element::new("SMandatoryPacksData");
                let paths = Element::new("Paths");
                el.children.push(XMLNode::Element(paths));
                el
            }
        },
        Err(_) => {
            let mut el = Element::new("SMandatoryPacksData");
            let paths = Element::new("Paths");
            el.children.push(XMLNode::Element(paths));
            el
        }
    };
    println!("[PATCH] Parsed or constructed root: {}", root.name);

    let paths = root.get_mut_child("Paths").expect("Should have <Paths> now");
    paths.children.clear();


    for id in BASE_DLC_ITEMS.iter() {
        let mut el = Element::new("Item");
        el.children.push(XMLNode::Text(format!("dlcpacks:/{}/", id)));
        paths.children.push(XMLNode::Element(el));
    }
    let mut mod_folders: Vec<String> = vec![];
    for entry in fs::read_dir(dlcpacks_dir).map_err(|e| format!("Failed to read dlcpacks dir: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() && path.join("dlc.rpf").exists() {
            if let Some(name) = entry.file_name().to_str() {
                if !BASE_DLC_ITEMS.iter().any(|base| base.eq_ignore_ascii_case(name)) {
                    mod_folders.push(name.to_string());
                }
            }
        }
    }
    mod_folders.sort_unstable();
    for mod_name in mod_folders {
        let mut el = Element::new("Item");
        el.children.push(XMLNode::Text(format!("dlcpacks:/{}/", mod_name)));
        paths.children.push(XMLNode::Element(el));
    }
    println!("[PATCH] Paths children after patch: {}", paths.children.len());
    let mut buf = Vec::new();
    buf.extend_from_slice(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let config = xmltree::EmitterConfig::new()
        .perform_indent(true)
        .indent_string("  ")
        .write_document_declaration(false);
    root.write_with_config(&mut buf, config)
        .map_err(|e| format!("Failed to write synced XML: {e}"))?;

    // Use the new safe overwrite helper
    safe_overwrite(dlclist_xml_path, &buf)?;
    println!("[PATCH] Wrote XML of {} bytes", buf.len());
    Ok(())
}



#[tauri::command]
async fn sync_and_patch_dlclist(
    app_handle: tauri::AppHandle,
    gta_path: String,
    tauri_root: &str
) -> Result<(), String> {
    async_runtime::spawn_blocking(move || {
        let update_rpf = Path::new(&gta_path).join("mods/update/update.rpf");
        let temp_dir = std::env::temp_dir().join(format!("dlclist_sync_{}", std::process::id()));
        let dlcpacks_dir = Path::new(&gta_path).join("mods/update/x64/dlcpacks");

        let rpf_exe = if cfg!(debug_assertions) {
            std::env::current_dir().unwrap().join("src-tauri/bin/rpf.exe")
        } else {
            app_handle.path().resolve("bin/rpf.exe", tauri::path::BaseDirectory::Resource).unwrap()
        };
        let codewalker_exe = if cfg!(debug_assertions) {
            std::env::current_dir().unwrap().join("src-tauri/bin/CodeWalkerCLI.exe")
        } else {
            app_handle.path().resolve("bin/CodeWalkerCLI.exe", tauri::path::BaseDirectory::Resource).unwrap()
        };


        if temp_dir.exists() { let _ = fs::remove_dir_all(&temp_dir); }
        fs::create_dir_all(&temp_dir).map_err(|e| format!("{e}"))?;
        let mut cmd1 = Command::new(&rpf_exe);
        cmd1.args(&[
            "extract",
            update_rpf.to_str().unwrap(),
            "common/data/dlclist.xml",
            "--output", temp_dir.to_str().unwrap(),
        ]);

        #[cfg(windows)]
        {
            cmd1.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        if !cmd1.status().unwrap().success() {
            return Err(format!(
                "rpf.exe extraction failed: {}",
                String::from_utf8_lossy(&cmd1.output().unwrap().stderr)
            ));
        }
        let xml_path = temp_dir.join("common/data/dlclist.xml");
        sync_dlclist_with_dlcpacks_dir(&xml_path, &dlcpacks_dir)?;
        let mut cmd2 = Command::new(&codewalker_exe);
        cmd2.args(&[
            &gta_path,
            update_rpf.to_str().unwrap(),
            "common/data/dlclist.xml",
            xml_path.to_str().unwrap(),
        ]);

        #[cfg(windows)]
        {
            cmd2.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
            
        if !cmd2.status().unwrap().success() {
            let _ = fs::remove_dir_all(&temp_dir);
            return Err(format!(
                "CodeWalkerCLI failed: {:?}, {:?}",
                cmd2.status().unwrap().code(),
                String::from_utf8_lossy(&cmd2.output().unwrap().stderr)
            ));
        }
        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    })
    .await
    .map_err(|e| format!("Thread error: {e}"))?
}



#[tauri::command]
fn detect_gta_path() -> Option<String> {
    // Windows only: Steam detection
    #[cfg(windows)]
    {
        if let Ok(hklm) = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Valve\\Steam")
        {
            if let Ok(steam_path) = hklm.get_value::<String, _>("SteamPath") {
                let gta_dir = format!("{}\\steamapps\\common\\Grand Theft Auto V", steam_path);
                if std::path::Path::new(&gta_dir).join("GTA5.exe").exists() {
                    return Some(gta_dir);
                }
            }
        }
        // Epic and Rockstar: Check default directories
        let epic_path = "C:\\Program Files\\Epic Games\\GTAV";
        if std::path::Path::new(epic_path).join("GTA5.exe").exists() {
            return Some(epic_path.to_string());
        }
        let rockstar_path = "C:\\Program Files\\Rockstar Games\\Grand Theft Auto V";
        if std::path::Path::new(rockstar_path).join("GTA5.exe").exists() {
            return Some(rockstar_path.to_string());
        }
    }
    None
}

#[tauri::command]
fn validate_gta_path(path: String) -> Option<String> {
    let exe_path = format!("{}\\GTA5.exe", path);
    if std::path::Path::new(&exe_path).exists() {
        Some(path)
    } else {
        None
    }
}



#[tauri::command]
fn list_installed_mods(app_handle: tauri::AppHandle, gta_path: String) -> Vec<ModInfo> {
    let dlcpacks_dir = format!(r"{}\mods\update\x64\dlcpacks", gta_path);
    let mut mods = Vec::new();
    let tauri_root = "."; // You can update this to your app's actual root as needed

    if let Ok(entries) = fs::read_dir(&dlcpacks_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dlc_rpf = path.join("dlc.rpf");
                if dlc_rpf.exists() {
                    // 1. Prepare extraction folder
                    let unique_hash = format!("{:x}", md5::compute(path.to_string_lossy().as_bytes()));
                    let extract_to = std::env::temp_dir().join(format!("mod_temp_{}", unique_hash)).to_string_lossy().to_string();
                    let _ = fs::create_dir_all(&extract_to);

                    // 2. Call the rpf.exe CLI to extract files
                    let rpf_exe = if cfg!(debug_assertions) {
                        // dev mode only: use relative path from project root
                        std::env::current_dir()
                            .unwrap()
                            .join("src-tauri")
                            .join("bin")
                            .join("rpf.exe")
                    } else {
                        app_handle
                            .path()
                            .resolve("bin/rpf.exe", tauri::path::BaseDirectory::Resource)
                            .unwrap()
                    };

                    let mut cmd = Command::new(&rpf_exe);
                    cmd.args([
                        "extract",
                        &dlc_rpf.to_string_lossy(),
                        "--output",
                        &extract_to,
                    ]);

                    #[cfg(windows)]
                    {
                        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
                    }

                    let mut mod_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let mut preview_image = None;
                    let mut mod_type = "unknown".to_string();

                    if let Ok(out) = cmd.output() {
                        if out.status.success() {
                            // Look for XML and image files in extract_to
                            if let Ok(entries) = fs::read_dir(&extract_to) {
                                for entry in entries.flatten() {
                                    let fname = entry.file_name().to_string_lossy().to_lowercase();
                                    let fpath = entry.path().to_string_lossy().to_string();
                                    if fname.contains("content.xml") || fname.contains("setup2.xml") {
                                        let xml_content = fs::read_to_string(&fpath).unwrap_or_default();
                                        if let Some(n) = xml_content.lines().find(|l| l.contains("<dataName>") || l.contains("<name>")) {
                                            if let (Some(start), Some(end)) = (n.find('>'), n.rfind('<')) {
                                                mod_name = n[start+1..end].trim().to_string();
                                            }
                                        }
                                        // Detect mod type by tags in xml_content if you want (pattern match for vehicleClass etc)
                                        if xml_content.contains("vehicle") {
                                            mod_type = "car".to_string();
                                            if xml_content.contains("plane") {
                                                mod_type = "plane".to_string();
                                            } else if xml_content.contains("bike") {
                                                mod_type = "motorcycle".to_string();
                                            }
                                        } else if xml_content.contains("ymap") {
                                            mod_type = "map".to_string();
                                        } else if xml_content.contains("peds.meta") {
                                            mod_type = "ped".to_string();
                                        }
                                    }
                                    if fname.contains("preview") || fname.contains("icon") {
                                        preview_image = Some(fs::read(&fpath).unwrap_or_default());
                                    }
                                }
                            }
                        }
                    }

                    let size_mb = dlc_rpf.metadata().map(|md| md.len() as f64 / 1_048_576.0).unwrap_or(0.0);
                    mods.push(ModInfo {
                        name: mod_name,
                        size_mb: (size_mb * 100.0).round() / 100.0,
                        path: path.to_string_lossy().to_string(),
                        preview_image,
                        mod_type,
                    });
                }
            }
        }
    }
    mods
}



#[derive(Serialize)]
pub struct ExtractResult {
    pub success: bool,
    pub error: Option<String>,
    pub mod_name: Option<String>,
    pub preview_image: Option<String>,
    pub mod_type: Option<String>, // <-- add this
}

#[tauri::command]
fn extract_mod_metadata(
    app_handle: tauri::AppHandle,
    dlc_rpf_path: String,
    tauri_root: String
) -> ExtractResult {
    use regex::Regex;

    let rpf_exe = if cfg!(debug_assertions) {
        // dev mode only: use relative path from project root
        std::env::current_dir()
            .unwrap()
            .join("src-tauri")
            .join("bin")
            .join("rpf.exe")
    } else {
        app_handle
            .path()
            .resolve("bin/rpf.exe", tauri::path::BaseDirectory::Resource)
            .unwrap()
    };

    let unique_hash = format!("{:x}", md5::compute(dlc_rpf_path.to_string().as_bytes()));
    let extract_to = std::env::temp_dir().join(format!("mod_temp_{}", unique_hash));
    println!("Extracting to {:?}", extract_to);

    if let Err(e) = fs::create_dir_all(&extract_to) {
        return ExtractResult {
            success: false,
            error: Some(format!("Failed to create output dir: {e}")),
            mod_name: None,
            preview_image: None,
            mod_type: None,
        };
    }

    let mut cmd = Command::new(&rpf_exe);
    cmd.args([
        "extract",
        &dlc_rpf_path,
        "--output",
        &extract_to.to_string_lossy(),
    ]);

    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    if let Err(err) = cmd.output() {
        return ExtractResult {
            success: false,
            error: Some(format!("Failed to run rpf.exe: {err}")),
            mod_name: None,
            preview_image: None,
            mod_type: None,
        };
    }
    let output = cmd.output().unwrap();

    if !cmd.status().unwrap().success() {
        return ExtractResult {
            success: false,
            error: Some(format!("{}", String::from_utf8_lossy(&cmd.output().unwrap().stderr))),
            mod_name: None,
            preview_image: None,
            mod_type: None,
        };
    }

    let mut mod_name = None;
    let mut preview_image = None;
    let vehicle_class_re = Regex::new(r"<vehicleClass>\s*([A-Za-z0-9_]+)\s*</vehicleClass>").unwrap();
    let vehicle_type_re = Regex::new(r"<type>\s*VEHICLE_TYPE_([A-Za-z0-9_]+)\s*</type>").unwrap();
    let mut mod_type = None;

    println!("Extracted files (recursive):");
    for entry in WalkDir::new(&extract_to).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() { continue; }
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        println!("File: {}", path.display());
        let xml_content = fs::read_to_string(&path).unwrap_or_default();

        // VehicleClass detection
        if xml_content.contains("<vehicleClass>") {
            println!("Content has <vehicleClass>: {:?}", path.display());
            if let Some(caps) = vehicle_class_re.captures(&xml_content) {
                let vclass = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                println!("Detected vehicleClass: {}", vclass);
                mod_type = match vclass {
                    "VC_BIKE" | "VC_MOTORCYCLE" => Some("motorcycle".to_string()),
                    "VC_PLANE" => Some("plane".to_string()),
                    "VC_BOAT" => Some("boat".to_string()),
                    "VC_HELI" => Some("helicopter".to_string()),
                    _ if vclass.starts_with("VC_") => Some("car".to_string()),
                    _ => Some("unknown".to_string()),
                };
            }
        }
        // VehicleType detection as backup
        if mod_type.is_none() && xml_content.contains("<type>") {
            if let Some(caps) = vehicle_type_re.captures(&xml_content) {
                let vtype = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                println!("Detected vehicleType: {}", vtype);
                mod_type = match vtype {
                    "BIKE" | "MOTORCYCLE" => Some("motorcycle".to_string()),
                    "PLANE" => Some("plane".to_string()),
                    "BOAT" => Some("boat".to_string()),
                    "HELI" => Some("helicopter".to_string()),
                    "CAR" => Some("car".to_string()),
                    _ => Some("unknown".to_string()),
                };
            }
        }
        // Fallbacks for map/ped
        if mod_type.is_none() && (name.contains("content.xml") || name.contains("setup2.xml")) {
            if xml_content.contains("<ymap") {
                mod_type = Some("map".to_string());
            }
        }
        if mod_type.is_none() && (name.contains("peds.meta") || xml_content.contains("<ped")) {
            mod_type = Some("ped".to_string());
        }
        // Mod name extraction
        if name.contains("content.xml") || name.contains("setup2.xml") {
            if let Some(n) = xml_content.lines().find(|l| l.contains("<dataName>") || l.contains("<name>")) {
                let start = n.find('>').unwrap_or(0) + 1;
                let end = n.rfind('<').unwrap_or(n.len());
                mod_name = Some(n[start..end].trim().to_string());
            }
        }
        // Preview image
        if name.contains("preview") || name.contains("icon") {
            preview_image = Some(path.to_string_lossy().to_string());
        }
    }
    println!("Final detected mod_type: {:?}", mod_type);

    ExtractResult {
        success: true,
        error: None,
        mod_name,
        preview_image,
        mod_type,
    }
}



#[tauri::command]
async fn list_valid_mods(gta_path: String, app_handle: tauri::AppHandle) -> Result<Vec<ModInfo>, String> {
    async_runtime::spawn_blocking(move || {
        get_valid_mods(&gta_path, &app_handle, ".")
    })
    .await
    .map_err(|e| format!("Thread error: {e}"))?
}

fn get_valid_mods(gta_path: &str, app_handle: &tauri::AppHandle, tauri_root: &str) -> Result<Vec<ModInfo>, String> {
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::fs;
    use std::process::Command;
    use regex::Regex;

    let dlcpacks_dir = PathBuf::from(gta_path).join("mods/update/x64/dlcpacks");
    let update_rpf = PathBuf::from(gta_path).join("mods/update/update.rpf");
    let temp_extract_dir = std::env::temp_dir().join(format!("dlclist_listpatch_{}", std::process::id()));

    // Locate rpf.exe: (same dev/prod logic as install/remove)
    let rpf_exe = if cfg!(debug_assertions) {
        std::env::current_dir().unwrap().join("src-tauri/bin/rpf.exe")
    } else {
        app_handle.path().resolve("bin/rpf.exe", tauri::path::BaseDirectory::Resource).unwrap()
    };
    let codewalker_exe = if cfg!(debug_assertions) {
        std::env::current_dir().unwrap().join("src-tauri/bin/CodeWalkerCLI.exe")
    } else {
        app_handle.path().resolve("bin/CodeWalkerCLI.exe", tauri::path::BaseDirectory::Resource).unwrap()
    };


    // Step 1: Extract dlclist.xml from update.rpf (just like install/update/remove)
    if temp_extract_dir.exists() {
        let _ = fs::remove_dir_all(&temp_extract_dir);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    fs::create_dir_all(&temp_extract_dir)
        .map_err(|e| format!("Failed to create temp extract dir: {e}"))?;

    let mut cmd = Command::new(&rpf_exe);
    cmd.args(&[
        "extract",
        update_rpf.to_str().unwrap(),
        "common/data/dlclist.xml",
        "--output",
        temp_extract_dir.to_str().unwrap(),
    ]);

    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let extract_output = cmd.output()
        .map_err(|e| format!("Failed to run rpf.exe: {e}"))?;
    if !extract_output.status.success() {
        return Err(format!("rpf.exe failed with exit code {:?}", extract_output.status.code()));
    }
    std::thread::sleep(std::time::Duration::from_millis(400));

    let actual_dlclist = temp_extract_dir.join("common").join("data").join("dlclist.xml");

    // Step 2: Build folder mod set
    let mut folder_mods = HashSet::new();
    let mut folder_info = std::collections::HashMap::new();
    if dlcpacks_dir.exists() {
        for entry in fs::read_dir(&dlcpacks_dir).map_err(|e| format!("Failed to read dlcpacks dir: {e}"))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let p = entry.path();
            if p.is_dir() && p.join("dlc.rpf").exists() {
                if let Some(name) = entry.file_name().to_str() {
                    folder_mods.insert(name.to_ascii_lowercase());
                    let size_mb = p.join("dlc.rpf").metadata().map(|m| m.len() as f64 / 1_048_576.0).unwrap_or(0.0);
                    folder_info.insert(name.to_ascii_lowercase(), (p.clone(), size_mb));
                }
            }
        }
    }

    // Step 3: Parse extracted dlclist.xml for mods
    let mut xml_mods = HashSet::new();
    let xml_text = fs::read_to_string(&actual_dlclist)
        .map_err(|e| format!("Could not read extracted dlclist.xml: {e}"))?;
    let re = Regex::new(r#"dlcpacks:/([^/]+)/"#).unwrap();
    for cap in re.captures_iter(&xml_text) {
        xml_mods.insert(cap[1].to_ascii_lowercase());
    }

    // Step 4: Only keep mods present in BOTH
    let valid_mods: Vec<String> = folder_mods.intersection(&xml_mods).cloned().collect();

    // Step 5: Build Vec<ModInfo>
    let mut mods = Vec::new();
    for mod_name in valid_mods {
        if let Some((path, size_mb)) = folder_info.get(&mod_name) {
            mods.push(ModInfo {
                name: mod_name.clone(),
                size_mb: (size_mb * 100.0).round() / 100.0,
                path: path.display().to_string(),
                preview_image: None,
                mod_type: "unknown".to_string(),
            });
        }
    }

    // Step 6: Cleanup
    let _ = fs::remove_dir_all(&temp_extract_dir);

    Ok(mods)
}





#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            detect_gta_path,
            validate_gta_path,
            list_installed_mods,
            extract_mod_metadata,
            install_mod_archive_async,
            delete_mod_async,
            list_valid_mods,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
