//! 硬件体检：CPU / 内存 / GPU+显存。
//! Windows 上用 CIM（PowerShell 无依赖）+ nvidia-smi（NVIDIA 精确显存），
//! 显存缺失时按常见显卡型号启发式估算。

use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GpuInfo {
    pub name: String,
    /// MB；None 表示未能获取
    pub vram_mb: Option<u32>,
    /// 显存来源：nvidia-smi / 系统 / 型号估算
    pub vram_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HardwareInfo {
    pub cpu: String,
    pub cores: u32,
    pub ram_gb: f32,
    pub gpus: Vec<GpuInfo>,
    /// 所有 GPU 中的最大显存（MB）
    pub max_vram_mb: Option<u32>,
    pub os: String,
}

/// 常见显卡显存估算表（MB），仅当系统查不到精确值时兜底。
fn vram_heuristic(name: &str) -> Option<u32> {
    let table: &[(&str, u32)] = &[
        // NVIDIA
        ("5090", 32764), ("5080", 16384), ("5070 ti", 16384), ("5070", 12288), ("5060 ti", 8192), ("5060", 8192), ("5050", 8192),
        ("4090", 24564), ("4080", 16384), ("4070 ti", 12288), ("4070", 12288), ("4060 ti", 8192), ("4060", 8192), ("4050", 6144),
        ("3090", 24564), ("3080", 10240), ("3070", 8192), ("3060", 12288), ("3050", 8192),
        ("2080 ti", 11264), ("2080", 8192), ("2070", 8192), ("2060", 6144),
        ("1080 ti", 11264), ("1080", 8192), ("1070", 8192), ("1060", 6144), ("1050", 4096),
        ("1650", 4096), ("1660", 6144),
        // AMD
        ("7900 xtx", 24564), ("7900 xt", 20480), ("7800 xt", 16384), ("7700 xt", 12288), ("7600", 8192),
        ("6950", 16384), ("6900", 16384), ("6800", 16384), ("6750", 12288), ("6700", 12288), ("6650", 8192), ("6600", 8192), ("6500", 4096),
        // Intel
        ("a770", 16384), ("a750", 8192), ("a380", 6144), ("b580", 12288), ("b570", 12288),
    ];
    let lower = name.to_lowercase();
    // 顺序重要：更长/更强的型号先匹配
    for (key, mb) in table {
        if lower.contains(key) {
            return Some(*mb);
        }
    }
    None
}

#[derive(Deserialize)]
struct CimGpu {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    adapter_ram: Option<u64>, // 注意 Win32_VideoController 是 uint32，超过 4GB 会溢出/截断
}

#[derive(Deserialize)]
struct CimReport {
    #[serde(default)]
    cpu: Option<String>,
    #[serde(default)]
    cores: Option<u32>,
    #[serde(default)]
    ram: Option<f64>,
    #[serde(default)]
    gpus: Vec<CimGpu>,
}

fn query_cim() -> Option<CimReport> {
    let script = r#"
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$ram = [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1GB,1)
$gpus = @(Get-CimInstance Win32_VideoController | Where-Object { $_.Name -and $_.Name -notmatch 'Basic Render|Microsoft Basic|Virtual|Remote|Indirect' } | ForEach-Object { @{ name = $_.Name; adapter_ram = $_.AdapterRAM } })
@{ cpu = $cpu.Name; cores = $cpu.NumberOfLogicalProcessors; ram = $ram; gpus = $gpus } | ConvertTo-Json -Compress
"#;
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str::<CimReport>(text.trim()).ok()
}

/// nvidia-smi 精确显存（MB）。
fn nvidia_vram() -> Option<(String, u32)> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?.trim();
    let mut parts = line.split(',');
    let name = parts.next()?.trim().to_string();
    let mb: u32 = parts.next()?.trim().parse().ok()?;
    Some((name, mb))
}

pub fn detect() -> HardwareInfo {
    let mut info = HardwareInfo {
        os: format!("Windows {}", std::env::consts::ARCH),
        ..Default::default()
    };

    if let Some(rep) = query_cim() {
        info.cpu = rep.cpu.unwrap_or_default();
        info.cores = rep.cores.unwrap_or(0);
        info.ram_gb = rep.ram.map(|v| v as f32).unwrap_or(0.0);
        info.gpus = rep
            .gpus
            .into_iter()
            .filter_map(|g| {
                g.name.map(|name| {
                    // Win32_VideoController 的 AdapterRAM 是 uint32，4GB 以上会溢出：
                    // 仅当 0 < 值 < 4095MB 时采信，否则走型号估算
                    let sys_mb = g.adapter_ram.map(|r| (r / (1024 * 1024)) as u32);
                    let (vram, src) = match sys_mb {
                        Some(mb) if mb > 0 && mb < 4095 => (Some(mb), "系统".to_string()),
                        _ => match vram_heuristic(&name) {
                            Some(mb) => (Some(mb), "型号估算".to_string()),
                            None => (None, "未知".to_string()),
                        },
                    };
                    GpuInfo { name, vram_mb: vram, vram_source: src }
                })
            })
            .collect();
    }

    // NVIDIA 精确值优先覆盖
    if let Some((smi_name, mb)) = nvidia_vram() {
        if let Some(g) = info
            .gpus
            .iter_mut()
            .find(|g| g.name.to_lowercase().contains("nvidia") || g.name == smi_name)
        {
            g.vram_mb = Some(mb);
            g.vram_source = "nvidia-smi".into();
        }
    }

    info.max_vram_mb = info
        .gpus
        .iter()
        .filter_map(|g| g.vram_mb)
        .max();
    info
}
