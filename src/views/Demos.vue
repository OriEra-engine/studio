<script setup lang="ts">
import { onMounted, ref } from "vue";
import { NCard, NGrid, NGi, NTag, NSpace, NButton, NAlert, NSpin } from "naive-ui";
import { useMessage } from "naive-ui";
import { api, type DemoInfo } from "../api";
import { RouterLink } from "vue-router";

const demos = ref<DemoInfo[]>([]);
const loading = ref(true);
const opening = ref("");
const message = useMessage();

onMounted(async () => {
  try {
    demos.value = await api.listDemos();
  } finally {
    loading.value = false;
  }
});

async function open(d: DemoInfo) {
  opening.value = d.id;
  try {
    await api.openDemo(d.id);
    localStorage.setItem("oe.seenDemo", "1");
  } finally {
    opening.value = "";
  }
}

async function openInBrowser(d: DemoInfo) {
  try {
    await api.openDemoInBrowser(d.id);
    localStorage.setItem("oe.seenDemo", "1");
  } catch (e) {
    message?.error?.(String(e));
  }
}
</script>

<template>
  <div class="page">
    <h2 class="page-title">Demo 演示厅</h2>
    <p class="page-sub">引擎全部示例已内置，点击即在新窗口运行，无需安装任何环境。AI 相关 demo 会自动植入「模型中心」里配置的模型。</p>

    <NAlert type="info" style="margin-bottom: 16px" v-if="!loading && demos.some(d => d.featured)">
      推荐从 <b>「隅角咖啡馆」</b> 开始——3D 世界里的 AI NPC 会记忆、聊天、推进剧情。
      想让 NPC 接上真实大模型，先到
      <RouterLink to="/models">模型中心</RouterLink>
      完成一次配置（智谱 glm-4-flash 有免费额度，国内直连）。
    </NAlert>

    <NSpin :show="loading">
      <NGrid :cols="4" :x-gap="14" :y-gap="14" responsive="screen" item-responsive>
        <NGi v-for="d in demos" :key="d.id" :span="d.featured ? '4 m:2' : '2 m:1'">
          <NCard class="demo-card" :class="{ featured: d.featured }" size="small" @click="open(d)">
            <template #header>
              <span style="font-size: 14px">{{ d.title }}</span>
            </template>
            <template #header-extra>
              <NTag :type="d.featured ? 'success' : 'default'" size="small" round>{{ d.tag }}</NTag>
            </template>
            <p class="demo-desc">{{ d.desc }}</p>
            <NSpace justify="end">
              <NButton size="small" quaternary @click.stop="openInBrowser(d)">🌐 浏览器打开</NButton>
              <NButton size="small" type="primary" ghost :loading="opening === d.id">
                {{ opening === d.id ? "启动中" : "运行" }}
              </NButton>
            </NSpace>
          </NCard>
        </NGi>
      </NGrid>
    </NSpin>
  </div>
</template>
