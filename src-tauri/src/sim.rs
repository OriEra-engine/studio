//! 具身仿真台（/sim 工作台）的落盘命令：把 iframe 镜像内核采集的
//! episode 组装成 **LeRobotDataset** 目录树，写到
//! `%APPDATA%/cn.oriera.studio/sim-lab/exports/<时间戳>/`。
//!
//! 三种编码，一份采集（多格式出口按需分发，单源 `crate::lerobot`
//! 承担全部 schema）：
//! - `v21`（**默认，字节冻结**）：LeRobotDataset v2.1 树，既有路径零变化；
//! - `v3`：LeRobotDataset v3.0 树（lerobot ≥ 0.4 直读）——`write_dir_v3`
//!   + 回执 + v3 卡片，与 engine `embodied_sim_collect --lerobot-v3` 同构；
//! - `npz`：numpy 直读裸数组（每集一个 `.npz`）——`write_npz` + 回执，
//!   与 engine `--export npz` 同构（按 vendored 版既有设计，不自创格式）。
//!
//! v2.1 写入走**单源** `crate::lerobot`（engine
//! crates/oriera-agent/src/lerobot.rs 的 vendored 副本，
//! sync-engine-snapshot.mjs 防漂移）——本模块只做 payload 解析、
//! 目录命名与落盘报告，schema 零本地实现（#6 双实现已清除）。
//!
//! **多相机/深度轨（已知收窄，如实声明）**：画面内核（kernel.js 镜像）
//! 目前只逐 tick 记 (state, action) 行，不采相机帧（右上角俯视 PiP 是
//! scissor 屏上渲染，从未落像素）；spec 的 `cameras` 段也尚未进内核采集
//! 链。因此三种格式导出的都是**纯状态数据集**：v3 树无视频轨
//! （info.json `video_path:null`，vendored 版对无相机数据集的合法形态）、
//! npz 只含 obs/act 数组。多相机采集待内核支持后再接
//! （vendored `CameraTrack`/`push_frame`/`add_camera_channels` 契约已就绪，
//! 到时只需透传内核帧，落盘侧零新格式）。**不许造假帧。**

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, Clone, Serialize)]
pub struct SimExportReport {
    pub dir: String,
    pub episodes: usize,
    pub frames: usize,
    /// 本次落盘用的编码（"v21" / "v3" / "npz"）——UI 据此说明训练路径。
    pub format: String,
}

/// 导出编码（`sim_export_lerobot` 的 `format` 参数）。缺省 = v2.1
/// （既有调用方/旧前端不带该参数，默认路径字节冻结）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    V21,
    V30,
    Npz,
}

impl ExportFormat {
    /// 目录名后缀（v3 与 engine 的 `lerobot-v30`、npz 与 `--export npz`
    /// 的 `npz/` 同名——同一份数据在 Studio 与 CLI 落出同样的目录身份）。
    fn dir_suffix(self) -> &'static str {
        match self {
            ExportFormat::V21 => "lerobot-v21",
            ExportFormat::V30 => "lerobot-v30",
            ExportFormat::Npz => "npz",
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            ExportFormat::V21 => "v21",
            ExportFormat::V30 => "v3",
            ExportFormat::Npz => "npz",
        }
    }
}

/// `format` 参数 → 编码。`None`/`""`/`"v21"` = 默认 v2.1（字节冻结路径）。
fn normalize_format(format: Option<&str>) -> Result<ExportFormat, String> {
    match format.unwrap_or("") {
        "" | "v21" => Ok(ExportFormat::V21),
        "v3" | "v30" => Ok(ExportFormat::V30),
        "npz" => Ok(ExportFormat::Npz),
        other => Err(format!(
            "未知导出格式 {other:?}（可选：v21〔默认〕/ v3 / npz）"
        )),
    }
}

