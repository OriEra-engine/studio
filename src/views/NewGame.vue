<script setup lang="ts">
import { computed, ref, toRefs } from "vue";
import {
  NButton, NCard, NInput, NGi, NGrid, NSteps, NStep, NTag, NSpace,
  NAlert, NCheckbox, useMessage, NSpin,
} from "naive-ui";
import { useRouter } from "vue-router";
import { api, type ProjectMeta, type GeneratedStory } from "../api";
import { GAMEPLAY_PACKS as FALLBACK_PACKS } from "../data/gameplay-packs";
import { STORY_FRAMEWORKS as FALLBACK_FRAMEWORKS } from "../data/story-frameworks";
import { store, refreshStatus } from "../store";
import { wizard } from "../wizard-state";
import ProviderQuickSetup from "../components/ProviderQuickSetup.vue";
import type { AgentPreset, GameplayPack, StoryBeat, StoryFramework } from "../types";

const message = useMessage();
const router = useRouter();

// 向导状态跨路由暂存：跳去模型中心/看 Demo 再回来，选过的东西不丢
const {
  step, gameplayId, frameworkId, appliedFramework, userIdea,
  premise, theme, agentSel, projectName,
} = toRefs(wizard);
const beats: StoryBeat[] = wizard.beats;

// ---- 模板库（Rust 合并内置+用户包；纯浏览器环境回退到静态数据）----
const packs = ref<GameplayPack[]>([...FALLBACK_PACKS]);
const frameworks = ref<StoryFramework[]>([...FALLBACK_FRAMEWORKS]);
const userPackCount = ref(0);
async function loadTemplates() {
  try {
    const lib = await api.listTemplates();
    if (lib.gameplay.length) packs.value = lib.gameplay;
    if (lib.story_frameworks.length) frameworks.value = lib.story_frameworks;
    userPackCount.value = lib.user_pack_files.length;
    lib.broken.forEach(([f, e]) => message.warning(`用户包 ${f} 解析失败：${e}`));
  } catch { /* 纯浏览器/降级：用静态回退 */ }
}
async function importPack() {
  try {
    const r = await api.importGameplayPack();
    if (r) {
      message.success(r);
      await loadTemplates();
    }
  } catch (e) {
    message.error(String(e));
  }
}

// ---- 主题 ----
const THEMES = [
  { id: "space", name: "🌌 深空", desc: "幽蓝星环" },
  { id: "cafe", name: "☕ 咖啡暖光", desc: "暖黄灯下" },
  { id: "dungeon", name: "🕯️ 幽暗地牢", desc: "火把与阴影" },
  { id: "snow", name: "❄️ 雪原", desc: "白昼极光" },
];

// ---- 第一步：玩法 ----
const pack = computed(() => packs.value.find(p => p.id === gameplayId.value));

// ---- 第二步：剧情 ----
const framework = computed(() => frameworks.value.find(f => f.id === frameworkId.value));

// ---- 第三步：模型（状态读全局 store）----
const providerReady = computed(() => !!store.status?.provider_configured);
const providerModel = computed(() => store.status?.provider?.model ?? "");

// ---- 第五步：生成 ----
const creating = ref(false);
const created = ref<ProjectMeta | null>(null);

loadTemplates();

function hueOf(seed: string): number {
  let h = 2166136261;
  for (const c of seed) { h ^= c.codePointAt(0)!; h = Math.imul(h, 16777619); }
  return (h >>> 0) % 360;
}
function packStyle(id: string) {
  const h = hueOf(id);
  return { background: `linear-gradient(160deg, hsl(${h} 45% 20%) 0%, #141418 78%)` };
}

function pickPack(id: string) {
  gameplayId.value = id;
  const p = packs.value.find(x => x.id === id)!;
  premise.value = p.story_premise;
  beats.splice(0, beats.length, ...p.story_beats.map(b => ({ ...b })));
  frameworkId.value = "pack-default";
  p.agents.forEach(a => (agentSel.value[a.name] = true));
  projectName.value = `我的${p.name}`;
}

function pickFramework(id: string) {
  frameworkId.value = id;
  if (id === "pack-default" && pack.value) {
    premise.value = pack.value.story_premise;
    beats.splice(0, beats.length, ...pack.value.story_beats.map(b => ({ ...b })));
    appliedFramework.value = "pack-default";
  }
}

