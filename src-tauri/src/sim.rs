//! 具身仿真台（/sim 工作台）的落盘命令：把 iframe 镜像内核采集的
//! episode 组装成 **LeRobotDataset v2.1** 目录树，写到
//! `%APPDATA%/cn.oriera.studio/sim-lab/exports/<时间戳>/`。
//!
//! v2.1 写入走**单源** `crate::lerobot`（engine
//! crates/oriera-agent/src/lerobot.rs 的 vendored 副本，
//! sync-engine-snapshot.mjs 防漂移）——本模块只做 payload 解析、
//! 目录命名与落盘报告，schema 零本地实现（#6 双实现已清除）。

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, Clone, Serialize)]
pub struct SimExportReport {
    pub dir: String,
    pub episodes: usize,
    pub frames: usize,
}

fn exports_root() -> Result<std::path::PathBuf, String> {
    let base = std::env::var("APPDATA").map_err(|_| "读取 APPDATA 失败".to_string())?;
    Ok(std::path::PathBuf::from(base)
        .join("cn.oriera.studio")
        .join("sim-lab")
        .join("exports"))
}

fn write(rel: &std::path::Path, content: &str) -> Result<(), String> {
    if let Some(dir) = rel.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败：{e}"))?;
    }
    std::fs::write(rel, content).map_err(|e| format!("写文件失败：{e}"))
}

/// 把 Sim 工作台采集的 episodes 组装成 LeRobot v2.1 目录树（单源
/// `crate::lerobot` 写入；payload 形如 {task, fps, obsNames, actionNames,
/// episodes:[{task, rows:[{state:[f32], action:[f32]}]}]}）。
#[tauri::command]
pub fn sim_export_lerobot(
    state: State<'_, crate::AppState>,
    payload: String,
) -> Result<SimExportReport, String> {
    let _ = state.snapshot_version;
    let v: serde_json::Value =
        serde_json::from_str(&payload).map_err(|e| format!("payload 非法：{e}"))?;
    let eps = v
        .get("episodes")
        .and_then(|x| x.as_array())
        .ok_or("payload 缺 episodes")?;
    if eps.is_empty() {
        return Err("没有可导出的 episode".into());
    }
    let fps = v.get("fps").and_then(|x| x.as_f64()).unwrap_or(60.0) as f32;
    let default_task = v
        .get("task")
        .and_then(|x| x.as_str())
        .unwrap_or("pick up the cube")
        .to_string();
    let obs_names: Vec<String> = v
        .get("obsNames")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .ok_or("payload 缺 obsNames")?;
    let act_names: Vec<String> = v
        .get("actionNames")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .ok_or("payload 缺 actionNames")?;

    let stamp = chrono_like_stamp();
    let root = exports_root()?.join(format!("{stamp}-lerobot-v21"));

    // 单源写入：engine 的 LeRobotV21 承担全部 v2.1 schema。
    let mut ds = crate::lerobot::LeRobotV21::new(
        fps,
        "oriera-arm-6-js",
        obs_names.clone(),
        act_names.clone(),
    );
    for (i, ep) in eps.iter().enumerate() {
        let task = ep
            .get("task")
            .and_then(|x| x.as_str())
            .unwrap_or(&default_task)
            .to_string();
        let rows = ep
            .get("rows")
            .and_then(|x| x.as_array())
            .ok_or(format!("episode {i} 缺 rows"))?;
        ds.start_episode(&task);
        for (ri, row) in rows.iter().enumerate() {
            let st: Vec<f32> = row
                .get("state")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|f| f.as_f64())
                        .map(|f| f as f32)
                        .collect()
                })
                .ok_or(format!("episode {i} row {ri} 缺 state"))?;
            let ac: Vec<f32> = row
                .get("action")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|f| f.as_f64())
                        .map(|f| f as f32)
                        .collect()
                })
                .ok_or(format!("episode {i} row {ri} 缺 action"))?;
            ds.push_step(&st, &ac)?;
        }
        ds.finish_episode();
    }
    let frames = ds.total_frames();
    ds.write_dir(&root)
        .map_err(|e| format!("写数据集失败：{e}"))?;
    crate::lerobot::LeRobotV21::write_receipt(
        &root,
        concat!("oriera-studio/", env!("CARGO_PKG_VERSION")),
        "rigid3d-js-mirror",
        1.0 / fps,
        &[],
        0,
    )
    .map_err(|e| format!("写回执失败：{e}"))?;
    crate::lerobot::LeRobotV21::write_card(&root, &default_task)
        .map_err(|e| format!("写卡片失败：{e}"))?;

    let latest = exports_root()?.join("LATEST");
    write(&latest, &root.to_string_lossy())?;

    Ok(SimExportReport {
        dir: root.to_string_lossy().to_string(),
        episodes: eps.len(),
        frames,
    })
}

/// 在资源管理器里打开最近一次导出目录。
#[tauri::command]
pub fn sim_reveal_export(app: AppHandle) -> Result<(), String> {
    let latest = exports_root()?.join("LATEST");
    let dir = std::fs::read_to_string(&latest).map_err(|_| "还没有导出记录".to_string())?;
    let dir = std::path::PathBuf::from(dir.trim());
    if !dir.is_dir() {
        return Err("导出目录不存在".into());
    }
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| format!("打开目录失败：{e}"))
}

/// 时间戳目录名（本地时间，无外部依赖）。
fn chrono_like_stamp() -> String {
    // std 无本地时区；用 UNIX 秒 + 由前端传时间的折中：这里用进程启动
    // 以来的纳秒尾数不足以人读——改为从 SystemTime 取 UNIX 秒再换算
    // UTC（误差=时区偏移，仅影响目录名可读性，不影响唯一性）。
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // UTC+8（本机主要使用场景）下的可读时间戳
    let (y, mo, d, h, mi, s) = utc_plus8(secs);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

fn utc_plus8(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let secs = secs + 8 * 3600;
    let days = secs / 86400;
    let rem = secs % 86400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // civil-from-days（Howard Hinnant 算法）
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u64, m as u64, d as u64, h, mi, s)
}
