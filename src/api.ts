import { invoke, Channel } from "@tauri-apps/api/core";
import type { GameSpec, StoryBeat } from "./types";

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
  beats: StoryBeat[];
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
  story: { framework?: string; premise: string; beats: StoryBeat[] };
  agents: unknown[];
}

export interface CopilotTurn {
  /** "user" | "assistant" */
  role: string;
  content: string;
}

/** AI 流式任务事件（Rust AiStreamEvent 一一对应） */
export type AiStreamEvent =
  | { event: "delta"; text: string }
  | { event: "done"; result: unknown }
  | { event: "error"; message: string };

/** 发起流式任务：返回 task id（可传给 cancelAi），增量经 onEvent 推送。 */
function streamTask(cmd: string, args: Record<string, unknown>, onEvent: (e: AiStreamEvent) => void): Promise<string> {
  const channel = new Channel<AiStreamEvent>();
  channel.onmessage = onEvent;
  return invoke<string>(cmd, { ...args, channel });
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
  generateStory: (gameplayName: string, tagline: string, userIdea: string, beatCount: number, frameworkRhythm?: string, economyNames?: string[]) =>
    invoke<GeneratedStory>("generate_story", {
      gameplayName,
      tagline,
      userIdea,
      beatCount,
      frameworkRhythm: frameworkRhythm ?? null,
      economy: (economyNames ?? []).map(n => ({ name: n })),
    }),
  /** 流式剧情生成：点击 1s 内开始出字，可取消 */
  generateStoryStream: (
    args: { gameplayName: string; tagline: string; userIdea: string; beatCount: number; frameworkRhythm?: string; economyNames?: string[] },
    onEvent: (e: AiStreamEvent) => void,
  ) =>
    streamTask("generate_story_stream", {
      gameplayName: args.gameplayName,
      tagline: args.tagline,
      userIdea: args.userIdea,
      beatCount: args.beatCount,
      frameworkRhythm: args.frameworkRhythm ?? null,
      economy: (args.economyNames ?? []).map(n => ({ name: n })),
    }, onEvent),
  cancelAi: (id: string) => invoke<void>("cancel_ai", { id }),
  createProject: (spec: GameSpec) => invoke<ProjectMeta>("create_project", { spec }),

  // 助手「奥粒」
  copilotChat: (system: string, history: CopilotTurn[], user: string) =>
    invoke<string>("copilot_chat", { system, history, user }),
  copilotChatStream: (system: string, history: CopilotTurn[], user: string, onEvent: (e: AiStreamEvent) => void) =>
    streamTask("copilot_chat_stream", { system, history, user }, onEvent),

  // 我的项目
  listProjects: () => invoke<ProjectMeta[]>("list_projects"),
  openProject: (id: string) => invoke<void>("open_project", { id }),
  deleteProject: (id: string) => invoke<void>("delete_project", { id }),
  revealProjectDir: (id: string) => invoke<void>("reveal_project_dir", { id }),
  /** 正在运行的项目 id（运行中徽标） */
  runningProjects: () => invoke<string[]>("running_projects"),

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
  /** 单拍重写预览：返回新旧两版，不落盘（diff 确认制） */
  previewRegenerateBeat: (id: string, beatIndex: number, instruction: string) =>
    invoke<{ old: import("./types").StoryBeat; new: import("./types").StoryBeat }>("preview_regenerate_beat", { id, beatIndex, instruction }),
  /** 采纳重写结果：落盘 + 备份 + 运行窗口热重载 */
  applyBeat: (id: string, beatIndex: number, beat: import("./types").StoryBeat) =>
    invoke<void>("apply_beat", { id, beatIndex, beat }),
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

/** 云端 provider 预设 —— 与引擎 PROVIDER_PRESETS 对齐(国内可直连的排前面,免费档优先) */
export interface CloudPreset {
  kind: string;
  label: string;
  base: string;
  model: string;
  note: string;
  /** 该厂商的可选模型(点击切换);缺省则自由填写(如豆包的 ep-接入点) */
  models?: string[];
  free?: boolean;
  /** 国内直连(无需海外网络) */
  cn?: boolean;
  /** 提供免费视觉模型 */
  vision?: boolean;
  /** 厂商集团标识 */
  badge?: string;
  url?: string;
}

export const CLOUD_PRESETS: CloudPreset[] = [
  { kind: "zhipu", label: "智谱 GLM", base: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4-flash",
    models: ["glm-4-flash", "glm-4.7-flash", "glm-4.6v-flash", "glm-5.3"],
    note: "国内直连 · Flash 档免费;4.6V 免费视觉", free: true, cn: true, vision: true, url: "https://open.bigmodel.cn" },
  { kind: "deepseek", label: "DeepSeek", base: "https://api.deepseek.com/v1", model: "deepseek-chat",
    models: ["deepseek-chat", "deepseek-reasoner"],
    note: "国内直连 · V4 性价比;错峰时段更省", cn: true, url: "https://platform.deepseek.com" },
  { kind: "doubao", label: "豆包 · 火山方舟", base: "https://ark.cn-beijing.volces.com/api/v3", model: "doubao-seed-1.6",
    note: "OpenAI 兼容;模型填模型 ID 或 ep-接入点", cn: true, badge: "字节跳动", url: "https://www.volcengine.com/product/doubao" },
  { kind: "moonshot", label: "Moonshot Kimi", base: "https://api.moonshot.cn/v1", model: "kimi-k2.6",
    models: ["kimi-k2.6"],
    note: "国内直连 · 256K 长上下文", cn: true, url: "https://platform.kimi.com" },
  { kind: "qwen", label: "Qwen 通义", base: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus",
    models: ["qwen-turbo", "qwen-plus", "qwen-max"],
    note: "阿里 · 国内直连,OpenAI 兼容模式", cn: true, url: "https://dashscope.console.aliyun.com" },
  { kind: "stepfun", label: "阶跃 Step", base: "https://api.stepfun.com/v1", model: "step-3.7-flash",
    note: "国内直连 · 与引擎 L7 预设表同步", cn: true, url: "https://platform.stepfun.com" },
  { kind: "openai", label: "OpenAI", base: "https://api.openai.com/v1", model: "gpt-4o-mini", note: "需海外网络", url: "https://platform.openai.com" },
  { kind: "anthropic", label: "Anthropic Claude", base: "https://api.anthropic.com/v1", model: "claude-3-5-haiku-latest", note: "需海外网络", url: "https://console.anthropic.com" },
  { kind: "openrouter", label: "OpenRouter", base: "https://openrouter.ai/api/v1", model: "openrouter/auto", note: "聚合多家模型", url: "https://openrouter.ai" },
  { kind: "groq", label: "Groq", base: "https://api.groq.com/openai/v1", model: "llama-3.1-8b-instant", note: "免费档速度快 · 需海外网络", free: true, url: "https://console.groq.com" },
];