const aiBusy = ref(false);
const aiStreaming = ref(false);
const aiStreamText = ref("");
let aiTaskId = "";

function stopAi() {
  if (aiTaskId) api.cancelAi(aiTaskId).catch(() => {});
}

async function aiGenerate() {
  if (!providerReady.value) {
    // 不再让用户自己找配置入口：直接带去第 3 步（内联快配）
    message.info("AI 定制需要模型——已带你到第 3 步，配好再回来点生成");
    wizard.step = 3;
    return;
  }
  if (!pack.value || aiBusy.value) return;
  aiBusy.value = true;
  aiStreaming.value = true;
  aiStreamText.value = "";
  try {
    // 叙述框架节奏单独传参,替换后端默认的三幕式指令(避免两套节奏打架)
    const rhythm =
      framework.value && frameworkId.value !== "pack-default"
        ? `${framework.value.name}：${framework.value.phases.join("→")}。各阶段节拍提示：${framework.value.beat_hint.join("；")}。请严格按该框架推进,不要套用三幕式。`
        : undefined;
    aiTaskId = await api.generateStoryStream(
      {
        gameplayName: pack.value.name,
        tagline: pack.value.tagline,
        userIdea: userIdea.value.trim(),
        beatCount: 6,
        frameworkRhythm: rhythm,
        economyNames: pack.value.economy.map(r => r.name),
      },
      ev => {
        if (ev.event === "delta") {
          aiStreamText.value += ev.text;
        } else if (ev.event === "done") {
          const story = ev.result as GeneratedStory;
          premise.value = story.premise;
          beats.splice(0, beats.length, ...story.beats);
          appliedFramework.value = frameworkId.value;
          aiBusy.value = false;
          aiStreaming.value = false;
          aiStreamText.value = "";
          message.success("AI 已生成定制剧情，可继续调整或直接下一步");
        } else {
          aiBusy.value = false;
          aiStreaming.value = false;
          if (ev.message !== "已取消") message.error(ev.message);
        }
      },
    );
  } catch (e) {
    aiBusy.value = false;
    aiStreaming.value = false;
    message.error(String(e).length > 160 ? String(e).slice(0, 160) + "…" : String(e));
  }
}

const chosenAgents = computed<AgentPreset[]>(() =>
  pack.value ? pack.value.agents.filter(a => agentSel.value[a.name]) : []
);

async function generate() {
  if (!pack.value) return;
  creating.value = true;
  try {
    const meta = await api.createProject({
      project_name: projectName.value.trim() || `我的${pack.value.name}`,
      gameplay: {
        id: pack.value.id,
        name: pack.value.name,
        tagline: pack.value.tagline,
        core_loop: pack.value.core_loop,
        mechanics: pack.value.mechanics,
        economy: pack.value.economy,
        difficulty: pack.value.difficulty,
      },
      story: {
        framework_name:
          appliedFramework.value === "pack-default"
            ? "三幕式（模板默认）"
            : frameworks.value.find(f => f.id === appliedFramework.value)?.name ?? "三幕式（模板默认）",
        premise: premise.value,
        beats: [...beats],
      },
      agents: chosenAgents.value,
      user_idea: userIdea.value,
      theme: theme.value,
    });
    created.value = meta;
    message.success(`「${meta.name}」已生成！即将进入你的世界……`);
    refreshStatus(); // 仪表盘项目计数即时 +1
    // F1（PLAN-godot-fusion）：Godot F5 体验——向导完成即自动进入
    // 游玩路由（浏览器直开），省一次手动导航；编辑器仍可从项目卡进入。
    await api.openProject(meta.id);
    router.push(`/play/${meta.id}`);
  } catch (e) {
    message.error(`生成失败：${e}`);
  } finally {
    creating.value = false;
  }
}

const canNext = computed(() => {
  switch (step.value) {
    case 1: return !!gameplayId.value;
    case 2: return beats.length > 0;
    case 3: return true;
    case 4: return chosenAgents.value.length > 0;
    default: return false;
  }
});
</script>

