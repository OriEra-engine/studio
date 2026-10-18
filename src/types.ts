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

export interface StoryBeat {
  phase: string;
  title: string;
  desc: string;
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