/// 内核 episodes 消息的解析结果（三种格式共用一份解析：同一采集、
/// 多种编码——字段语义与缺省兜底见 [`parse_payload`]）。
struct ParsedPayload {
    fps: f32,
    default_task: String,
    robot: String,
    spec_hash: String,
    /// 逐集回执指纹（十进制 u64 串已解析）：v1 状态口径 / v2 身份口径。
    fp_v1: Vec<u64>,
    fp_v2: Vec<u64>,
    obs_names: Vec<String>,
    act_names: Vec<String>,
    /// 每集：任务串 + (state, action) 行。
    episodes: Vec<(String, Vec<(Vec<f32>, Vec<f32>)>)>,
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

/// 解析内核 episodes 消息（三种格式共用；护栏与错误措辞与既有 v2.1
/// 路径逐字一致）。payload 形如 {task, fps, robot, specHash, obsNames,
/// actionNames, episodes:[{task, fingerprint, fingerprintV2,
/// rows:[{state:[f32], action:[f32]}]}]}——robot/specHash/逐集指纹由
/// 内核随消息透传（P2-W5A），落盘侧不再硬编码机器人名。
fn parse_payload(payload: &str) -> Result<ParsedPayload, String> {
    // 资源耗尽护栏（安全深检）：payload 上限 50MB / 2 万行。
    if payload.len() > 50 * 1024 * 1024 {
        return Err("payload 超限（>50MB）".into());
    }
    let v: serde_json::Value =
        serde_json::from_str(payload).map_err(|e| format!("payload 非法：{e}"))?;
    if let Some(eps) = v.get("episodes").and_then(|x| x.as_array()) {
        let total_rows: usize = eps
            .iter()
            .filter_map(|e| e.get("rows").and_then(|r| r.as_array()))
            .map(|r| r.len())
            .sum();
        if total_rows > 20_000 {
            return Err(format!("总行数超限（{total_rows} > 20000）——请分批导出"));
        }
    }
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
    // robot 名透传（内核按解析后的 spec 出 robot.name，如 tabletop-4dof /
    // panda7）——info.json 的 robot_type 不再硬编码；旧内核快照缺字段时
    // 兜底一个不冒领型号的通用名。
    let robot = v
        .get("robot")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("oriera-arm")
        .to_string();
    // spec 身份哈希（内核 spec_hash()，16 hex）——进 oriera_receipt.json。
    let spec_hash = v
        .get("specHash")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    // 逐集回执指纹（P2-W5A）：内核每集结束抓 wasm 的 fingerprint()（v1
    // 状态口径）与 fingerprint_v2()（状态⊕身份⊕spec_hash），十进制 u64 串
    // 随 episode 透传。任一集缺字段或解析失败（旧内核快照）→ 整组置空：
    // 回执宁可没有逐集指纹，也不写错位/半截的。
    let mut fp_v1: Vec<u64> = Vec::new();
    let mut fp_v2: Vec<u64> = Vec::new();
    let parse_fps = |key: &str| -> Option<Vec<u64>> {
        eps.iter()
            .map(|ep| {
                ep.get(key)
                    .and_then(|x| x.as_str())
                    .and_then(|s| s.parse::<u64>().ok())
            })
            .collect()
    };
    if eps.iter().all(|ep| {
        ep.get("fingerprint").and_then(|x| x.as_str()).is_some()
            && ep.get("fingerprintV2").and_then(|x| x.as_str()).is_some()
    }) {
        if let (Some(a), Some(b)) = (parse_fps("fingerprint"), parse_fps("fingerprintV2")) {
            fp_v1 = a;
            fp_v2 = b;
        }
    }
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
    let mut episodes = Vec::with_capacity(eps.len());
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
        let mut parsed_rows = Vec::with_capacity(rows.len());
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
            parsed_rows.push((st, ac));
        }
        episodes.push((task, parsed_rows));
    }
    Ok(ParsedPayload {
        fps,
        default_task,
        robot,
        spec_hash,
        fp_v1,
        fp_v2,
        obs_names,
        act_names,
        episodes,
    })
}

