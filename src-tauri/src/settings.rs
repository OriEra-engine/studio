//! 应用设置。
//! API key 存系统凭据管理器（Windows 凭据管理器），settings.json 只存非敏感字段。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

const KEYRING_SERVICE: &str = "OriEraStudio";
const KEYRING_ENTRY: &str = "provider-api-key";

/// L7 provider 配置 —— 与引擎 kernel.js 的 cfg {kind,key,model,base} 一一对应。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderCfg {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base: String,
}

impl ProviderCfg {
    pub fn is_usable(&self) -> bool {
        !self.kind.is_empty() && self.kind != "rule" && !self.key.is_empty()
    }
}

/// settings.json 中持久化的形态（key 不落盘）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredProvider {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub base: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub provider: Option<StoredProvider>,
    /// 开发者模式：demo/项目窗口自动开 devtools
    #[serde(default)]
    pub developer_mode: bool,
}

fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("cannot resolve app data dir: {e}"))?;
    let _ = std::fs::create_dir_all(&dir);
    Ok(dir.join("settings.json"))
}

pub fn load(app: &tauri::AppHandle) -> Settings {
    let path = match settings_path(app) {
        Ok(p) => p,
        Err(_) => return Settings::default(),
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &tauri::AppHandle, s: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// 读取完整 provider（含凭据管理器里的 key）。
pub fn load_provider(app: &tauri::AppHandle) -> Option<ProviderCfg> {
    let stored = load(app).provider?;
    let key = keyring_get().unwrap_or_default();
    let cfg = ProviderCfg {
        kind: stored.kind,
        model: stored.model,
        base: stored.base,
        key,
    };
    if cfg.is_usable() { Some(cfg) } else { None }
}

/// 保存 provider：key 进凭据管理器，其余进 settings.json。
pub fn save_provider(app: &tauri::AppHandle, cfg: &ProviderCfg) -> Result<(), String> {
    if !cfg.key.is_empty() {
        keyring_set(&cfg.key)?;
    } else {
        let _ = keyring_del();
    }
    let mut s = load(app);
    s.provider = Some(StoredProvider {
        kind: cfg.kind.clone(),
        model: cfg.model.clone(),
        base: cfg.base.clone(),
    });
    save(app, &s)
}

pub fn clear_provider(app: &tauri::AppHandle) -> Result<(), String> {
    let _ = keyring_del();
    let mut s = load(app);
    s.provider = None;
    save(app, &s)
}

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ENTRY).map_err(|e| format!("凭据管理器不可用：{e}"))
}

fn keyring_set(value: &str) -> Result<(), String> {
    let entry = keyring_entry()?;
    match entry.set_password(value) {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("写入凭据失败：{e}")),
    }
}

fn keyring_get() -> Result<String, String> {
    let entry = keyring_entry()?;
    match entry.get_password() {
        Ok(v) => Ok(v),
        Err(keyring::Error::NoEntry) => Ok(String::new()),
        Err(e) => Err(format!("读取凭据失败：{e}")),
    }
}

fn keyring_del() -> Result<(), String> {
    let entry = keyring_entry()?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除凭据失败：{e}")),
    }
}
