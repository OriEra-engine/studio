<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { NButton } from "naive-ui";
import { api, type ProjectDetail } from "../api";
import { store } from "../store";

/* 主窗口全屏游玩路由：SPA 内部导航，绕开子窗口 WebView2 兼容问题 */
const route = useRoute();
const router = useRouter();
const id = route.params.id as string;
const key = ref(0);
const name = ref("");

const url = computed(() => {
  const port = store.status?.server?.port;
  return port ? `http://127.0.0.1:${port}/projects/${id}/` : "";
});

onMounted(async () => {
  try {
    const d: ProjectDetail = await api.readProject(id);
    name.value = String(d.game.name ?? "");
  } catch { /* 标题缺省 */ }
});

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
    <iframe v-if="url" :key="key" :src="url" class="play-frame" :title="name || id"></iframe>
  </div>
</template>

<style scoped>
.play { height: 100vh; display: flex; flex-direction: column; background: #0d1017; }
.play-bar { display: flex; align-items: center; gap: 10px; padding: 6px 12px;
  background: #131726; border-bottom: 1px solid #232a3d; color: #aeb6cc; }
.play-name { font-size: 13px; color: #e6e8ef; }
.play-frame { flex: 1; width: 100%; border: 0; background: #0d1017; }
</style>
