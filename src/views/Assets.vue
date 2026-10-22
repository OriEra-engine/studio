<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  NButton, NCard, NGrid, NGi, NInput, NTag, NSpace, NEmpty, NPopconfirm,
  NSelect, useMessage, NSpin,
} from "naive-ui";
import { api, type ProjectMeta } from "../api";

interface AssetMeta {
  id: string;
  file: string;
  prompt: string;
  created: string;
}

const message = useMessage();
const prompt = ref("");
const generating = ref(false);
const assets = ref<AssetMeta[]>([]);
const port = ref<number | null>(null);
const projects = ref<ProjectMeta[]>([]);
const targetProject = ref<string | null>(null);
const busyId = ref("");

function assetUrl(file: string): string {
  return port.value ? `http://127.0.0.1:${port.value}/${file}` : "";
}

async function refresh() {
  try {
    assets.value = await api.listAssets();
  } catch { /* 浏览器降级 */ }
  try {
    const st = await (await import("../api")).api.getStatus();
    port.value = st.server.port;
  } catch { /* 降级 */ }
  try {
    projects.value = await api.listProjects();
  } catch { /* 降级 */ }
}

async function generate() {
  const p = prompt.value.trim();
  if (!p) return message.warning("先描述你想要的插画（例如：雨夜咖啡馆窗边的猫，赛博朋克霓虹）");
  generating.value = true;
  try {
    const meta = await api.generateAsset(p);
    message.success("插画已生成并入库");
    prompt.value = "";
    await refresh();
    targetProject.value = targetProject.value || (projects.value[0]?.id ?? null);
    if (targetProject.value) {
      await api.addAssetToProject(meta.id, targetProject.value);
      message.success("已自动挂进最近项目的画廊墙（运行中窗口已刷新）");
    }
  } catch (e) {
    message.error(String(e));
  } finally {
    generating.value = false;
  }
}

async function addToProject(a: AssetMeta) {
  if (!targetProject.value) return message.warning("先在右上角选择目标项目");
  busyId.value = a.id;
  try {
    await api.addAssetToProject(a.id, targetProject.value);
    message.success("已挂进该项目画廊墙（运行中窗口已自动刷新）");
  } catch (e) {
    message.error(String(e));
  } finally {
    busyId.value = "";
  }
}

async function remove(a: AssetMeta) {
  try {
    await api.deleteAsset(a.id);
    await refresh();
  } catch (e) {
    message.error(String(e));
  }
}

onMounted(refresh);
</script>

<template>
  <div class="page">
    <h2 class="page-title">🎨 AI 素材中心</h2>
    <p class="page-sub">一句话生成插画（智谱 CogView，免费档），一键挂进你游戏的 3D 画廊墙。</p>

    <NCard size="small" style="margin-bottom: 16px">
      <template #header>✨ 生成插画</template>
      <template #header-extra>
        <NSpace size="small" align="center">
          <span style="font-size: 12px; color: #8a90a6">挂到项目</span>
          <NSelect
            v-model:value="targetProject" size="small" style="width: 220px"
            :options="projects.map(p => ({ label: p.name, value: p.id }))"
            placeholder="选择项目"
            clearable
          />
        </NSpace>
      </template>
      <NInput
        v-model:value="prompt" type="textarea" :rows="2"
        placeholder="描述画面：例如「雨夜咖啡馆窗边，一只猫看着霓虹灯，赛博朋克风格」"
        @keydown.enter.exact.prevent="generate"
      />
      <NSpace style="margin-top: 10px" align="center">
        <NButton type="primary" :loading="generating" @click="generate">
          {{ generating ? "生成中（约 10-20 秒）…" : "✨ 生成插画" }}
        </NButton>
        <span style="font-size: 12px; color: #8a90a6">
          智谱 CogView · cogview-3-flash 免费档 · 需在模型中心配置智谱 Key
        </span>
      </NSpace>
    </NCard>

    <NSpin :show="false">
      <NEmpty v-if="assets.length === 0" description="还没有资产——在上面输入描述生成第一张吧" style="margin: 40px 0" />
      <NGrid v-else :cols="4" :x-gap="14" :y-gap="14" responsive="screen" item-responsive>
        <NGi v-for="a in assets" :key="a.id" span="4 m:1">
          <NCard size="small" class="asset-card">
            <template #cover>
              <img v-if="port" :src="assetUrl(a.file)" class="asset-thumb" :alt="a.prompt" loading="lazy" />
              <div v-else class="asset-thumb asset-empty">内置服务未就绪</div>
            </template>
            <p class="asset-prompt" :title="a.prompt">{{ a.prompt }}</p>
            <NSpace size="small" justify="end">
              <NSelect
                size="tiny" style="width: 130px" placeholder="挂到项目"
                :options="projects.map(p => ({ label: p.name, value: p.id }))"
                @update:value="(v: string) => { targetProject = v; addToProject(a); }"
              />
              <NButton size="tiny" :loading="busyId === a.id" @click="addToProject(a)">挂载</NButton>
              <NPopconfirm @positive-click="remove(a)">
                <template #trigger><NButton size="tiny" quaternary type="error">删除</NButton></template>
                删除这张资产？
              </NPopconfirm>
            </NSpace>
          </NCard>
        </NGi>
      </NGrid>
    </NSpin>
  </div>
</template>

<style scoped>
.asset-card :deep(.n-card-cover) { height: 160px; overflow: hidden; }
.asset-thumb { width: 100%; height: 160px; object-fit: cover; display: block; }
.asset-empty { display: flex; align-items: center; justify-content: center; color: #5f6b8a; font-size: 12px; background: #10141f; }
.asset-prompt { font-size: 12px; color: #8a90a6; line-height: 1.5; margin: 0 0 8px;
  display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
</style>