/// 把解析后的采集组装成 vendored [`crate::lerobot::LeRobotV21`]（三种
/// 编码共用同一份 ds：engine 同款「一个数据集、多种编码」语义）。
fn build_dataset(p: &ParsedPayload) -> Result<crate::lerobot::LeRobotV21, String> {
    let mut ds = crate::lerobot::LeRobotV21::new(
        p.fps,
        &p.robot,
        p.obs_names.clone(),
        p.act_names.clone(),
    );
    for (task, rows) in &p.episodes {
        ds.start_episode(task);
        for (st, ac) in rows {
            // 行维与 obsNames/actionNames 同源同长，契约不符（内核消息
            // 与维名表不一致）是可读 Error，不是 panic。
            ds.push_step(st, ac)?;
        }
        ds.finish_episode();
    }
    Ok(ds)
}

/// 回执（逐集指纹 v1+v2 双口径）随**每种**编码落一份
/// `meta/oriera_receipt.json`（engine collect 的同款：身份随编码走）。
/// v2 为空时 write_receipt 自动省略 scheme 字段（旧内核快照 = 字节兼容
/// 的退化回执）。successes 恒 0：工作台不判成功率（作业证据以 CLI
/// eval_report 为准，见工作台「确定性说明」）。
fn write_receipt_at(root: &std::path::Path, p: &ParsedPayload) -> Result<(), String> {
    crate::lerobot::LeRobotV21::write_receipt(
        root,
        concat!("oriera-studio/", env!("CARGO_PKG_VERSION")),
        "rigid3d-js-mirror",
        1.0 / p.fps,
        &p.spec_hash,
        &p.fp_v1,
        0,
        "v2",
        &p.fp_v2,
    )
    .map_err(|e| format!("写回执失败：{e}"))
}

/// 按格式落盘到 `root`（不碰 exports 根、不写 LATEST——命令壳负责）。
/// 返回 (episodes, frames)。
fn write_format_at(
    format: ExportFormat,
    p: &ParsedPayload,
    root: &std::path::Path,
) -> Result<(usize, usize), String> {
    let ds = build_dataset(p)?;
    let frames = ds.total_frames();
    match format {
        // v2.1 既有导出（字节冻结）：write_dir → 回执 → 卡片，调用序列
        // 与错误措辞与重构前逐字一致（golden 测试钉死字节）。
        ExportFormat::V21 => {
            ds.write_dir(root).map_err(|e| format!("写数据集失败：{e}"))?;
            write_receipt_at(root, p)?;
            crate::lerobot::LeRobotV21::write_card(root, &p.default_task)
                .map_err(|e| format!("写卡片失败：{e}"))?;
        }
        // v3.0 树（lerobot ≥ 0.4 直读）：write_dir_v3 → 同一份回执 →
        // v3 卡片（engine `--lerobot-v3` 的落盘序列）。无相机帧 ⇒ 无视频
        // 轨（info.json video_path:null），纯状态数据集的合法 v3 形态。
        ExportFormat::V30 => {
            ds.write_dir_v3(root)
                .map_err(|e| format!("写数据集失败：{e}"))?;
            write_receipt_at(root, p)?;
            crate::lerobot::LeRobotV21::write_card_v3(root, &p.default_task)
                .map_err(|e| format!("写卡片失败：{e}"))?;
        }
        // npz 裸导出（numpy 直读，每集一个 .npz）→ 同一份回执（engine
        // `--export npz` 的落盘序列；无卡片——与 engine 一致）。
        ExportFormat::Npz => {
            ds.write_npz(root)
                .map_err(|e| format!("写 npz 导出失败：{e}"))?;
            write_receipt_at(root, p)?;
        }
    }
    Ok((p.episodes.len(), frames))
}

/// 解析 + 按格式落盘到 `root`（命令壳与测试共用的核心；不含 LATEST）。
fn export_into_at(
    format: ExportFormat,
    payload: &str,
    root: &std::path::Path,
) -> Result<SimExportReport, String> {
    let p = parse_payload(payload)?;
    let (episodes, frames) = write_format_at(format, &p, root)?;
    Ok(SimExportReport {
        dir: root.to_string_lossy().to_string(),
        episodes,
        frames,
        format: format.as_str().to_string(),
    })
}