<template>
  <div class="page">
    <h2 class="page-title">新建游戏</h2>
    <p class="page-sub">五步生成一个可运行的 AI 原生游戏原型：3D 世界 · AI NPC · 剧情导演系统。</p>

    <NSteps :current="step" size="small" style="margin-bottom: 20px">
      <NStep title="选玩法" />
      <NStep title="定剧情" />
      <NStep title="模型植入" />
      <NStep title="居民人设" />
      <NStep title="生成" />
    </NSteps>

    <!-- 第一步：玩法 -->
    <template v-if="step === 1">
      <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px">
        <span style="color: #6f6f7a; font-size: 12.5px">
          内置 {{ packs.length }} 个玩法包{{ userPackCount ? ` · 已加载你的 ${userPackCount} 个` : '' }}
        </span>
        <NButton size="small" secondary @click="importPack">📦 导入玩法包</NButton>
      </div>
      <NGrid :cols="5" :x-gap="12" :y-gap="12" responsive="screen" item-responsive>
        <NGi v-for="p in packs" :key="p.id" span="5 s:3 m:1">
          <div
            class="pack-card" :class="{ sel: gameplayId === p.id }" :style="packStyle(p.id)"
            role="button" tabindex="0" :aria-pressed="gameplayId === p.id" :aria-label="`选择玩法 ${p.name}`"
            @click="pickPack(p.id)" @keydown.enter.prevent="pickPack(p.id)" @keydown.space.prevent="pickPack(p.id)"
          >
            <div class="pack-emoji">{{ p.emoji }}</div>
            <div class="pack-name">{{ p.name }}</div>
            <div class="pack-tagline">{{ p.tagline }}</div>
            <div class="pack-tags"><NTag v-for="t in p.tags" :key="t" size="tiny" round :bordered="false">{{ t }}</NTag></div>
          </div>
        </NGi>
      </NGrid>
      <NCard v-if="pack" size="small" style="margin-top: 16px">
        <b>{{ pack.emoji }} {{ pack.name }}</b> — {{ pack.desc }}
        <div style="color: #74747f; font-size: 12.5px; margin-top: 8px">核心循环：{{ pack.core_loop.join(' → ') }}</div>
      </NCard>
    </template>

    <!-- 第二步：剧情 -->
    <template v-if="step === 2 && pack">
      <NGrid :cols="3" :x-gap="16" responsive="screen" item-responsive>
        <NGi span="3 m:1">
          <NCard title="叙事框架" size="small">
            <div class="fw-list">
              <div
                class="fw-item sel-like" :class="{ sel: frameworkId === 'pack-default' }"
                role="button" tabindex="0" :aria-pressed="frameworkId === 'pack-default'" aria-label="选择叙事框架：模板默认"
                @click="pickFramework('pack-default')" @keydown.enter.prevent="pickFramework('pack-default')" @keydown.space.prevent="pickFramework('pack-default')"
              >
                <b>📦 模板默认</b><div class="fw-desc">{{ pack.name }} 精选的三幕式剧情（左下方预览）</div>
              </div>
              <div
                v-for="f in frameworks" :key="f.id" class="fw-item" :class="{ sel: frameworkId === f.id }"
                role="button" tabindex="0" :aria-pressed="frameworkId === f.id" :aria-label="`选择叙事框架 ${f.name}`"
                @click="pickFramework(f.id)" @keydown.enter.prevent="pickFramework(f.id)" @keydown.space.prevent="pickFramework(f.id)"
              >
                <b>{{ f.emoji }} {{ f.name }}</b>
                <div class="fw-desc">{{ f.desc }}</div>
                <div class="fw-phases">{{ f.phases.join(" → ") }}</div>
              </div>
            </div>
          </NCard>
        </NGi>
        <NGi span="3 m:2">
          <NCard title="你的想法（可选）+ AI 定制" size="small" style="margin-bottom: 12px">
            <NInput
              v-model:value="userIdea" type="textarea" :rows="2"
              placeholder="例如：把故事放在深海邮轮上，主角是个失忆的钢琴师……（留空则按模板/AI 自由发挥）"
            />
            <div style="margin-top: 10px; display: flex; gap: 10px; align-items: center">
              <NButton type="primary" ghost :loading="aiBusy && !aiStreaming" @click="aiGenerate">
                ✨ {{ aiStreaming ? '生成中…' : providerReady ? 'AI 生成定制剧情' : 'AI 定制（先在第 3 步配置模型）' }}
              </NButton>
              <NButton v-if="aiStreaming" size="small" quaternary type="error" @click="stopAi">■ 停止</NButton>
              <span style="color: #71717c; font-size: 12px">使用模型：{{ providerModel || '未配置' }}</span>
            </div>
            <p v-if="frameworkId !== 'pack-default' && appliedFramework !== frameworkId" style="color: #e8a35c; font-size: 12px; margin: 10px 0 0">
              ⚠️ 已选「{{ framework?.name }}」但尚未应用——下方节拍仍是模板默认（三幕式）。点上方「AI 生成」后生效。
            </p>
          </NCard>
          <NCard :title="`剧情预览（${beats.length} 拍）`" size="small">
            <div v-if="aiStreaming" class="stream-pane">
              <pre class="stream-text">{{ aiStreamText }}▍</pre>
              <p class="stream-hint">AI 正在逐字创作——首次点击约 1 秒内出字。生成完成自动替换预览，可随时「停止」。</p>
            </div>
            <template v-else>
              <p style="color:#9fc3ff; font-size: 13px; margin: 0 0 12px">{{ premise }}</p>
              <div v-for="(b, i) in beats" :key="i" class="beat-line">
                <span class="beat-ph">{{ b.phase }}</span>
                <b>{{ b.title }}</b>
                <span class="beat-ds">{{ b.desc }}</span>
              </div>
            </template>
          </NCard>
        </NGi>
      </NGrid>
    </template>

    <!-- 第三步：模型（内联快配，不再跳页） -->
    <template v-if="step === 3">
      <NAlert :type="providerReady ? 'success' : 'warning'" style="margin-bottom: 14px">
        <template v-if="providerReady">
          当前已植入：<b>{{ providerModel }}</b> —— 生成的游戏里 NPC 对话与导演旁白将由它驱动。
        </template>
        <template v-else>
          在下面直接配置（也可跳过——生成的游戏会以<b>离线性格引擎</b>运行，之后随时可植入）。
        </template>
      </NAlert>
      <NCard size="small" style="max-width: 720px">
        <ProviderQuickSetup />
      </NCard>
      <div style="margin-top: 12px">
        <NButton size="tiny" quaternary @click="router.push('/models')">需要硬件体检 / Ollama 本地模型 / 任务槽位？前往模型中心 ↗</NButton>
      </div>
    </template>

    <!-- 第四步：居民 -->
    <template v-if="step === 4 && pack">
      <NCard title="🌍 场景主题" size="small" style="margin-bottom: 14px">
        <div style="display: flex; gap: 10px; flex-wrap: wrap">
          <div
            v-for="t in THEMES" :key="t.id" class="theme-chip" :class="{ sel: theme === t.id }"
            role="button" tabindex="0" :aria-pressed="theme === t.id" :aria-label="`选择主题 ${t.name}`"
            @click="theme = t.id" @keydown.enter.prevent="theme = t.id" @keydown.space.prevent="theme = t.id"
          >
            <div class="theme-name">{{ t.name }}</div>
            <div class="theme-desc">{{ t.desc }}</div>
          </div>
        </div>
      </NCard>
      <NGrid :cols="3" :x-gap="14" responsive="screen" item-responsive>
        <NGi v-for="a in pack.agents" :key="a.name" span="3 m:1">
          <NCard size="small" :class="{ agentoff: !agentSel[a.name] }">
            <template #header>
              <NCheckbox v-model:checked="agentSel[a.name]" style="margin-right: 8px" />
              <b>{{ a.name }}</b> <NTag size="tiny" round style="margin-left: 6px">{{ a.role }}</NTag>
            </template>
            <p style="color: #a0a0ab; font-size: 12.5px; line-height: 1.65">{{ a.persona }}</p>
            <p style="color: #6f6f7a; font-size: 12px; margin-top: 8px">情绪：{{ a.mood }}</p>
            <p class="first-line">「{{ a.first_line }}」</p>
          </NCard>
        </NGi>
      </NGrid>
    </template>

    <!-- 第五步：生成 -->
    <template v-if="step === 5 && pack">
      <NCard size="small" style="max-width: 640px">
        <NSpace vertical size="large">
          <div>
            <label class="fld-label">游戏名</label>
            <NInput v-model:value="projectName" placeholder="给你的游戏起个名字" />
          </div>
          <div class="summary">
            <div>玩法：<b>{{ pack.emoji }} {{ pack.name }}</b> — {{ pack.tagline }}</div>
            <div>叙事：<b>{{ appliedFramework === 'pack-default' ? '模板默认三幕式' : frameworks.find(f => f.id === appliedFramework)?.name }}</b> · {{ beats.length }} 拍</div>
            <div>居民：<b>{{ chosenAgents.map(a => a.name).join('、') }}</b></div>
            <div>主题：<b>{{ THEMES.find(t => t.id === theme)?.name }}</b></div>
            <div>模型：<b>{{ providerReady ? providerModel : '离线性格引擎' }}</b></div>
          </div>
          <NSpin :show="creating">
            <NButton type="primary" size="large" style="width: 100%" @click="generate">
              🚀 生成游戏原型并运行
            </NButton>
          </NSpin>
          <p style="color: #71717c; font-size: 12px">
            生成后自动在新窗口运行；也可到「我的项目」随时再次打开。原型 = Three.js 3D 场景 + AI NPC + 剧情导演，可在项目目录继续改造。
          </p>
        </NSpace>
      </NCard>
    </template>

    <!-- 导航 -->
    <div style="margin-top: 22px; display: flex; justify-content: space-between">
      <NButton v-if="step > 1" quaternary @click="step--">← 上一步</NButton>
      <span v-else></span>
      <NButton v-if="step < 5" type="primary" :disabled="!canNext" @click="step++">下一步 →</NButton>
    </div>
  </div>
