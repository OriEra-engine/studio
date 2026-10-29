//! 模型推荐矩阵：按硬件体检结果给出云端 / 本地（Ollama）双轨建议。
//! 推荐结果可直接转成引擎 provider 配置注入。

use crate::hardware::HardwareInfo;
use crate::settings::ProviderCfg;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CloudRec {
    pub label: String,
    pub kind: String,
    pub model: String,
    pub base: String,
    pub note: String,
    pub free: bool,
    pub signup_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalRec {
    pub ollama_model: String,
    pub size_note: String,
    pub disk_gb: f32,
    pub vram_need_gb: u32,
    /// 可直接注入引擎的 provider 配置（Ollama 的 OpenAI 兼容端点）
    pub provider: ProviderCfg,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub tier: String, // "cloud" | "both"
    pub headline: String,
    pub detail: String,
    pub cloud: Vec<CloudRec>,
    pub local: Option<LocalRec>,
}

#[allow(dead_code)] // 2026-10-28 用户要求暂时下线 XLite 入口；恢复时把 xlite() 加回下方 cloud 列表
fn xlite() -> CloudRec {
    CloudRec {
        label: "XLite 潜元网关 · 全家桶".into(),
        kind: "xlite".into(),
        model: "gpt-4o-mini".into(),
        base: "https://xlite.dezai.com/v1".into(),
        note: "潜元无限统一网关 · 一个 Key 调用 160+ 模型，OpenAI 兼容".into(),
        free: false,
        signup_url: "https://xlite.dezai.com".into(),
    }
}

fn zhipu() -> CloudRec {
    CloudRec {
        label: "智谱 GLM · glm-4-flash".into(),
        kind: "zhipu".into(),
        model: "glm-4-flash".into(),
        base: "https://open.bigmodel.cn/api/paas/v4".into(),
        note: "国内直连，免充值有免费额度，注册即用——低配置首选".into(),
        free: true,
        signup_url: "https://open.bigmodel.cn".into(),
    }
}

fn deepseek() -> CloudRec {
    CloudRec {
        label: "DeepSeek · deepseek-chat".into(),
        kind: "deepseek".into(),
        model: "deepseek-chat".into(),
        base: "https://api.deepseek.com/v1".into(),
        note: "国内直连，价格低、中文写作强".into(),
        free: false,
        signup_url: "https://platform.deepseek.com".into(),
    }
}

fn moonshot() -> CloudRec {
    CloudRec {
        label: "Moonshot Kimi".into(),
        kind: "moonshot".into(),
        model: "kimi-k2.6".into(),
        base: "https://api.moonshot.cn/v1".into(),
        note: "国内直连，256K 长上下文见长".into(),
        free: false,
        signup_url: "https://platform.kimi.com".into(),
    }
}

fn doubao() -> CloudRec {
    CloudRec {
        label: "豆包 · 火山方舟".into(),
        kind: "doubao".into(),
        model: "doubao-seed-1.6".into(),
        base: "https://ark.cn-beijing.volces.com/api/v3".into(),
        note: "字节跳动 · OpenAI 兼容，模型填 ID 或 ep-接入点".into(),
        free: false,
        signup_url: "https://www.volcengine.com/product/doubao".into(),
    }
}

fn ollama_rec(model: &str, size_note: &str, disk_gb: f32, vram_need_gb: u32) -> LocalRec {
    LocalRec {
        ollama_model: model.into(),
        size_note: size_note.into(),
        disk_gb,
        vram_need_gb,
        provider: ProviderCfg {
            kind: "openai".into(),
            key: "ollama".into(),
            model: model.into(),
            base: "http://127.0.0.1:11434/v1".into(),
        },
    }
}

/// 核心推荐矩阵（v1 规则）。
pub fn recommend(hw: &HardwareInfo) -> Recommendation {
    let has_dgpu = hw.gpus.iter().any(|g| {
        !g.name.to_lowercase().contains("integrated")
            && !g.name.to_lowercase().contains("amd radeon graphics")
            && !g.name.to_lowercase().contains("iris")
    });
    let vram = hw.max_vram_mb.unwrap_or(0) as f32 / 1024.0;

    // ---------- 低配：云端为主 ----------
    if !has_dgpu || vram < 4.0 || hw.ram_gb < 8.0 {
        return Recommendation {
            tier: "cloud".into(),
            headline: "你的配置更适合云端模型".into(),
            detail: format!(
                "检测到 {} / 内存 {:.0}GB{}。推荐使用云端 API（无需科学上网，国内直连），AI 由云端运行，本机零负担。智谱 glm-4-flash 有免费额度，注册即用。",
                hw.gpus.first().map(|g| g.name.as_str()).unwrap_or("集成显卡"),
                hw.ram_gb,
                if has_dgpu { format!(" / 独显显存约 {:.0}GB", vram) } else { String::new() }
            ),
            cloud: vec![zhipu(), deepseek(), doubao()],
            local: None,
        };
    }

    // ---------- 有独显：云端 + 本地双轨 ----------
    let (local, headline) = if vram < 8.0 {
        (
            Some(ollama_rec(
                "qwen3:4b",
                "4B 参数 · Q4 量化，入门可跑",
                2.6,
                4,
            )),
            format!("显存约 {:.0}GB：可跑 4B 本地小模型，云端仍是主力", vram),
        )
    } else if vram < 13.0 {
        (
            Some(ollama_rec(
                "qwen3:8b",
                "8B 参数 · Q4 量化，对话质量与速度的甜点档",
                5.2,
                8,
            )),
            format!("显存约 {:.0}GB：8B 本地模型体验良好，推荐本地为主", vram),
        )
    } else if vram < 20.0 {
        (
            Some(ollama_rec(
                "qwen3:14b",
                "14B 参数 · Q4 量化，NPC 表现更立体",
                9.3,
                13,
            )),
            format!("显存约 {:.0}GB：可上 14B，本地即有云端级表现", vram),
        )
    } else {
        (
            Some(ollama_rec(
                "qwen3:32b",
                "32B 参数 · Q4 量化，旗舰级本地推理",
                20.0,
                20,
            )),
            format!("显存约 {:.0}GB：旗舰配置，32B 本地大模型无压力", vram),
        )
    };

    Recommendation {
        tier: "both".into(),
        headline,
        detail: format!(
            "本地模型零 token 费用、数据不出本机，适合长时间挂机游玩；云端免安装、覆盖所有机型。两者都注入引擎，随时在「模型中心」切换。"
        ),
        cloud: vec![zhipu(), deepseek(), doubao(), moonshot()],
        local,
    }
}
