// vite define 注入的构建期常量（见 vite.config.ts）
declare const __APP_VERSION__: string;

/** 玩法/剧情模板与生成 Spec 的共享类型 */

export interface Mechanic {
  name: string;
  detail: string;
}

export interface EconomyResource {
  name: string;
  source: string;
  sink: string;
}

/** 节拍分支选项(v0.3 叙事分支):next 省略/-1/null=顺延;effect 形如「金币+15」「金币-10」 */
export interface BeatChoice {
  label: string;
  next?: number | null;
  effect?: string | null;
}

export interface StoryBeat {
  phase: string;
  title: string;
  desc: string;
  choices?: BeatChoice[] | null;
  /** #120 影游可执行腿：可选扩展字段。runtime（runtime/index.html）按字段存在性守卫，
   *  缺省时行为与旧版完全一致（其余 16 包零回归）。 */
  /** choice 倒计时秒数；仅对渲染了 choices 的拍生效，超时自动选第一个可用选项 */
  countdown?: number | null;
  /** video 舞台：/projects/{id}/ 相对路径（如 clips/x.mp4）或 /assets/ 绝对路径 */
  video?: string | null;
  /** 结局节点：不再显示推进按钮，按 settle 规则全屏结算 */
  ending?: boolean | null;
  /** 结算规则：分支名 → "*" 通配 或 "变量>=数值"；按声明顺序取第一个命中分支 */
  settle?: Record<string, string> | null;
  /** 结局文案（可选）：分支名 → 结局标题 / 一句评语 */
  ending_titles?: Record<string, string> | null;
  ending_lines?: Record<string, string> | null;
}

export interface AgentPreset {
  name: string;
  role: string;
  persona: string;
  mood: string;
  first_line: string;
}

export interface GameplayPack {
  id: string;
  name: string;
  emoji: string;
  tagline: string;
  tags: string[];
  desc: string;
  core_loop: string[];
  mechanics: Mechanic[];
  economy: EconomyResource[];
  difficulty: string;
  story_premise: string;
  story_beats: StoryBeat[];
  agents: AgentPreset[];
}

export interface StoryFramework {
  id: string;
  name: string;
  emoji: string;
  desc: string;
  phases: string[];
  beat_hint: string[];
}

/** 交给 Rust 项目生成器的完整规格 */
export interface GameSpec {
  project_name: string;
  gameplay: {
    id: string;
    name: string;
    tagline: string;
    core_loop: string[];
    mechanics: Mechanic[];
    economy: EconomyResource[];
    difficulty: string;
  };
  story: {
    framework_name: string;
    premise: string;
    beats: StoryBeat[];
  };
  agents: AgentPreset[];
  user_idea: string;
}
