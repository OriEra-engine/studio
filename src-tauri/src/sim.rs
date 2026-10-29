//! 具身仿真台（/sim 工作台）的落盘命令：把 iframe 镜像内核采集的
//! episode 组装成 **LeRobotDataset v2.1** 目录树（meta 全套 + JSONL 行）
//! 写到 `%APPDATA%/cn.oriera.studio/sim-lab/exports/<时间戳>/`。
//!
//! 行格式与引擎 Rust 导出器（engine crates/oriera-agent/src/lerobot.rs）
//! 同契约：data 层 JSONL 列名即 v2.1 parquet 列名，官方转换脚本
//! `scripts/jsonl_to_lerobot_v21.py`（engine 仓）1:1 转 parquet 后
//! lerobot-train 直读。serde_json::Value 透明存取，schema 前端持有
//! （Studio 命令层惯例）。

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

fn jarr(v: &[f64]) -> String {
    let parts: Vec<String> = v.iter().map(|x| format!("{x:.6}")).collect();
    format!("[{}]", parts.join(","))
}

fn stats_block(rows: &[Vec<Vec<f64>>], dim: usize) -> String {
    // 逐维 min/max/mean/std（与引擎 lerobot.rs 的 stats.json 同形状）
    if rows.is_empty() || dim == 0 {
        return "{\"min\":[],\"max\":[],\"mean\":[],\"std\":[],\"count\":0}".into();
    }
    let mut min = vec![f64::INFINITY; dim];
    let mut max = vec![f64::NEG_INFINITY; dim];
    let mut sum = vec![0.0f64; dim];
    let mut sq = vec![0.0f64; dim];
    let mut count = 0usize;
    for row in rows {
        for r in row {
            count += 1;
            for (i, x) in r.iter().take(dim).enumerate() {
                min[i] = min[i].min(*x);
                max[i] = max[i].max(*x);
                sum[i] += x;
                sq[i] += x * x;
            }
        }
    }
    let mean: Vec<String> = sum
        .iter()
        .map(|s| format!("{:.7}", s / count as f64))
        .collect();
    let std: Vec<String> = sq
        .iter()
        .zip(sum.iter())
        .map(|(s2, s)| {
            let var = (s2 / count as f64) - (s / count as f64) * (s / count as f64);
            format!("{:.7}", var.max(0.0).sqrt())
        })
        .collect();
    format!(
        "{{\"min\":{},\"max\":{},\"mean\":[{}],\"std\":[{}],\"count\":{}}}",
        jarr(&min),
        jarr(&max),
        mean.join(","),
        std.join(","),
        count
    )
}

fn write(rel: &std::path::Path, content: &str) -> Result<(), String> {
    if let Some(dir) = rel.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败：{e}"))?;
    }
    std::fs::write(rel, content).map_err(|e| format!("写文件失败：{e}"))
}

