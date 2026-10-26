<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { NButton } from "naive-ui";
import { api, type ProjectDetail } from "../api";
import { refreshStatus, store } from "../store";

/* 主窗口全屏游玩路由：SPA 内部导航，绕开子窗口 WebView2 兼容问题 */
const route = useRoute();
const router = useRouter();
const id = route.params.id as string;
const key = ref(0);
const name = ref("");

/* B2 首跑引导层：首次进入项目（localStorage 无 played.<id>）先看三步上手，
   点「开始游玩」写标记并放行 iframe；二次进入直接进游戏。 */
const guide = ref(false);

const url = computed(() => {
  const port = store.status?.server?.port;
  return port ? `http://127.0.0.1:${port}/projects/${id}/` : "";
});

/* 2026-10-26 十角色测评 P0：端口未就绪时 url 为空，旧版只剩黑屏零提示
   ——小白以为软件死了。三态兜底：未就绪卡（可重试/浏览器打开）、加载中
   提示、慢加载兜底建议。 */
const loaded = ref(false);
const slowLoad = ref(false);
let slowTimer: number | undefined;
watch([url, key], () => {
  loaded.value = false;
  slowLoad.value = false;
  if (slowTimer !== undefined) clearTimeout(slowTimer);
  if (url.value) {
    slowTimer = window.setTimeout(() => { slowLoad.value = true; }, 10000);
  }
});
async function retry() {
  await refreshStatus();
  key.value++;
}

onMounted(async () => {
  try {
    const d: ProjectDetail = await api.readProject(id);
    name.value = String(d.game.name ?? "");
  } catch { /* 标题缺省 */ }
  try {
    guide.value = !localStorage.getItem(`played.${id}`);
  } catch { /* localStorage 不可用（极端隐私模式）：不引导直接进 */ }
});

function startPlay() {
  try {
    localStorage.setItem(`played.${id}`, "1");
  } catch { /* 写不进就本次放行，下次再引导 */ }
  guide.value = false;
}

function openBrowser() {
  api.openProjectInBrowser(id).catch(() => {});
}
</script>

<template>
  <div class="play">
    <div class="play-bar">
      <NButton size="tiny" quaternary @click="router.push('/projects')">← 返回</NButton>
      <b class="play-name">🎮 {{ name || id }}</b>
      <span style="flex: 1"></span>
      <NButton size="tiny" quaternary @click="key++">⟳ 刷新</NButton>
      <NButton size="tiny" quaternary @click="openBrowser">🌐 浏览器打开</NButton>
    </div>
    <div class="play-stage">
      <iframe v-if="url" :key="key" :src="url" class="play-frame" :title="name || id" @load="loaded = true"></iframe>
      <!-- 未就绪兜底：不再黑屏静默 -->
      <div v-if="!url" class="play-dead" role="alert">
        <p class="dead-title">🎮 游戏服务还没就绪</p>
        <p class="dead-sub">内置服务器可能仍在启动，或本次会话未启动成功。</p>
        <div class="dead-actions">
          <NButton size="small" type="primary" @click="retry">⟳ 重试</NButton>
          <NButton size="small" quaternary @click="openBrowser">🌐 用系统浏览器打开</NButton>
          <NButton size="small" quaternary @click="router.push('/projects')">← 回项目列表</NButton>
        </div>
      </div>
      <div v-else-if="!loaded" class="play-loading" aria-live="polite">
        游戏启动中…（首次约 3-8 秒）
        <span v-if="slowLoad" class="slow-hint">比预期慢？试试右上角「🌐 浏览器打开」</span>
      </div>
      <!-- B2 首跑引导覆盖层：只挡游戏区，顶栏返回/刷新仍可用 -->
      <div v-if="guide" class="guide-layer" role="dialog" aria-modal="true" aria-label="首跑引导">
        <div class="guide-card">
          <h3 class="guide-title">🎮 首次进入「{{ name || id }}」</h3>
          <p class="guide-sub">三步上手，30 秒开玩</p>
          <ol class="guide-steps">
            <li>
              <span class="guide-num">1</span>
              <span>本局可能是<b>离线预览</b>——到模型中心配好 Key，NPC 对话与剧情导演即由真 AI 驱动</span>
            </li>
            <li>
              <span class="guide-num">2</span>
              <span>在<b>对话面板</b>说话，NPC 会记住你</span>
            </li>
            <li>
              <span class="guide-num">3</span>
              <span>左侧<b>剧情页</b>推进故事</span>
            </li>
          </ol>
          <div class="guide-actions">
            <NButton type="primary" @click="startPlay">开始游玩</NButton>
            <NButton size="small" quaternary @click="router.push('/models')">前往模型中心 ↗</NButton>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.play { height: 100vh; display: flex; flex-direction: column; background: #0a0a0c; }
.play-bar { display: flex; align-items: center; gap: 10px; padding: 6px 12px;
  background: #141419; border-bottom: 1px solid #202027; color: #a8a8b3; }
.play-name { font-size: 13px; color: #e9e9ee; }
.play-stage { flex: 1; position: relative; display: flex; }
.play-frame { flex: 1; width: 100%; border: 0; background: #0a0a0c; }
.play-dead {
  flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center;
  gap: 6px; color: #a0a0ab; background: #0a0a0c;
}
.dead-title { font-size: 16px; color: #e9e9ee; margin: 0; }
.dead-sub { font-size: 13px; margin: 0 0 10px; }
.dead-actions { display: flex; gap: 10px; }
.play-loading {
  position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center;
  justify-content: center; gap: 8px; color: #8b8b96; font-size: 13px; pointer-events: none;
  background: #0a0a0c;
}
.slow-hint { font-size: 12px; color: #c9a6ff; }
.guide-layer { position: absolute; inset: 0; z-index: 10; display: flex;
  align-items: center; justify-content: center; background: rgba(10, 10, 12, .9); }
.guide-card { width: min(460px, 88vw); background: #141419; border: 1px solid #202027;
  border-radius: 14px; padding: 26px 28px; }
.guide-title { margin: 0; font-size: 17px; color: #e9e9ee; }
.guide-sub { margin: 6px 0 16px; font-size: 12.5px; color: #6f6f7a; }
.guide-steps { list-style: none; margin: 0 0 20px; padding: 0; display: flex;
  flex-direction: column; gap: 12px; }
.guide-steps li { display: flex; gap: 12px; align-items: flex-start;
  font-size: 13px; line-height: 1.65; color: #a8a8b3; }
.guide-steps b { color: #9fc3ff; font-weight: 600; }
.guide-num { flex: none; width: 22px; height: 22px; border-radius: 50%;
  background: #1a2a3f; color: #7eb8ff; font-size: 12px; font-weight: 700;
  display: flex; align-items: center; justify-content: center; margin-top: 1px; }
.guide-actions { display: flex; align-items: center; gap: 10px; }
</style>
