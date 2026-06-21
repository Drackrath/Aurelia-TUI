use crate::util::error::STError;

use std::fs::File;
use std::io::Write;
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

fn touch(path: &Path) -> io::Result<()> {
    match fs::OpenOptions::new().create(true).write(true).open(path) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

fn mkdir(dir: String) -> Result<PathBuf, STError> {
    let dir = shellexpand::full(&dir)?.to_string();
    let dir = Path::new(&dir);

    fs::create_dir_all(dir)?;
    Ok(dir.to_path_buf())
}

pub fn cache_directory() -> Result<PathBuf, STError> {
    let dir = match env::var("AURELIA_TUI_CACHE_DIR") {
        Ok(dir) => dir,
        _ => "~/.cache/aurelia-tui".to_string(),
    };
    mkdir(dir)
}

pub fn config_directory() -> Result<PathBuf, STError> {
    let dir = match env::var("AURELIA_TUI_DIR") {
        Ok(dir) => dir,
        _ => "~/.config/aurelia-tui".to_string(),
    };
    mkdir(dir)
}

pub fn icon_directory() -> Result<PathBuf, STError> {
    let dir = match env::var("AURELIA_TUI_ICON_DIR") {
        Ok(dir) => dir,
        _ => format!("{}/icons", cache_directory()?.as_path().display()),
    };
    mkdir(dir)
}

pub fn icon_exists(id: i32) -> Result<PathBuf, STError> {
    let dir = icon_directory()?;
    let icon = &format!("{}.ico", id);
    let icon = Path::new(icon);
    let icon = dir.join(icon);
    if icon.exists() {
        Ok(icon)
    } else {
        Err(STError::Problem(format!("Icon doesn't exist: {:?}", icon)))
    }
}

pub fn icon_save(id: i32, icon: &[u8]) -> Result<(), STError> {
    let dir = icon_directory()?;
    let icon_path = &format!("{}.ico", id);
    let icon_path = Path::new(icon_path);
    let icon_path = dir.join(icon_path);
    let mut file = File::create(icon_path)?;
    file.write_all(icon)?;
    Ok(())
}

pub fn config_location() -> Result<PathBuf, STError> {
    let dir = config_directory()?;
    let config_path = Path::new("config.json");
    let config_path = dir.join(config_path);
    touch(&config_path)?;
    Ok(config_path)
}

pub fn cache_location() -> Result<PathBuf, STError> {
    let dir = cache_directory()?;
    let cache_path = Path::new("games.json");
    let cache_path = dir.join(cache_path);
    touch(&cache_path)?;
    Ok(cache_path)
}

pub fn invalidate_cache() -> Result<(), STError> {
    fs::remove_file(cache_location()?)?;
    Ok(())
}