/// 把 Sim 工作台采集的 episodes 组装成 LeRobot v2.1 目录树。
/// payload 形如 {task, fps, obsNames, actionNames,
/// episodes:[{task, rows:[{state:[f32], action:[f32]}]}]}
#[tauri::command]
pub fn sim_export_lerobot(
    state: State<'_, crate::AppState>,
    payload: String,
) -> Result<SimExportReport, String> {
    let _ = state.snapshot_version; // 目录名时间戳已含版本语境；占位取用防误删
    let v: serde_json::Value =
        serde_json::from_str(&payload).map_err(|e| format!("payload 非法：{e}"))?;
    let eps = v
        .get("episodes")
        .and_then(|x| x.as_array())
        .ok_or("payload 缺 episodes")?;
    if eps.is_empty() {
        return Err("没有可导出的 episode".into());
    }
    let fps = v.get("fps").and_then(|x| x.as_f64()).unwrap_or(60.0);
    let default_task = v
        .get("task")
        .and_then(|x| x.as_str())
        .unwrap_or("pick up the cube")
        .to_string();
    let obs_names = v
        .get("obsNames")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(|x| x.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let act_names = v
        .get("actionNames")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(|x| x.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if obs_names.is_empty() || act_names.is_empty() {
        return Err("payload 缺 obsNames/actionNames".into());
    }

    let stamp = chrono_like_stamp();
    let root = exports_root()?.join(format!("{stamp}-lerobot-v21"));

    // 逐集落 JSONL 行 + 收集统计
    let mut tasks: Vec<String> = Vec::new();
    let mut ep_meta = String::new();
    let mut all_states: Vec<Vec<Vec<f64>>> = Vec::new();
    let mut all_actions: Vec<Vec<Vec<f64>>> = Vec::new();
    let mut all_ts: Vec<Vec<f64>> = Vec::new();
    let mut total_frames = 0usize;
    let mut global_index = 0usize;
    for (i, ep) in eps.iter().enumerate() {
        let rows = ep
            .get("rows")
            .and_then(|x| x.as_array())
            .ok_or(format!("episode {i} 缺 rows"))?;
        let task = ep
            .get("task")
            .and_then(|x| x.as_str())
            .unwrap_or(&default_task)
            .to_string();
        let task_index = match tasks.iter().position(|t| *t == task) {
            Some(k) => k,
            None => {
                tasks.push(task.clone());
                tasks.len() - 1
            }
        };
        let mut lines = String::new();
        let mut ep_states: Vec<Vec<f64>> = Vec::new();
        let mut ep_actions: Vec<Vec<f64>> = Vec::new();
        for (ri, row) in rows.iter().enumerate() {
            let st: Vec<f64> = row
                .get("state")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|f| f.as_f64()).collect())
                .ok_or(format!("episode {i} row {ri} 缺 state"))?;
            let ac: Vec<f64> = row
                .get("action")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|f| f.as_f64()).collect())
                .ok_or(format!("episode {i} row {ri} 缺 action"))?;
            if st.len() != obs_names.len() || ac.len() != act_names.len() {
                return Err(format!(
                    "episode {i} row {ri} 维度不符：state {}/{} action {}/{}",
                    st.len(),
                    obs_names.len(),
                    ac.len(),
                    act_names.len()
                ));
            }
            let ts = ri as f64 / fps;
            lines.push_str(&format!(
                "{{\"observation.state\":{},\"action\":{},\"timestamp\":{ts:.6},\"frame_index\":{ri},\"episode_index\":{i},\"index\":{},\"task_index\":{task_index}}}\n",
                jarr(&st),
                jarr(&ac),
                global_index
            ));
            global_index += 1;
            ep_states.push(st);
            ep_actions.push(ac);
            all_ts.push(vec![ts]);
        }
        total_frames += rows.len();
        ep_meta.push_str(&format!(
            "{{\"episode_index\":{i},\"tasks\":[{}],\"length\":{}}}\n",
            serde_json::to_string(&task).unwrap_or_else(|_| "\"task\"".into()),
            rows.len()
        ));
        all_states.push(ep_states);
        all_actions.push(ep_actions);
        let path = root.join(format!("data/chunk-000/episode_{i:06}.jsonl"));
        write(&path, &lines)?;
    }

    // meta 全套
    let names_json = |items: &[String]| {
        let parts: Vec<String> = items.iter().map(|s| format!("\"{s}\"")).collect();
        format!("[{}]", parts.join(","))
    };
    let info = format!(
        concat!(
            "{{\"codebase_version\":\"v2.1\",\"robot_type\":\"oriera-arm-6-js\",",
            "\"total_episodes\":{eps},\"total_frames\":{frames},",
            "\"total_tasks\":{tasks_n},\"total_videos\":0,\"total_chunks\":1,",
            "\"chunks_size\":1000,\"fps\":{fps},\"splits\":{{\"train\":\"0:{eps}\"}},",
            "\"data_path\":\"data/chunk-{{episode_chunk:03d}}/episode_{{episode_index:06d}}.parquet\",",
            "\"video_path\":\"videos/chunk-{{episode_chunk:03d}}/{{video_key}}/episode_{{episode_index:06d}}.mp4\",",
            "\"features\":{{",
            "\"observation.state\":{{\"dtype\":\"float32\",\"shape\":[{sd}],\"names\":{{\"joints\":{on}}}}},",
            "\"action\":{{\"dtype\":\"float32\",\"shape\":[{ad}],\"names\":{{\"joints\":{an}}}}},",
            "\"timestamp\":{{\"dtype\":\"float32\",\"shape\":[1],\"names\":null}},",
            "\"frame_index\":{{\"dtype\":\"int64\",\"shape\":[1],\"names\":null}},",
            "\"episode_index\":{{\"dtype\":\"int64\",\"shape\":[1],\"names\":null}},",
            "\"index\":{{\"dtype\":\"int64\",\"shape\":[1],\"names\":null}},",
            "\"task_index\":{{\"dtype\":\"int64\",\"shape\":[1],\"names\":null}}",
            "}}}}\n"
        ),
        eps = eps.len(),
        frames = total_frames,
        tasks_n = tasks.len(),
        fps = fps,
        sd = obs_names.len(),
        ad = act_names.len(),
        on = names_json(&obs_names),
        an = names_json(&act_names),
    );
    write(&root.join("meta/info.json"), &info)?;
    write(&root.join("meta/episodes.jsonl"), &ep_meta)?;
    let tasks_lines: String = tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            format!(
                "{{\"task_index\":{i},\"task\":{}}}\n",
                serde_json::to_string(t).unwrap_or_default()
            )
        })
        .collect();
    write(&root.join("meta/tasks.jsonl"), &tasks_lines)?;
    let ts_rows: Vec<Vec<Vec<f64>>> = vec![all_ts];
    let stats = format!(
        "{{\"observation.state\":{},\"action\":{},\"timestamp\":{}}}\n",
        stats_block(&all_states, obs_names.len()),
        stats_block(&all_actions, act_names.len()),
        stats_block(&ts_rows, 1),
    );
    write(&root.join("meta/stats.json"), &stats)?;
    write(
        &root.join("meta/oriera_receipt.json"),
        &format!(
            concat!(
                "{{\"codebase_version\":\"oriera-embodied-v2-js\",",
                "\"engine\":\"oriera-studio/{ver}/{snap}\",",
                "\"backend\":\"rigid3d-js-mirror\",",
                "\"note\":\"交互镜像采集；确定性逐集指纹回执请用引擎 embodied_sim_collect\"}}\n"
            ),
            ver = env!("CARGO_PKG_VERSION"),
            snap = state.snapshot_version
        ),
    )?;
    write(
        &root.join("README.md"),
        &format!(
            "# OriEra 具身仿真台采集数据集（LeRobotDataset v2.1）\n\n任务：{task}\n\n转换：python engine/scripts/jsonl_to_lerobot_v21.py <本目录>\n训练：lerobot-train --dataset.repo_id=<user>/<ds> --policy.type=act\n",
            task = default_task
        ),
    )?;

    // 记录最近一次导出路径（reveal 用）
    let latest = exports_root()?.join("LATEST");
    write(&latest, &root.to_string_lossy())?;

    Ok(SimExportReport {
        dir: root.to_string_lossy().to_string(),
        episodes: eps.len(),
        frames: total_frames,
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