</template>

<style scoped>
.stream-pane { background: #0e0e12; border: 1px solid #202027; border-radius: 10px; padding: 12px 14px; }
.stream-text { font-family: inherit; font-size: 12.5px; color: #a8a8b3; line-height: 1.7; white-space: pre-wrap; word-break: break-word; margin: 0; max-height: 260px; overflow-y: auto; }
.stream-hint { color: #55555f; font-size: 11.5px; margin: 8px 0 0; }
.pack-card:focus-visible, .fw-item:focus-visible, .theme-chip:focus-visible { outline: 2px solid #7eb8ff; outline-offset: 2px; }
.pack-card { background: #141418; border: 1.5px solid #202027; border-radius: 12px; padding: 14px 12px; cursor: pointer; height: 100%; transition: all .12s; }
.pack-card:hover { border-color: #3a3a46; transform: translateY(-2px); }
.pack-card.sel { border-color: #7eb8ff; background: #1a1a20; }
.pack-emoji { font-size: 26px; margin-bottom: 6px; }
.pack-name { font-weight: 700; font-size: 14px; }
.pack-tagline { color: #74747f; font-size: 11.5px; margin: 4px 0 8px; line-height: 1.5; }
.pack-tags { display: flex; gap: 4px; flex-wrap: wrap; }
.fw-item { background: #141418; border: 1.5px solid #202027; border-radius: 10px; padding: 10px 12px; margin-bottom: 8px; cursor: pointer; font-size: 13px; }
.fw-item:hover { border-color: #3a3a46; }
.fw-item.sel { border-color: #7eb8ff; background: #1a1a20; }
.fw-desc { color: #74747f; font-size: 11.5px; line-height: 1.55; margin-top: 4px; }
.fw-phases { color: #5f7ba8; font-size: 10.5px; margin-top: 6px; }
.beat-line { padding: 8px 0; border-bottom: 1px dashed #202027; font-size: 13px; display: flex; gap: 10px; align-items: baseline; }
.beat-line:last-child { border-bottom: none; }
.beat-ph { color: #6f6f7a; font-size: 11px; flex: none; width: 70px; }
.beat-ds { color: #74747f; font-size: 12px; }
.agentoff { opacity: .45; }
.first-line { color: #9fc3ff; font-size: 12.5px; margin-top: 10px; line-height: 1.6; }
.fld-label { display: block; font-size: 12px; color: #74747f; margin-bottom: 6px; }
.summary { font-size: 13px; color: #a0a0ab; line-height: 2.1; background: #141418; border-radius: 10px; padding: 14px 16px; }
.theme-chip { background: #141418; border: 1.5px solid #202027; border-radius: 10px; padding: 10px 18px; cursor: pointer; text-align: center; }
.theme-chip:hover { border-color: #3a3a46; }
.theme-chip.sel { border-color: #7eb8ff; background: #1a1a20; }
.theme-name { font-size: 13.5px; font-weight: 700; }
.theme-desc { font-size: 11px; color: #6f6f7a; margin-top: 3px; }
</style>
