<script setup lang="ts">
import { onMounted, ref } from "vue";
import { NCard, NGrid, NGi, NButton, NEmpty, NSpace, NTag, useMessage, NPopconfirm } from "naive-ui";
import { useRouter } from "vue-router";
import { api, type ProjectMeta } from "../api";
import { refreshStatus } from "../store";

const message = useMessage();
const router = useRouter();
const projects = ref<ProjectMeta[]>([]);
const loading = ref(true);

async function refresh() {
  loading.value = true;
  try {
    projects.value = await api.listProjects();
  } finally {
    loading.value = false;
  }
}
onMounted(refresh);

async function run(p: ProjectMeta) {
  try {
    await api.openProject(p.id);
  } catch (e) {
    message.error(String(e));
  }
}
async function reveal(p: ProjectMeta) {
  try {
    await api.revealProjectDir(p.id);
  } catch (e) {
    message.error(String(e));
  }
}
async function remove(p: ProjectMeta) {
  try {
    await api.deleteProject(p.id);
    message.success(`已删除「${p.name}」`);
    await refresh();
    refreshStatus(); // 同步仪表盘的项目计数
  } catch (e) {
    message.error(String(e));
  }
}

function fmtDate(unix: string) {
  const d = new Date(Number(unix) * 1000);
  return `${d.getMonth() + 1}-${d.getDate()} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}
</script>

<template>
  <div class="page">
    <h2 class="page-title">我的项目</h2>
    <p class="page-sub">由「新建游戏」向导生成的 AI 原型都在这里，随时运行、改造、删除。</p>

    <NEmpty v-if="!loading && projects.length === 0" description="还没有项目——去「新建游戏」五步生成一个吧" style="margin: 60px 0">
      <template #extra>
        <router-link to="/new-game"><NButton type="primary">🚀 新建游戏</NButton></router-link>
      </template>
    </NEmpty>

    <NGrid :cols="3" :x-gap="14" :y-gap="14" responsive="screen" item-responsive>
      <NGi v-for="p in projects" :key="p.id" span="3 m:1">
        <NCard size="small" class="proj-card">
          <template #header>
            <b>🎮 {{ p.name }}</b>
          </template>
          <template #header-extra>
            <NTag size="tiny" round>{{ fmtDate(p.created) }}</NTag>
          </template>
          <p style="color: #8a93ad; font-size: 12.5px; min-height: 34px">{{ p.tagline }}</p>
          <NSpace size="small">
            <NButton size="small" type="primary" @click="run(p)">运行</NButton>
            <NButton size="small" secondary @click="router.push(`/editor/${p.id}`)">✏️ 编辑</NButton>
            <NButton size="small" secondary @click="reveal(p)">打开目录</NButton>
            <NPopconfirm @positive-click="remove(p)">
              <template #trigger>
                <NButton size="small" quaternary type="error">删除</NButton>
              </template>
              确认删除「{{ p.name }}」？此操作不可恢复。
            </NPopconfirm>
          </NSpace>
        </NCard>
      </NGi>
    </NGrid>
  </div>
</template>
