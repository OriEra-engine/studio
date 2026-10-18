<script setup lang="ts">
import { computed, reactive, ref } from "vue";
import {
  NButton, NCard, NInput, NGi, NGrid, NSteps, NStep, NTag, NSpace,
  NAlert, NCheckbox, useMessage, NSpin,
} from "naive-ui";
import { useRouter } from "vue-router";
import { api, type ProjectMeta, type GeneratedStory } from "../api";
import { GAMEPLAY_PACKS as FALLBACK_PACKS } from "../data/gameplay-packs";
import { STORY_FRAMEWORKS as FALLBACK_FRAMEWORKS } from "../data/story-frameworks";
import type { AgentPreset, GameplayPack, StoryBeat, StoryFramework } from "../types";

const message = useMessage();
const router = useRouter();
const step = ref(1);

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
const theme = ref("space");

// ---- 第一步：玩法 ----
const gameplayId = ref("");
const pack = computed(() => packs.value.find(p => p.id === gameplayId.value));

// ---- 第二步：剧情 ----
const frameworkId = ref("pack-default");
/** 实际应用到当前 beats 的框架（选了框架但没点 AI 生成时不改标签，避免失实） */
const appliedFramework = ref("pack-default");
const userIdea = ref("");
const premise = ref("");
const beats = reactive<StoryBeat[]>([]);
const framework = computed(() => frameworks.value.find(f => f.id === frameworkId.value));

// ---- 第三步：模型 ----
const providerReady = ref(false);
const providerModel = ref("");

// ---- 第四步：Agent ----
const agentSel = reactive<Record<string, boolean>>({});

// ---- 第五步：生成 ----
const projectName = ref("");
const creating = ref(false);
const created = ref<ProjectMeta | null>(null);

async function boot() {
  try {
    const status = await api.getStatus();
    providerReady.value = status.provider_configured;
    providerModel.value = status.provider?.model ?? "";
  } catch { /* 纯浏览器环境 */ }
  await loadTemplates();
}
boot();

