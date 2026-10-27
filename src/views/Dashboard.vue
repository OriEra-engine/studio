<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { NCard, NGrid, NGi, NButton, NSpace, NTag } from "naive-ui";
import { RouterLink, useRouter } from "vue-router";
import { store } from "../store";
import { wizard } from "../wizard-state";
import { api, type ProjectMeta } from "../api";

// 状态来自全局 store：模型中心保存后这里的徽章即时变绿，无需刷新
const status = computed(() => store.status);
const loading = computed(() => !store.loaded);
const seenDemo = ref(localStorage.getItem("oe.seenDemo") === "1");
const router = useRouter();

const isFresh = () => status.value && !status.value.provider_configured && status.value.project_count === 0;

/* ---- prompt-first（Codex 式）：一句话直达向导，idea 预填第 2 步 ---- */
const idea = ref("");
const IDEA_CHIPS = [
  "咖啡店经营 + 连环失踪案",
  "雪原地牢，火把是唯一资源",
  "赛博朋克酒吧的 AI 陪酒师",
  "修仙宗门料理大赛",
];
function startCreate(text?: string) {
  const t = (text ?? idea.value).trim();
  if (t) wizard.userIdea = t; // 向导第 2 步「你的灵感」自动带上
  router.push("/new-game");
}