/// 把 Sim 工作台采集的 episodes 组装成 LeRobot 数据集（`format` 选编码：
/// v2.1〔默认，字节冻结〕/ v3.0 / npz——同一份采集、多种编码，逐集回执
/// 指纹随每种编码落 `meta/oriera_receipt.json`）。
#[tauri::command]
pub fn sim_export_lerobot(
    state: State<'_, crate::AppState>,
    payload: String,
    format: Option<String>,
) -> Result<SimExportReport, String> {
    let _ = state.snapshot_version;
    let format = normalize_format(format.as_deref())?;
    let stamp = chrono_like_stamp();
    let root = exports_root()?.join(format!("{stamp}-{}", format.dir_suffix()));
    let report = export_into_at(format, &payload, &root)?;

    let latest = exports_root()?.join("LATEST");
    write(&latest, &root.to_string_lossy())?;
    Ok(report)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 与内核 episodes 消息同形的固定 payload（2 集 / 3 行，含 v1+v2
    /// 逐集指纹）——v2.1 golden 字节自**改动前**代码实测钉死（见
    /// v21_tree_is_byte_frozen），三格式共用。
    const PAYLOAD: &str = r#"{
        "task": "pick up the cube and place it at the target",
        "fps": 60,
        "robot": "tabletop-4dof",
        "specHash": "a1b2c3d4e5f60718",
        "obsNames": ["j0","j1","j2","j3","jv0","jv1","jv2","jv3","grip","ee.x","ee.y","ee.z","obj.x","obj.y","obj.z","goal.x","goal.y","goal.z","hold","done","t"],
        "actionNames": ["a0","a1","a2","a3","ag"],
        "episodes": [
            {"task": "pick up the cube and place it at the target",
             "fingerprint": "12345678901234567890", "fingerprintV2": "9876543210987654321",
             "rows": [
                {"state": [0.0,0.1,0.2,0.3,0.0,0.0,0.0,0.0,0.5,0.1,0.0,0.3,0.05,0.0,0.02,0.1,0.0,0.25,0,0,0], "action": [0.01,0.0,-0.01,0.0,0.0]},
                {"state": [0.01,0.1,0.2,0.3,0.1,0.0,0.0,0.0,0.5,0.11,0.0,0.3,0.05,0.0,0.02,0.1,0.0,0.25,0,0,1], "action": [0.02,0.01,0.0,0.0,0.5]}
             ]},
            {"task": "second task phrase",
             "fingerprint": "42", "fingerprintV2": "7",
             "rows": [
                {"state": [1.0,0.9,0.8,0.7,0.0,0.1,0.0,0.0,0.0,0.2,0.1,0.3,0.1,0.05,0.0,0.15,0.05,0.2,1,1,2], "action": [-0.01,0.0,0.01,0.0,1.0]}
             ]}
        ]
    }"#;

    fn fmd5(b: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &x in b {
            h = (h ^ x as u64).wrapping_mul(0x100_0000_01b3);
        }
        h
    }

    /// 测试沙盒根（每次调用一个新的子目录，进程级唯一）。
    fn sandbox(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("oriera-sim-test")
            .join(format!("{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn list_tree(root: &std::path::Path) -> Vec<(String, usize, u64)> {
        fn walk(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<(String, usize, u64)>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, base, out);
                } else {
                    let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                    let b = std::fs::read(&p).unwrap();
                    out.push((rel, b.len(), fmd5(&b)));
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }

    fn read(root: &std::path::Path, rel: &str) -> Vec<u8> {
        std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("读 {rel} 失败：{e}"))
    }

    // ── v2.1 字节冻结（默认路径） ─────────────────────────────────
    //
    // golden 采集自**重构前**代码（原 sim_export_lerobot 命令体）对同一
    // PAYLOAD 的实测输出：文件集 + 每文件 (len, FNV-1a) + MANIFEST/回执
    // 全文。MANIFEST 钉死 info/stats/tasks/episodes/data/modality 七类
    // 文件的 vendored content_hash ⇒ v2.1 树逐字节覆盖。

    const GOLDEN_FILES: &[(&str, usize, u64)] = &[
        ("MANIFEST.jsonl", 418, 0xe480_bc4c_ce46_8860),
        ("README.md", 642, 0xd9f0_6761_2f6a_e046),
        ("data/chunk-000/episode_000000.jsonl", 499, 0xbd7e_7376_7590_398c),
        ("data/chunk-000/episode_000001.jsonl", 249, 0x51f0_9ca3_5185_a20f),
        ("meta/episodes.jsonl", 149, 0x1bed_8a05_e5da_36ad),
        ("meta/info.json", 987, 0xd5cc_d26e_70e9_7191),
        ("meta/modality.json", 294, 0x2857_3f71_1205_9c70),
        ("meta/oriera_receipt.json", 412, 0x5fb3_bc84_2f5d_018d),
        ("meta/stats.json", 1003, 0xaa68_f7b8_bc4d_134d),
        ("meta/tasks.jsonl", 115, 0x78ad_b614_a5cb_c1b2),
    ];

    const GOLDEN_MANIFEST: &str = concat!(
        r#"{"file":"meta/info.json","hash":"3de55e199d75fd93"}"#, "\n",
        r#"{"file":"meta/stats.json","hash":"a481ac2873110954"}"#, "\n",
        r#"{"file":"meta/tasks.jsonl","hash":"606a2527a0f9b531"}"#, "\n",
        r#"{"file":"meta/episodes.jsonl","hash":"f7a4fd1c95ea8e8c"}"#, "\n",
        r#"{"file":"data/chunk-000/episode_000000.jsonl","hash":"f4695a3972d6b26a"}"#, "\n",
        r#"{"file":"data/chunk-000/episode_000001.jsonl","hash":"5e53cd09a2159d05"}"#, "\n",
        r#"{"file":"meta/modality.json","hash":"e7b65dbe1f2243e9"}"#, "\n",
    );

    const GOLDEN_RECEIPT: &str = concat!(
        r#"{"codebase_version":"oriera-embodied-v2","engine":"oriera-studio/0.16.0","backend":"rigid3d-js-mirror","dt":0.01666667,"spec_hash":"a1b2c3d4e5f60718","fingerprint_scheme":"v2","episode_fingerprints_v2":[9876543210987654321,7],"episode_fingerprints_v1":[12345678901234567890,42],"episode_state_hashes":[12345678901234567890,42],"episode_state_hashes_note":"legacy alias of episode_fingerprints_v1","successes":0}"#,
        "\n",
    );

    #[test]
    fn v21_tree_is_byte_frozen() {
        let root = sandbox("v21-golden");
        let report = export_into_at(ExportFormat::V21, PAYLOAD, &root).expect("导出");
        assert_eq!(report.episodes, 2);
        assert_eq!(report.frames, 3);
        assert_eq!(report.format, "v21");
        let tree = list_tree(&root);
        let want: Vec<(String, usize, u64)> = GOLDEN_FILES
            .iter()
            .map(|(r, n, h)| (r.to_string(), *n, *h))
            .collect();
        assert_eq!(tree, want, "v2.1 文件集/大小/内容指纹与改动前不一致");
        assert_eq!(String::from_utf8(read(&root, "MANIFEST.jsonl")).unwrap(), GOLDEN_MANIFEST);
        assert_eq!(
            String::from_utf8(read(&root, "meta/oriera_receipt.json")).unwrap(),
            GOLDEN_RECEIPT
        );
        // 回执双口径：v1 与 v2 逐集指纹都在（回执随行）。
        assert!(GOLDEN_RECEIPT.contains(r#""episode_fingerprints_v1":[12345678901234567890,42]"#));
        assert!(GOLDEN_RECEIPT.contains(r#""episode_fingerprints_v2":[9876543210987654321,7]"#));
    }

    #[test]
    fn v21_export_is_deterministic() {
        let a = sandbox("v21-det-a");
        let b = sandbox("v21-det-b");
        export_into_at(ExportFormat::V21, PAYLOAD, &a).unwrap();
        export_into_at(ExportFormat::V21, PAYLOAD, &b).unwrap();
        assert_eq!(list_tree(&a), list_tree(&b), "重导出必须逐字节一致");
    }

    #[test]
    fn default_and_explicit_v21_format_agree() {
        // normalize 的缺省语义：None/""/"v21" 全部落默认 v2.1。
        assert_eq!(normalize_format(None).unwrap(), ExportFormat::V21);
        assert_eq!(normalize_format(Some("")).unwrap(), ExportFormat::V21);
        assert_eq!(normalize_format(Some("v21")).unwrap(), ExportFormat::V21);
        assert_eq!(normalize_format(Some("v3")).unwrap(), ExportFormat::V30);
        assert_eq!(normalize_format(Some("v30")).unwrap(), ExportFormat::V30);
        assert_eq!(normalize_format(Some("npz")).unwrap(), ExportFormat::Npz);
        assert!(normalize_format(Some("parquet")).is_err());
        let a = sandbox("fmt-default");
        export_into_at(normalize_format(None).unwrap(), PAYLOAD, &a).unwrap();
        let b = sandbox("fmt-explicit");
        export_into_at(normalize_format(Some("v21")).unwrap(), PAYLOAD, &b).unwrap();
        assert_eq!(list_tree(&a), list_tree(&b));
    }

    // ── v3.0 出口 ────────────────────────────────────────────────

    #[test]
    fn v3_export_lays_the_v30_tree() {
        let root = sandbox("v3-tree");
        let report = export_into_at(ExportFormat::V30, PAYLOAD, &root).expect("导出");
        assert_eq!(report.format, "v3");
        // v3 关键文件在位（vendored write_dir_v3 的 stand-in 布局）。
        for rel in [
            "meta/info.json",
            "meta/stats.json",
            "meta/modality.json",
            "meta/tasks.jsonl",
            "meta/episodes/chunk-000/file-000.jsonl",
            "data/chunk-000/file-000.jsonl",
            "MANIFEST.jsonl",
            "README.md",
            "meta/oriera_receipt.json",
        ] {
            assert!(root.join(rel).is_file(), "v3 树缺 {rel}");
        }
        // 纯状态数据集：无视频轨帧目录，也不落 v2.1 的逐集数据文件。
        assert!(!root.join("videos").exists(), "无相机帧不该有 videos/ 目录");
        assert!(!root.join("data/chunk-000/episode_000000.jsonl").exists());
        let info = String::from_utf8(read(&root, "meta/info.json")).unwrap();
        assert!(info.contains(r#""codebase_version":"v3.0""#), "{info}");
        assert!(info.contains(r#""video_path":null"#), "无 RGB 相机 ⇒ video_path:null");
        assert!(info.contains(r#""total_episodes":2"#));
        // v3 episodes 元数据：两集边界 + 共享 file-000 指针。
        let ep_meta = String::from_utf8(read(&root, "meta/episodes/chunk-000/file-000.jsonl")).unwrap();
        assert!(ep_meta.contains(r#""episode_index":0,"tasks":["pick up the cube and place it at the target"],"length":2,"dataset_from_index":0,"dataset_to_index":2"#));
        assert!(ep_meta.contains(r#""episode_index":1,"tasks":["second task phrase"],"length":1,"dataset_from_index":2,"dataset_to_index":3"#));
        // 行合并进共享 file（v3 语义：文件名不是集边界）。
        let data = String::from_utf8(read(&root, "data/chunk-000/file-000.jsonl")).unwrap();
        assert_eq!(data.lines().count(), 3);
        // MANIFEST 提交点在位，且钉住 v3 关键文件。
        let manifest = String::from_utf8(read(&root, "MANIFEST.jsonl")).unwrap();
        for rel in [
            "meta/info.json",
            "meta/episodes/chunk-000/file-000.jsonl",
            "data/chunk-000/file-000.jsonl",
        ] {
            assert!(manifest.contains(&format!(r#""file":"{rel}""#)), "{manifest}");
        }
        // 回执随行且与 v2.1 逐字节同一份（身份随编码走）。
        assert_eq!(
            read(&root, "meta/oriera_receipt.json"),
            GOLDEN_RECEIPT.as_bytes()
        );
        assert!(String::from_utf8(read(&root, "README.md")).unwrap().contains("LeRobotDataset v3.0"));
    }

    // ── npz 出口 ─────────────────────────────────────────────────

    #[test]
    fn npz_export_lays_numpy_archives_with_receipt() {
        let root = sandbox("npz-tree");
        let report = export_into_at(ExportFormat::Npz, PAYLOAD, &root).expect("导出");
        assert_eq!(report.format, "npz");
        // 文件集：每集一个 .npz + MANIFEST + 回执（meta/）——npz 不落卡片
        // （engine `--export npz` 同款）。
        let tree = list_tree(&root);
        let rels: Vec<&str> = tree.iter().map(|(r, _, _)| r.as_str()).collect();
        assert_eq!(
            rels,
            vec![
                "MANIFEST.jsonl",
                "episode_000000.npz",
                "episode_000001.npz",
                "meta/oriera_receipt.json",
            ]
        );
        // ZIP32 魔数 + 成员名（np.load 的最小结构）。
        for i in 0usize..2 {
            let zip = read(&root, format!("episode_{i:06}.npz").as_str());
            assert!(zip.starts_with(b"PK\x03\x04"), "第 {i} 集不是 ZIP");
            let obs = format!("obs_{i}.npy");
            let act = format!("act_{i}.npy");
            assert!(zip.windows(obs.len()).any(|w| w == obs.as_bytes()));
            assert!(zip.windows(act.len()).any(|w| w == act.as_bytes()));
            assert!(zip.windows(9).any(|w| w == b"meta.json"));
        }
        // 每集 meta.json：oriera-npz/1 + 形状（第 0 集：2 行 21/5 维）。
        let zip0 = read(&root, "episode_000000.npz");
        let fmt_key = r#""format":"oriera-npz/1""#;
        assert!(zip0.windows(fmt_key.len()).any(|w| w == fmt_key.as_bytes()));
        let shapes = r#""obs_shape":[2,21],"act_shape":[2,5]"#;
        assert!(zip0.windows(shapes.len()).any(|w| w == shapes.as_bytes()));
        // 回执随行：与 v2.1 逐字节同一份（身份随编码走）。
        assert_eq!(
            read(&root, "meta/oriera_receipt.json"),
            GOLDEN_RECEIPT.as_bytes()
        );
        // MANIFEST 钉住两个归档。
        let manifest = String::from_utf8(read(&root, "MANIFEST.jsonl")).unwrap();
        assert!(manifest.contains(r#""file":"episode_000000.npz""#));
        assert!(manifest.contains(r#""file":"episode_000001.npz""#));
    }

    // ── 护栏（三格式共用，措辞与既有路径一致） ─────────────────────

    #[test]
    fn guards_reject_bad_payloads() {
        let root = sandbox("guards");
        assert!(export_into_at(ExportFormat::V21, "不是json", &root)
            .unwrap_err()
            .contains("payload 非法"));
        assert!(export_into_at(ExportFormat::V21, r#"{"obsNames":[],"actionNames":[]}"#, &root)
            .unwrap_err()
            .contains("payload 缺 episodes"));
        assert!(
            export_into_at(
                ExportFormat::V21,
                r#"{"episodes":[],"obsNames":[],"actionNames":[]}"#,
                &root
            )
            .unwrap_err()
            .contains("没有可导出的 episode")
        );
        // 缺 obsNames / 缺行字段：可读 Error，不半截落盘。
        assert!(export_into_at(ExportFormat::V30, r#"{"episodes":[{"rows":[]}]}"#, &root)
            .unwrap_err()
            .contains("payload 缺 obsNames"));
        assert!(
            export_into_at(
                ExportFormat::Npz,
                r#"{"obsNames":["x"],"actionNames":["a"],"episodes":[{"rows":[{"action":[0.0]}]}]}"#,
                &root
            )
            .unwrap_err()
            .contains("episode 0 row 0 缺 state")
        );
        // 维度不符：push_step 契约错误冒泡为可读 Error（不 panic）。
        let bad_dim = r#"{"obsNames":["x"],"actionNames":["a"],"episodes":[{"rows":[{"state":[1.0,2.0],"action":[0.0]}]}]}"#;
        assert!(export_into_at(ExportFormat::V30, bad_dim, &root)
            .unwrap_err()
            .contains("维度契约不符"));
    }

    // ── 三格式端到端（opt-in：真实内核 transcript） ────────────────
    //
    // 默认 ignore——需要一份真实采集 transcript（env
    // ORIERA_SIM_E2E_TRANSCRIPT 指向 kernel.js postMessage {type:"episodes"}
    // 同形的 JSON；生成器见引擎仓 embodied_sim_collect 的浏览器等价流程，
    // 或直接在 Studio 录制后从 DevTools 抓消息体）。跑法：
    //   ORIERA_SIM_E2E_TRANSCRIPT=<json> cargo test --lib e2e -- --ignored --nocapture
    //
    // 三种编码各落一份到 %TEMP%/oriera-sim-e2e-<pid>/<fmt>/ 并打印树与
    // 回执——「同一份采集、多种编码、回执随行」的端到端证据。
    #[test]
    #[ignore = "opt-in：需要 ORIERA_SIM_E2E_TRANSCRIPT 指向真实采集 transcript"]
    fn e2e_three_formats_from_real_transcript() {
        let path = std::env::var("ORIERA_SIM_E2E_TRANSCRIPT")
            .expect("设 ORIERA_SIM_E2E_TRANSCRIPT=<transcript.json> 再跑");
        let payload = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读 transcript {path} 失败：{e}"));
        let out_root = std::env::temp_dir().join(format!("oriera-sim-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out_root);
        for fmt in [ExportFormat::V21, ExportFormat::V30, ExportFormat::Npz] {
            let dir = out_root.join(fmt.dir_suffix());
            let report = export_into_at(fmt, &payload, &dir).expect("导出");
            println!(
                "[e2e] {:>14} → {}（{} 集 / {} 行）",
                report.format, report.dir, report.episodes, report.frames
            );
            fn walk(p: &std::path::Path, base: &std::path::Path, out: &mut Vec<String>) {
                for e in std::fs::read_dir(p).unwrap() {
                    let c = e.unwrap().path();
                    if c.is_dir() {
                        walk(&c, base, out);
                    } else {
                        let rel = c.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                        let n = std::fs::metadata(&c).unwrap().len();
                        out.push(format!("    {rel} ({n} B)"));
                    }
                }
            }
            let mut files = Vec::new();
            walk(&dir, &dir, &mut files);
            files.sort();
            for f in &files {
                println!("{}", f);
            }
            let receipt = std::fs::read_to_string(dir.join("meta/oriera_receipt.json")).unwrap();
            println!("    receipt: {}", &receipt[..receipt.len().min(220)]);
            println!();
        }
        // 回执跨格式一致：三份 meta/oriera_receipt.json 逐字节同一。
        let v21 = std::fs::read(out_root.join("lerobot-v21/meta/oriera_receipt.json")).unwrap();
        let v3 = std::fs::read(out_root.join("lerobot-v30/meta/oriera_receipt.json")).unwrap();
        let npz = std::fs::read(out_root.join("npz/meta/oriera_receipt.json")).unwrap();
        assert_eq!(v21, v3);
        assert_eq!(v21, npz);
    }
}