function pickPack(id: string) {
  gameplayId.value = id;
  const p = packs.value.find(x => x.id === id)!;
  premise.value = p.story_premise;
  beats.splice(0, beats.length, ...p.story_beats.map(b => ({ ...b })));
  frameworkId.value = "pack-default";
  p.agents.forEach(a => (agentSel[a.name] = true));
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
async function aiGenerate() {
  if (!providerReady.value) {
    message.warning("AI 定制需要先在「模型中心」配置模型");
    return;
  }
  if (!pack.value) return;
  aiBusy.value = true;
  try {
    const fwHint = framework.value && frameworkId.value !== "pack-default"
      ? `（叙事框架：${framework.value.name}，阶段：${framework.value.phases.join("→")}）`
      : "";
    const story: GeneratedStory = await api.generateStory(
      pack.value.name,
      pack.value.tagline,
      `${userIdea.value} ${fwHint}`.trim(),
      6,
    );
    premise.value = story.premise;
    beats.splice(0, beats.length, ...story.beats);
    appliedFramework.value = frameworkId.value;
    message.success("AI 已生成定制剧情，可继续调整或直接下一步");
  } catch (e) {
    message.error(String(e).length > 160 ? String(e).slice(0, 160) + "…" : String(e));
  } finally {
    aiBusy.value = false;
  }
}

const chosenAgents = computed<AgentPreset[]>(() =>
  pack.value ? pack.value.agents.filter(a => agentSel[a.name]) : []
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
    message.success(`「${meta.name}」已生成！`);
    await api.openProject(meta.id);
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
    case 4: return chosenAgents.length > 0;
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
        <span style="color: #7c86a3; font-size: 12.5px">
          内置 {{ packs.length }} 个玩法包{{ userPackCount ? ` · 已加载你的 ${userPackCount} 个` : '' }}
        </span>
        <NButton size="small" secondary @click="importPack">📦 导入玩法包</NButton>
      </div>
      <NGrid :cols="5" :x-gap="12" :y-gap="12" responsive="screen" item-responsive>
        <NGi v-for="p in packs" :key="p.id" span="5 s:3 m:1">
          <div class="pack-card" :class="{ sel: gameplayId === p.id }" @click="pickPack(p.id)">
            <div class="pack-emoji">{{ p.emoji }}</div>
            <div class="pack-name">{{ p.name }}</div>
            <div class="pack-tagline">{{ p.tagline }}</div>
            <div class="pack-tags"><NTag v-for="t in p.tags" :key="t" size="tiny" round :bordered="false">{{ t }}</NTag></div>
          </div>
        </NGi>
      </NGrid>
      <NCard v-if="pack" size="small" style="margin-top: 16px">
        <b>{{ pack.emoji }} {{ pack.name }}</b> — {{ pack.desc }}
        <div style="color: #8a93ad; font-size: 12.5px; margin-top: 8px">核心循环：{{ pack.core_loop.join(' → ') }}</div>
      </NCard>
    </template>

    <!-- 第二步：剧情 -->
    <template v-if="step === 2 && pack">
      <NGrid :cols="3" :x-gap="16" responsive="screen" item-responsive>
        <NGi span="3 m:1">
          <NCard title="叙事框架" size="small">
            <div class="fw-list">
              <div class="fw-item sel-like" :class="{ sel: frameworkId === 'pack-default' }" @click="pickFramework('pack-default')">
                <b>📦 模板默认</b><div class="fw-desc">{{ pack.name }} 精选的三幕式剧情（左下方预览）</div>
              </div>
              <div v-for="f in frameworks" :key="f.id" class="fw-item" :class="{ sel: frameworkId === f.id }" @click="pickFramework(f.id)">
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
              <NButton type="primary" ghost :loading="aiBusy" :disabled="!providerReady" @click="aiGenerate">
                ✨ {{ providerReady ? 'AI 生成定制剧情' : 'AI 定制（需先配置模型）' }}
              </NButton>
              <span style="color: #5a627a; font-size: 12px">使用模型：{{ providerModel || '未配置' }}</span>
            </div>
            <p v-if="frameworkId !== 'pack-default' && appliedFramework !== frameworkId" style="color: #e8a35c; font-size: 12px; margin: 10px 0 0">
              ⚠️ 已选「{{ framework?.name }}」但尚未应用——下方节拍仍是模板默认（三幕式）。点上方「AI 生成」后生效。
            </p>
          </NCard>
          <NCard :title="`剧情预览（${beats.length} 拍）`" size="small">
            <p style="color:#9fc3ff; font-size: 13px; margin: 0 0 12px">{{ premise }}</p>
            <div v-for="(b, i) in beats" :key="i" class="beat-line">
              <span class="beat-ph">{{ b.phase }}</span>
              <b>{{ b.title }}</b>
              <span class="beat-ds">{{ b.desc }}</span>
            </div>
          </NCard>
        </NGi>
      </NGrid>
    </template>

    <!-- 第三步：模型 -->
    <template v-if="step === 3">
      <NCard size="small">
        <template #header>🔌 模型植入</template>
        <NAlert :type="providerReady ? 'success' : 'warning'" style="margin-bottom: 14px">
          <template v-if="providerReady">
            当前已植入：<b>{{ providerModel }}</b> —— 生成的游戏里 NPC 对话与导演旁白将由它驱动。
          </template>
          <template v-else>
            尚未配置模型。生成的游戏会以<b>离线性格引擎</b>运行（可玩但 NPC 为固定话术）。
          </template>
        </NAlert>
        <NSpace>
          <NButton type="primary" ghost @click="router.push('/models')">前往模型中心配置</NButton>
          <NButton secondary @click="boot()">刷新状态</NButton>
          <NButton v-if="providerReady" quaternary @click="step = 4">已就绪，下一步</NButton>
        </NSpace>
      </NCard>
    </template>

    <!-- 第四步：居民 -->
    <template v-if="step === 4 && pack">
      <NCard title="🌍 场景主题" size="small" style="margin-bottom: 14px">
        <div style="display: flex; gap: 10px; flex-wrap: wrap">
          <div v-for="t in THEMES" :key="t.id" class="theme-chip" :class="{ sel: theme === t.id }" @click="theme = t.id">
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
            <p style="color: #9aa2b8; font-size: 12.5px; line-height: 1.65">{{ a.persona }}</p>
            <p style="color: #7c86a3; font-size: 12px; margin-top: 8px">情绪：{{ a.mood }}</p>
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
          <p style="color: #5a627a; font-size: 12px">
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
.pack-card { background: #161a28; border: 1.5px solid #232a3d; border-radius: 12px; padding: 14px 12px; cursor: pointer; height: 100%; transition: all .12s; }
.pack-card:hover { border-color: #3a5e9e; transform: translateY(-2px); }
.pack-card.sel { border-color: #7eb8ff; background: #17223a; }
.pack-emoji { font-size: 26px; margin-bottom: 6px; }
.pack-name { font-weight: 700; font-size: 14px; }
.pack-tagline { color: #8a93ad; font-size: 11.5px; margin: 4px 0 8px; line-height: 1.5; }
.pack-tags { display: flex; gap: 4px; flex-wrap: wrap; }
.fw-item { background: #161a28; border: 1.5px solid #232a3d; border-radius: 10px; padding: 10px 12px; margin-bottom: 8px; cursor: pointer; font-size: 13px; }
.fw-item:hover { border-color: #3a5e9e; }
.fw-item.sel { border-color: #7eb8ff; background: #17223a; }
.fw-desc { color: #8a93ad; font-size: 11.5px; line-height: 1.55; margin-top: 4px; }
.fw-phases { color: #5f7ba8; font-size: 10.5px; margin-top: 6px; }
.beat-line { padding: 8px 0; border-bottom: 1px dashed #232a3d; font-size: 13px; display: flex; gap: 10px; align-items: baseline; }
.beat-line:last-child { border-bottom: none; }
.beat-ph { color: #7c86a3; font-size: 11px; flex: none; width: 70px; }
.beat-ds { color: #8a93ad; font-size: 12px; }
.agentoff { opacity: .45; }
.first-line { color: #9fc3ff; font-size: 12.5px; margin-top: 10px; line-height: 1.6; }
.fld-label { display: block; font-size: 12px; color: #8a93ad; margin-bottom: 6px; }
.summary { font-size: 13px; color: #9aa2b8; line-height: 2.1; background: #161a28; border-radius: 10px; padding: 14px 16px; }
.theme-chip { background: #161a28; border: 1.5px solid #232a3d; border-radius: 10px; padding: 10px 18px; cursor: pointer; text-align: center; }
.theme-chip:hover { border-color: #3a5e9e; }
.theme-chip.sel { border-color: #7eb8ff; background: #17223a; }
.theme-name { font-size: 13.5px; font-weight: 700; }
.theme-desc { font-size: 11px; color: #7c86a3; margin-top: 3px; }
</style>
