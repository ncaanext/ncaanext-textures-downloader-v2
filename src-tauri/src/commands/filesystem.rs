use crate::config::SLUS_FOLDER;
use chrono::Local;
use std::fs;
use std::path::PathBuf;

/// Check if the SLUS folder already exists in the textures directory
#[tauri::command]
pub fn check_existing_folder(textures_dir: String) -> Result<bool, String> {
    let path = PathBuf::from(&textures_dir).join(SLUS_FOLDER);
    Ok(path.exists())
}

/// Backup the existing SLUS folder by renaming it with a timestamp
#[tauri::command]
pub fn backup_existing_folder(textures_dir: String) -> Result<String, String> {
    let source = PathBuf::from(&textures_dir).join(SLUS_FOLDER);

    if !source.exists() {
        return Err(format!("Folder {} does not exist", SLUS_FOLDER));
    }

    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
    let backup_name = format!("{}_backup_{}", SLUS_FOLDER, timestamp);
    let dest = PathBuf::from(&textures_dir).join(&backup_name);

    fs::rename(&source, &dest)
        .map_err(|e| format!("Failed to backup folder: {}", e))?;

    Ok(backup_name)
}

/// Delete the existing SLUS folder
#[tauri::command]
pub fn delete_existing_folder(textures_dir: String) -> Result<(), String> {
    let path = PathBuf::from(&textures_dir).join(SLUS_FOLDER);

    if !path.exists() {
        return Ok(());
    }

    fs::remove_dir_all(&path)
        .map_err(|e| format!("Failed to delete folder: {}", e))?;

    Ok(())
}

/// Check if a directory exists and is writable
#[tauri::command]
pub fn validate_directory(path: String) -> Result<bool, String> {
    let path = PathBuf::from(&path);

    if !path.exists() {
        return Ok(false);
    }

    if !path.is_dir() {
        return Err("Path is not a directory".to_string());
    }

    // Try to check write permission by checking metadata
    match fs::metadata(&path) {
        Ok(metadata) => {
            if metadata.permissions().readonly() {
                Err("Directory is read-only".to_string())
            } else {
                Ok(true)
            }
        }
        Err(e) => Err(format!("Cannot access directory: {}", e)),
    }
}
