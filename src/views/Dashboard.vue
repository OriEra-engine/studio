<script setup lang="ts">
import { onMounted, ref } from "vue";
import { NCard, NGrid, NGi, NButton, NSpace, NTag, NStatistic } from "naive-ui";
import { api, type AppStatus } from "../api";
import { RouterLink } from "vue-router";

const status = ref<AppStatus | null>(null);
const loading = ref(true);
const seenDemo = ref(localStorage.getItem("oe.seenDemo") === "1");

onMounted(async () => {
  try {
    status.value = await api.getStatus();
  } finally {
    loading.value = false;
  }
});

const isFresh = () => status.value && !status.value.provider_configured && status.value.project_count === 0;
</script>

<template>
  <div class="page">
    <div class="hero">
      <h1>欢迎使用 OriEra Studio</h1>
      <p>
        AI 原生、虚实融合、可自演化开放世界的桌面工作台。引擎演示已内置，无需安装任何工具——
        点左侧「Demo 演示厅」直接体验；想拥有自己的 AI 游戏，走「新建游戏」五步向导。
      </p>
      <NSpace>
        <RouterLink to="/demos"><NButton type="primary">进入演示厅</NButton></RouterLink>
        <RouterLink to="/new-game"><NButton type="info" ghost>新建我的游戏</NButton></RouterLink>
        <RouterLink to="/models"><NButton secondary>配置 AI 模型</NButton></RouterLink>
      </NSpace>
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

    <NGrid :cols="4" :x-gap="14" :y-gap="14" responsive="screen" item-responsive>
      <NGi span="4 m:2">
        <NCard title="内置服务" size="small">
          <template #header-extra>
            <NTag :type="status ? 'success' : 'default'" size="small" round>
              {{ status ? "运行中" : "检测中" }}
            </NTag>
          </template>
          <NStatistic label="本地演示服务器" :value="status ? `127.0.0.1:${status.server.port}` : '—'" />
        </NCard>
      </NGi>
      <NGi span="4 m:2">
        <NCard title="引擎快照" size="small">
          <template #header-extra>
            <NTag type="info" size="small" round>已内置</NTag>
          </template>
          <NStatistic label="OriEra Engine" :value="status?.server.snapshot_version ?? '—'" />
        </NCard>
      </NGi>
      <NGi span="4 m:2">
        <NCard title="AI 模型" size="small">
          <template #header-extra>
            <NTag :type="status?.provider_configured ? 'success' : 'warning'" size="small" round>
              {{ status?.provider_configured ? "已植入" : "未配置" }}
            </NTag>
          </template>
          <NStatistic
            label="当前 provider"
            :value="status?.provider ? `${status.provider.model}` : '规则引擎（离线）'"
          />
          <div style="margin-top: 8px" v-if="!status?.provider_configured">
            <RouterLink to="/models"><NButton size="small" type="primary" ghost>去配置模型</NButton></RouterLink>
          </div>
        </NCard>
      </NGi>
      <NGi span="4 m:2">
        <NCard title="我的游戏" size="small">
          <template #header-extra>
            <NTag size="small" round>{{ status?.project_count ?? 0 }} 个</NTag>
          </template>
          <NStatistic label="已生成项目" :value="status?.project_count ?? 0" />
          <div style="margin-top: 8px">
            <RouterLink to="/projects"><NButton size="small" secondary>查看项目</NButton></RouterLink>
          </div>
        </NCard>
      </NGi>
    </NGrid>
  </div>
</template>

<style scoped>
.step-card { background: #161a28; border: 1.5px solid #232a3d; border-radius: 12px; padding: 14px; height: 100%; display: flex; flex-direction: column; gap: 6px; }
.step-card.done { opacity: .6; }
.step-no { width: 26px; height: 26px; border-radius: 50%; background: #2d5b9e; color: #fff; display: flex; align-items: center; justify-content: center; font-size: 13px; font-weight: 700; }
.step-card.done .step-no { background: transparent; }
.step-t { font-weight: 700; font-size: 14px; }
.step-d { color: #8a93ad; font-size: 12px; line-height: 1.6; margin-bottom: 4px; flex: 1; }
</style>
