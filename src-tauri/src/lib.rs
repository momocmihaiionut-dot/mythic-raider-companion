// Tauri v2 backend for the HLTG companion: detect WoW, download data, write SavedVariables.

use std::fs;
use std::path::PathBuf;
use serde::Serialize;

const COMMON: &[&str] = &[
    r"C:\Program Files (x86)\World of Warcraft\_retail_",
    r"C:\Program Files\World of Warcraft\_retail_",
    r"D:\World of Warcraft\_retail_",
    r"D:\Games\World of Warcraft\_retail_",
    r"D:\Jocuri\World of Warcraft\_retail_",
    r"E:\World of Warcraft\_retail_",
];

#[derive(Serialize)]
struct SyncResult {
    accounts: usize,
    bytes: usize,
    paths: Vec<String>,
}

#[cfg(windows)]
fn registry_path() -> Option<PathBuf> {
    use winreg::enums::*;
    use winreg::RegKey;
    let roots = [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER];
    let subs = [
        r"SOFTWARE\WOW6432Node\Blizzard Entertainment\World of Warcraft",
        r"SOFTWARE\Blizzard Entertainment\World of Warcraft",
    ];
    for root in roots {
        let hk = RegKey::predef(root);
        for sub in subs {
            if let Ok(k) = hk.open_subkey(sub) {
                if let Ok(p) = k.get_value::<String, _>("InstallPath") {
                    let pb = PathBuf::from(p);
                    let retail = if pb.join("WTF").exists() { pb } else { pb.join("_retail_") };
                    if retail.join("WTF").exists() {
                        return Some(retail);
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn registry_path() -> Option<PathBuf> { None }

fn detect() -> Option<PathBuf> {
    if let Some(p) = registry_path() {
        return Some(p);
    }
    for c in COMMON {
        let p = PathBuf::from(c);
        if p.join("WTF").exists() {
            return Some(p);
        }
    }
    None
}

fn fetch(source: &str) -> Result<Vec<u8>, String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        // Timeout so a stalled server can't hang the sync worker indefinitely (the data file is
        // several MB, so allow a generous window for slow connections).
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client.get(source).send().map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        Ok(resp.bytes().map_err(|e| e.to_string())?.to_vec())
    } else {
        fs::read(source).map_err(|e| format!("read {}: {}", source, e))
    }
}

// ---- Tauri commands ----

#[tauri::command]
fn detect_wow() -> Option<String> {
    detect().map(|p| p.display().to_string())
}

#[tauri::command]
fn validate_path(path: String) -> bool {
    PathBuf::from(&path).join("WTF").exists()
}

// Native folder picker for the WoW _retail_ directory. rfd is blocking, so run it on a worker
// thread to keep the WebView responsive. `current` (if a real dir) seeds the dialog's start folder.
#[tauri::command]
async fn pick_folder(current: Option<String>) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dlg = rfd::FileDialog::new()
            .set_title("Select your World of Warcraft _retail_ folder");
        if let Some(c) = current.as_deref() {
            let p = PathBuf::from(c);
            if p.is_dir() {
                dlg = dlg.set_directory(p);
            }
        }
        dlg.pick_folder().map(|p| p.display().to_string())
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
fn wow_running() -> bool {
    #[cfg(windows)]
    {
        if let Ok(out) = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq Wow.exe", "/NH"])
            .output()
        {
            return String::from_utf8_lossy(&out.stdout).contains("Wow.exe");
        }
    }
    false
}

// Ask the server to start a background crawl of one spec ("Class:Spec"). The server returns at once
// (a per-region crawl outlasts the proxy timeout); the UI then polls talent_status. Async + a worker
// thread so the blocking request never freezes the WebView. base ends with ".../api/talents/".
#[tauri::command]
async fn refresh_spec(base: String, spec: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || refresh_spec_blocking(base, spec))
        .await
        .map_err(|e| e.to_string())?
}

fn refresh_spec_blocking(base: String, spec: String) -> Result<String, String> {
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(format!("{}refresh", base))
        .query(&[("spec", &spec)])
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if status.as_u16() == 429 {
        // Server guard replies (already fresh / busy / budget) are user-facing sentences —
        // pass them through bare so the UI can show them without an "HTTP 429" prefix.
        return Err(body);
    }
    if !status.is_success() {
        return Err(format!("HTTP {} {}", status, body));
    }
    Ok(body)
}

// Small JSON poll of crawl progress: { generatedAt, specs, running, spec, error }.
#[tauri::command]
async fn talent_status(base: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(format!("{}status", base))
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        resp.text().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn sync(wow_path: String, source: String) -> Result<SyncResult, String> {
    tauri::async_runtime::spawn_blocking(move || sync_blocking(wow_path, source))
        .await
        .map_err(|e| e.to_string())?
}

fn sync_blocking(wow_path: String, source: String) -> Result<SyncResult, String> {
    let retail = PathBuf::from(&wow_path);
    if !retail.join("WTF").exists() {
        return Err("WoW path invalid (no WTF folder).".into());
    }
    // Write the data as an addon code file (Interface/AddOns/MythicRaiderTalents/Data.lua) rather
    // than SavedVariables. WoW re-executes addon files on /reload and never overwrites them, so the
    // user can sync while the game is open and just /reload — no need to close the game.
    let addon_dir = retail.join("Interface").join("AddOns").join("MythicRaiderTalents");
    if !addon_dir.exists() {
        return Err("Mythic Raider Talents addon not found in Interface/AddOns. Install the addon first.".into());
    }
    let data = fetch(&source)?;
    // Safety: never overwrite Data.lua unless this really is Mythic Raider talent data.
    // Guards against a 503 page or an old/incompatible server response wiping good data.
    let head = &data[..data.len().min(512)];
    if !head.windows(21).any(|w| w == b"MythicRaiderTalentsDB") {
        return Err("That didn't look like Mythic Raider talent data (the server may be updating). Nothing was written — try again in a minute.".into());
    }
    // Stamp the companion version after the downloaded table so the addon can tell a self-updating
    // install (0.4.0+) from an older one and nudge only the latter to update. Valid Lua: it sets a
    // field on the global table the downloaded data just assigned.
    let mut out = data;
    out.extend_from_slice(
        format!("\nMythicRaiderTalentsDB.companionVersion = \"{}\"\n", env!("CARGO_PKG_VERSION"))
            .as_bytes(),
    );
    let target = addon_dir.join("Data.lua");
    fs::write(&target, &out).map_err(|e| e.to_string())?;
    Ok(SyncResult { accounts: 1, bytes: out.len(), paths: vec![target.display().to_string()] })
}

#[derive(Serialize)]
struct UpdateInfo {
    version: String,
    notes: String,
}

// The updater only works in packaged builds; in `tauri dev` check() errors and the UI
// treats any error as "no update", so development is unaffected.
#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await.map_err(|e| e.to_string())? {
        Some(u) => Ok(Some(UpdateInfo {
            version: u.version.clone(),
            notes: u.body.clone().unwrap_or_default(),
        })),
        None => Ok(None),
    }
}

#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("No update available.")?;
    let mut downloaded: usize = 0;
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk;
                let _ = app.emit(
                    "update-progress",
                    serde_json::json!({ "downloaded": downloaded, "total": total }),
                );
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            detect_wow,
            validate_path,
            pick_folder,
            wow_running,
            refresh_spec,
            talent_status,
            sync,
            check_update,
            install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