/* ---- 最近项目（ZCode 会话列表式：状态点 + 时间） ---- */
const recent = ref<ProjectMeta[]>([]);
onMounted(async () => {
  try {
    const list = await api.listProjects();
    recent.value = [...list].sort((a, b) => Number(b.created) - Number(a.created)).slice(0, 3);
  } catch { /* 纯浏览器降级 */ }
});
function fmtDate(unix: string) {
  const d = new Date(Number(unix) * 1000);
  return `${d.getMonth() + 1}-${d.getDate()} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}
</script>

<template>
  <div class="page">
    <!-- prompt-first hero（Codex 式：首页即任务） -->
    <div class="hero-v2">
      <h1>今天想做<span class="grad">什么游戏</span>？</h1>
      <p class="hero-tip">一句话描述，五步向导带你出成品——没配模型也能玩默认剧情。</p>
      <div class="hero-composer">
        <textarea
          v-model="idea"
          rows="1"
          placeholder="例如：经营一家开在世界尽头的咖啡馆，常客里藏着一位失忆的侦探…"
          @keydown.enter.exact.prevent="startCreate()"
        ></textarea>
        <button class="hero-go" :disabled="!idea.trim()" @click="startCreate()">开始创造 →</button>
      </div>
      <div class="hero-chips">
        <span class="hero-chips-label">试试：</span>
        <button v-for="c in IDEA_CHIPS" :key="c" @click="startCreate(c)">{{ c }}</button>
      </div>
    </div>

    <!-- 首启三步欢迎流（零项目且未配模型时展示） -->
    <NCard v-if="!loading && isFresh()" title="👋 欢迎来到 OriEra——三步开启你的 AI 游戏" size="small" style="margin-bottom: 18px">
      <NGrid :cols="3" :x-gap="12" responsive="screen" item-responsive>
        <NGi span="3 m:1">
          <div class="step-card" :class="{ done: seenDemo }">
            <div class="step-no">{{ seenDemo ? '✅' : '1' }}</div>
            <div class="step-t">看一个 Demo</div>
            <div class="step-d">演示厅里的「隅角咖啡馆」有会记忆的 3D AI NPC</div>
            <RouterLink to="/demos"><NButton size="small" secondary>进入演示厅</NButton></RouterLink>
          </div>
        </NGi>
        <NGi span="3 m:1">
          <div class="step-card" :class="{ done: status?.provider_configured }">
            <div class="step-no">{{ status?.provider_configured ? '✅' : '2' }}</div>
            <div class="step-t">领取免费模型</div>
            <div class="step-d">智谱 glm-4-flash 国内直连有免费额度，注册即用</div>
            <RouterLink to="/models"><NButton size="small" secondary>去模型中心</NButton></RouterLink>
          </div>
        </NGi>
        <NGi span="3 m:1">
          <div class="step-card">
            <div class="step-no">3</div>
            <div class="step-t">生成你的游戏</div>
            <div class="step-d">选玩法 → 定剧情 → 五步拥有自己的 AI 原生游戏</div>
            <RouterLink to="/new-game"><NButton size="small" type="primary">🚀 开始</NButton></RouterLink>
          </div>
        </NGi>
      </NGrid>
    </NCard>

    <!-- 最近项目（会话列表式） -->
    <div v-if="recent.length" class="recent">
      <div class="recent-head">
        <span class="recent-title">最近项目</span>
        <RouterLink to="/projects" class="recent-more">全部 →</RouterLink>
      </div>
      <div class="recent-list">
        <RouterLink v-for="p in recent" :key="p.id" :to="`/editor/${p.id}`" class="recent-item">
          <span class="recent-emoji">🎮</span>
          <span class="recent-name">{{ p.name }}</span>
          <span class="recent-tagline">{{ p.tagline }}</span>
          <span class="recent-time mono">{{ fmtDate(p.created) }}</span>
        </RouterLink>
      </div>
    </div>

    <NGrid :cols="3" :x-gap="14" :y-gap="14" responsive="screen" item-responsive>
      <NGi span="3 m:1">
        <NCard title="内置服务" size="small">
          <template #header-extra>
            <span class="oe-badge"><span class="status-dot on pulse"></span>运行中</span>
          </template>
          <div class="stat-line"><span class="stat-label">本地演示服务器</span><span class="stat-val mono">{{ status ? `127.0.0.1:${status.server.port}` : '—' }}</span></div>
          <div class="stat-line"><span class="stat-label">引擎快照</span><span class="stat-val mono">{{ status?.server.snapshot_version ?? '—' }}</span></div>
        </NCard>
      </NGi>
      <NGi span="3 m:1">
        <NCard title="AI 模型" size="small">
          <template #header-extra>
            <span class="oe-badge" :style="status?.provider_configured ? '' : 'color: var(--warn)'">
              <span class="status-dot" :class="status?.provider_configured ? 'on' : ''"></span>
              {{ status?.provider_configured ? "已连接" : "未连接" }}
            </span>
          </template>
          <div class="stat-line">
            <span class="stat-label">当前 provider</span>
            <span class="stat-val mono">{{ status?.provider ? status.provider.model : '规则引擎（离线）' }}</span>
          </div>
          <div style="margin-top: 8px" v-if="!status?.provider_configured">
            <RouterLink to="/models"><NButton size="small" type="primary" ghost>去配置模型</NButton></RouterLink>
          </div>
        </NCard>
      </NGi>
      <NGi span="3 m:1">
        <NCard title="我的游戏" size="small">
          <template #header-extra>
            <NTag size="small" round>{{ status?.project_count ?? 0 }} 个</NTag>
          </template>
          <div class="stat-line"><span class="stat-label">已生成项目</span><span class="stat-val mono">{{ status?.project_count ?? 0 }}</span></div>
          <div style="margin-top: 8px">
            <RouterLink to="/projects"><NButton size="small" secondary>查看项目</NButton></RouterLink>
          </div>
        </NCard>
      </NGi>
    </NGrid>
  </div>
</template>

<style scoped>
/* ---- prompt-first hero（ZCode 中性黑：灰阶层次 + 极淡顶部提亮） ---- */
.hero-v2 {
  border-radius: 16px;
  padding: 34px 34px 26px;
  margin-bottom: 18px;
  background:
    radial-gradient(1100px 280px at 30% -70%, rgba(255, 255, 255, .045), transparent 60%),
    linear-gradient(160deg, #17171c 0%, #111114 55%, #0d0d10 100%);
  border: 1px solid var(--line);
}
.hero-v2 h1 { margin: 0 0 6px; font-size: 27px; letter-spacing: .3px; }
.hero-v2 h1 .grad {
  background: linear-gradient(120deg, #9fd0ff, #4f8ae8);
  -webkit-background-clip: text; background-clip: text; color: transparent;
}
.hero-tip { color: var(--text-3); font-size: 13px; margin: 0 0 18px; }

.hero-composer {
  display: flex; gap: 10px; align-items: stretch;
  background: #0a0a0c; border: 1px solid var(--line-strong); border-radius: 14px;
  padding: 8px 8px 8px 4px; transition: border-color .15s ease, box-shadow .15s ease;
}
.hero-composer:focus-within { border-color: #4a4a58; box-shadow: 0 0 0 3px rgba(255, 255, 255, .05); }
.hero-composer textarea {
  flex: 1; resize: none; background: none; border: none; outline: none;
  color: var(--text); font-family: inherit; font-size: 14.5px; line-height: 1.6;
  padding: 8px 10px; max-height: 160px; min-height: 38px;
}
.hero-composer textarea::placeholder { color: var(--text-3); }
.hero-go {
  flex: none; border: none; border-radius: 10px; padding: 0 22px;
  background: var(--accent-strong); color: #fff; font-size: 14px; font-weight: 600;
  cursor: pointer; font-family: inherit; transition: background .12s ease;
}
.hero-go:disabled { opacity: .4; cursor: default; }
.hero-go:not(:disabled):hover { background: #6ba1f2; }

.hero-chips { display: flex; flex-wrap: wrap; gap: 7px; align-items: center; margin-top: 14px; }
.hero-chips-label { color: var(--text-3); font-size: 12px; }
.hero-chips button {
  font-size: 12px; color: var(--text-2); background: rgba(20, 24, 39, .8);
  border: 1px solid var(--line); border-radius: 999px; padding: 3px 12px;
  cursor: pointer; font-family: inherit; transition: all .12s ease;
}
.hero-chips button:hover { color: var(--accent); border-color: #3a5f9e; background: var(--bg-hover); }

/* ---- 最近项目（会话列表式：hover 高亮整行） ---- */
.recent { margin: 20px 0 18px; }
.recent-head { display: flex; align-items: baseline; gap: 10px; margin-bottom: 8px; }
.recent-title { font-size: 14px; font-weight: 700; }
.recent-more { font-size: 12px; color: var(--text-3); }
.recent-more:hover { color: var(--accent); }
.recent-list { display: flex; flex-direction: column; gap: 6px; }
.recent-item {
  display: flex; align-items: center; gap: 10px;
  padding: 9px 14px; border: 1px solid var(--line); border-radius: 10px;
  background: var(--bg-raise); color: var(--text); text-decoration: none;
  transition: border-color .12s ease, background .12s ease;
}
.recent-item:hover { border-color: var(--line-strong); background: var(--bg-hover); }
.recent-emoji { flex: none; }
.recent-name { font-weight: 600; font-size: 13.5px; flex: none; }
.recent-tagline { color: var(--text-3); font-size: 12.5px; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.recent-time { color: var(--text-3); font-size: 11.5px; flex: none; }

/* ---- 状态卡数据行（等宽技术值） ---- */
.stat-line { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; }
.stat-label { color: var(--text-3); font-size: 12.5px; }
.stat-val { font-size: 15px; font-weight: 600; color: var(--text); }

.step-card { background: var(--bg-raise); border: 1.5px solid var(--line); border-radius: 12px; padding: 14px; height: 100%; display: flex; flex-direction: column; gap: 6px; }
.step-card.done { opacity: .6; }
.step-no { width: 26px; height: 26px; border-radius: 50%; background: var(--accent-deep); color: #fff; display: flex; align-items: center; justify-content: center; font-size: 13px; font-weight: 700; }
.step-card.done .step-no { background: transparent; }
.step-t { font-weight: 700; font-size: 14px; }
.step-d { color: var(--text-3); font-size: 12px; line-height: 1.6; margin-bottom: 4px; flex: 1; }
</style>
