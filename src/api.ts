import { invoke } from "@tauri-apps/api/core";
import type { GameSpec } from "./types";

export interface FeedbackResult {
  /** direct = 已直提 issue；manual = 需浏览器/手动投递 */
  mode: string;
  url: string;
  file: string;
}

/** L7 provider 配置 —— 与引擎 kernel.js 的 cfg 结构一一对应 */
export interface ProviderCfg {
  kind: string;
  key: string;
  model: string;
  base: string;
}

export interface DemoInfo {
  id: string;
  title: string;
  desc: string;
  tag: string;
  featured: boolean;
}

export interface ServerInfo {
  port: number;
  snapshot_version: string;
  snapshot_dir: string;
  projects_dir: string;
}

export interface AppStatus {
  server: ServerInfo;
  provider_configured: boolean;
  provider: ProviderCfg | null;
  project_count: number;
}

export interface GpuInfo {
  name: string;
  vram_mb: number | null;
  vram_source: string;
}

export interface HardwareInfo {
  cpu: string;
  cores: number;
  ram_gb: number;
  gpus: GpuInfo[];
  max_vram_mb: number | null;
  os: string;
}

export interface CloudRec {
  label: string;
  kind: string;
  model: string;
  base: string;
  note: string;
  free: boolean;
  signup_url: string;
}

export interface LocalRec {
  ollama_model: string;
  size_note: string;
  disk_gb: number;
  vram_need_gb: number;
  provider: ProviderCfg;
}

export interface Recommendation {
  tier: "cloud" | "both";
  headline: string;
  detail: string;
  cloud: CloudRec[];
  local: LocalRec | null;
}

export interface ProjectMeta {
  id: string;
  name: string;
  tagline: string;
  created: string;
  dir: string;
  url: string;
}

export interface GeneratedStory {
  premise: string;
  beats: { phase: string; title: string; desc: string }[];
}

export interface OllamaModel {
  name: string;
  size_gb: number;
}

export interface OllamaStatus {
  installed: boolean;
  install_path: string | null;
  version: string | null;
  running: boolean;
  models: OllamaModel[];
}

export interface TemplateLib {
  gameplay: import("./types").GameplayPack[];
  story_frameworks: import("./types").StoryFramework[];
  user_pack_files: string[];
  broken: [string, string][];
}

export interface ProjectDetail {
  game: Record<string, unknown>;
  story: { framework?: string; premise: string; beats: { phase: string; title: string; desc: string }[] };
  agents: unknown[];
}

export const api = {
  getStatus: () => invoke<AppStatus>("get_status"),
  listDemos: () => invoke<DemoInfo[]>("list_demos"),
  openDemo: (folder: string) => invoke<void>("open_demo", { folder }),

  // 模型中心
  detectHardware: () => invoke<HardwareInfo>("detect_hardware"),
  getRecommendations: (hw: HardwareInfo) => invoke<Recommendation>("get_recommendations", { hw }),
  saveProvider: (cfg: ProviderCfg) => invoke<void>("save_provider", { cfg }),
  clearProvider: () => invoke<void>("clear_provider"),
  testProvider: () => invoke<string>("test_provider"),

  // 新建游戏
  generateStory: (gameplayName: string, tagline: string, userIdea: string, beatCount: number) =>
    invoke<GeneratedStory>("generate_story", {
      gameplayName,
      tagline,
      userIdea,
      beatCount,
    }),
  createProject: (spec: GameSpec) => invoke<ProjectMeta>("create_project", { spec }),

  // 我的项目
  listProjects: () => invoke<ProjectMeta[]>("list_projects"),
  openProject: (id: string) => invoke<void>("open_project", { id }),
  deleteProject: (id: string) => invoke<void>("delete_project", { id }),
  revealProjectDir: (id: string) => invoke<void>("reveal_project_dir", { id }),

  // Ollama
  ollamaStatus: () => invoke<OllamaStatus>("ollama_status"),
  ollamaServe: () => invoke<void>("ollama_serve"),
  ollamaPull: (model: string) => invoke<void>("ollama_pull", { model }),
  ollamaDeleteModel: (model: string) => invoke<void>("ollama_delete_model", { model }),
  gpuVramUsage: () => invoke<[number, number] | null>("gpu_vram_usage"),

  // 模板库（v0.2 外置）
  listTemplates: () => invoke<TemplateLib>("list_templates"),
  importGameplayPack: () => invoke<string>("import_gameplay_pack"),

  // 编辑器与项目维护（v0.2）
  readProject: (id: string) => invoke<ProjectDetail>("read_project", { id }),
  updateStory: (id: string, story: ProjectDetail["story"]) => invoke<void>("update_story", { id, story }),
  regenerateBeat: (id: string, beatIndex: number, instruction: string) =>
    invoke<{ phase: string; title: string; desc: string }>("regenerate_beat", { id, beatIndex, instruction }),
  upgradeRuntime: (id: string) => invoke<string>("upgrade_runtime", { id }),
  exportProjectZip: (id: string) => invoke<string>("export_project_zip", { id }),

  // 设置
  setDeveloperMode: (enabled: boolean) => invoke<void>("set_developer_mode", { enabled }),

  // 反馈链路（v0.3：桌面端 → OriEra 世界引擎仓库 issues）
  submitFeedback: (kind: string, title: string, body: string) =>
    invoke<FeedbackResult>("submit_feedback", { kind, title, body }),
  hasFeedbackPat: () => invoke<boolean>("has_feedback_pat"),
  saveFeedbackPat: (pat: string) => invoke<void>("save_feedback_pat", { pat }),
  clearFeedbackPat: () => invoke<void>("clear_feedback_pat"),
};

/** 云端 provider 预设 —— 与引擎 PROVIDER_PRESETS 对齐（国内可直连的排前面） */
export const CLOUD_PRESETS: { kind: string; label: string; base: string; model: string; note: string; free?: boolean; url?: string }[] = [
  { kind: "zhipu", label: "智谱 GLM", base: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4-flash", note: "国内直连 · glm-4-flash 有免费额度，注册即用", free: true, url: "https://open.bigmodel.cn" },
  { kind: "deepseek", label: "DeepSeek", base: "https://api.deepseek.com/v1", model: "deepseek-chat", note: "国内直连 · 便宜好用", url: "https://platform.deepseek.com" },
  { kind: "moonshot", label: "Moonshot Kimi", base: "https://api.moonshot.cn/v1", model: "moonshot-v1-8k", note: "国内直连 · 长文本", url: "https://platform.moonshot.cn" },
  { kind: "openai", label: "OpenAI", base: "https://api.openai.com/v1", model: "gpt-4o-mini", note: "需海外网络" },
  { kind: "anthropic", label: "Anthropic Claude", base: "https://api.anthropic.com/v1", model: "claude-3-5-haiku-latest", note: "需海外网络" },
  { kind: "openrouter", label: "OpenRouter", base: "https://openrouter.ai/api/v1", model: "openrouter/auto", note: "聚合多家模型" },
  { kind: "groq", label: "Groq", base: "https://api.groq.com/openai/v1", model: "llama-3.1-8b-instant", note: "免费档速度快" },
];
