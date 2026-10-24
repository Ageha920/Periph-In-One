use crate::model::SavedDevice;
use serde::{Deserialize, Serialize};
use std::{io::{self, Write}, path::{Path, PathBuf}};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub width: f32,
    pub height: f32,
    pub devices: Vec<SavedDevice>,
}

impl Default for Config {
    fn default() -> Self {
        Self { version: 1, width: 360.0, height: 640.0, devices: Vec::new() }
    }
}

impl Config {
    pub fn path() -> io::Result<PathBuf> {
        let exe = std::env::current_exe()?;
        Ok(exe.parent().ok_or_else(|| io::Error::other("无法确定程序目录"))?.join("config/settings.json"))
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let mut config: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                if config.version != 1 { return Err(io::Error::other("不支持的配置版本，请保留原文件")); }
                if !config.width.is_finite() || !config.height.is_finite() {
                    return Err(io::Error::other("窗口尺寸无效"));
                }
                config.width = config.width.clamp(320.0, 3840.0);
                config.height = config.height.clamp(340.0, 2160.0);
                Ok(config)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let parent = path.parent().ok_or_else(|| io::Error::other("配置目录无效"))?;
        std::fs::create_dir_all(parent)?;
        // Atomic replacement: interruption must not truncate existing remarks.
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(file.as_file_mut(), self).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.as_file_mut().sync_all()?;
        file.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
}

