//! Demo 目录与打开逻辑：元数据 + webview 窗口 + provider 注入脚本。

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

#[derive(Debug, Clone, Serialize)]
pub struct DemoInfo {
    pub id: String,
    pub title: String,
    pub desc: String,
    pub tag: String,
    pub featured: bool,
}

/// 内置 demo 的中文元数据（id 与 examples/ 目录名一致）。
/// title 取自各 demo README/index.html，desc 为中文简介。
const DEMOS: &[( &str, &str, &str, &str, bool)] = &[
    ("01-hello-triangle", "01 · Hello Triangle", "WebGPU 渲染管线最小示例：一个三角形的完整渲染闭环，L5 层的「Hello World」。", "渲染 · WebGPU", false),
    ("02-ecs-spawn", "02 · ECS Spawn", "实体组件系统演示：实体的生成与销毁，L3 ECS 层数据驱动架构入门。", "ECS 架构", false),
    ("03-physics", "03 · Physics", "物理沙盒：刚体运动、碰撞与作用力，实时物理模拟体验。", "物理模拟", false),
    ("04-agent-chat", "04 · Agent Chat", "Agent 对话演示：规则引擎驱动的 NPC 意图理解与应答（离线即可运行）。", "AI Agent", false),
    ("05-gltf-model", "05 · glTF Model", "glTF 3D 模型加载：Three.js 渲染带动画的 3D 角色，L4 资产层示例。", "3D 资产", false),
    ("06-agent-physics", "06 · Agent Physics", "Agent × 物理联动：用自然语言指令指挥物理世界（「生成 20 个」「关掉重力」）。", "AI × 物理", false),
    ("07-webgpu-particles", "07 · WebGPU 1M Particles", "WebGPU 百万粒子系统：GPU 计算驱动的粒子流场，考验渲染上限。", "渲染 · WebGPU", false),
    ("08-ecs-visualizer", "08 · ECS Visualizer", "ECS 可视化：实时观察实体-组件-系统的数据流转，理解引擎架构。", "ECS 架构", false),
    ("09-full-arch", "09 · Full Architecture", "全架构串联演示：从 PAL 到渲染的完整分层协作，一次看懂引擎全貌。", "全架构", false),
    ("10-word-spirit", "10 · 言灵世界 Word Spirit", "AI 原生小游戏：文字驱动世界生成与演化，「所说即所见」的世界模型雏形。", "AI 原生玩法", false),
    ("11-ai-cafe", "11 · 隅角咖啡馆", "旗舰示例：3D 世界里的 AI NPC——会记忆、会聊天、会推进剧情。配置模型后 NPC 由大模型驱动，拥有长期记忆与导演系统。", "AI NPC · 旗舰", true),
    ("12-memory-atlas", "12 · 记忆图谱 Memory Atlas", "长期记忆可视化：MemoryStore 的检索式记忆以图谱呈现——写入、衰减、召回一览。", "L9 记忆", false),
    ("13-genai-gallery", "13 · 生成式画廊 GenAI Gallery", "一句话装帧世界：L8 生成管线（文生图/文生3D）+ L4 命名热替换，配 Key 后海报与穹顶实时生成。", "L8 生成", false),
];

pub fn catalog() -> Vec<DemoInfo> {
    DEMOS
        .iter()
        .map(|(id, title, desc, tag, featured)| DemoInfo {
            id: id.to_string(),
            title: title.to_string(),
            desc: desc.to_string(),
            tag: tag.to_string(),
            featured: *featured,
        })
        .collect()
}


#[tauri::command]
pub fn list_demos() -> Vec<DemoInfo> {
    catalog()
}

#[tauri::command]
pub fn open_demo(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    folder: String,
) -> Result<(), String> {
    // 目录名白名单校验，杜绝路径穿越
    let valid = catalog().iter().any(|d| d.id == folder);
    if !valid {
        return Err(format!("unknown demo: {folder}"));
    }

    let url: tauri::Url = format!("http://127.0.0.1:{}/examples/{}/", state.port, folder)
        .parse()
        .map_err(|e| format!("bad url: {e}"))?;

    let title = catalog()
        .iter()
        .find(|d| d.id == folder)
        .map(|d| format!("OriEra · {}", d.title))
        .unwrap_or_else(|| "OriEra Demo".to_string());

    let label = format!("demo-{folder}");
    // 已存在同名窗口则先关闭，保证注入脚本重新生效
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.close();
    }

    let mut builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(url.clone()))
        .title(&title)
        .inner_size(1280.0, 860.0)
        .min_inner_size(960.0, 640.0);
    // 同 open_project：不挂 initialization_script，避免新版 WebView2 吞初始导航
    let win = builder.build().map_err(|e| e.to_string())?;
    // 同 open_project：新版 WebView2 下显式补导航，防 about:blank 空窗
    let _ = win.navigate(url.clone());
    if crate::settings::load(&app).developer_mode {
        win.open_devtools();
    }
    Ok(())
}
