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
  kind?: string; // image（插画）| mesh（3D 模型，Rodin）；旧数据缺省 = image
}

const message = useMessage();
const prompt = ref("");
const generating = ref(false);
// Rodin 文生 3D（key 只进 localStorage 与本次请求，不入配置文件）
const meshPrompt = ref("");
const rodinKey = ref(localStorage.getItem("oe.rodinKey") || "");
const rodinBase = ref(localStorage.getItem("oe.rodinBase") || "");
const meshGenerating = ref(false);
// Seedance 文生视频（key 只进 localStorage 与本次请求，不入配置文件）
const videoPrompt = ref("");
const arkKey = ref(localStorage.getItem("oe.arkKey") || "");
const videoModel = ref<string>("doubao-seedance-2-0-mini-260615");
const videoRatio = ref<string>("16:9");
const videoGenerating = ref(false);
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

async function generateMesh() {
  const p = meshPrompt.value.trim();
  if (!p) return message.warning("先描述你想要的 3D 模型（例如：一个圆润的机器人咖啡杯，低多边形风格）");
  if (!rodinKey.value.trim()) return message.warning("先填入 Hyper3D API Key（hyper3d.ai 控制台申请）");
  meshGenerating.value = true;
  try {
    localStorage.setItem("oe.rodinKey", rodinKey.value.trim());
    localStorage.setItem("oe.rodinBase", rodinBase.value.trim());
    const meta = await api.generateMesh(p, rodinKey.value.trim(), rodinBase.value.trim() || undefined);
    message.success("3D 模型已生成并入库（GLB）");
    meshPrompt.value = "";
    await refresh();
  } catch (e) {
    message.error(String(e));
  } finally {
    meshGenerating.value = false;
  }
}

async function generateVideo() {
  const p = videoPrompt.value.trim();
  if (!p) return message.warning("先描述你想要的镜头（例如：雨夜地铁站台空镜，镜头缓推，霓虹反射在湿地面上）");
  if (!arkKey.value.trim()) return message.warning("先填入火山方舟 API Key（console.volcengine.com/ark 控制台创建）");
  videoGenerating.value = true;
  try {
    localStorage.setItem("oe.arkKey", arkKey.value.trim());
    const meta = await api.generateVideo(p, arkKey.value.trim(), undefined, videoModel.value, videoRatio.value);
    message.success("视频已生成并入库（MP4，无水印）");
    videoPrompt.value = "";
    await refresh();
  } catch (e) {
    message.error(String(e));
  } finally {
    videoGenerating.value = false;
  }
}

// 批量镜头量产：每行一个 prompt，串行生成 + 成本估算（实测价位 mini≈¥2.5 / 2.5≈¥8.4 每 5s@720p）
const batchText = ref("");
const batchModel = ref<string>("doubao-seedance-2-0-mini-260615");
const batchRatio = ref<string>("16:9");
const batchRunning = ref(false);
const batchDone = ref(0);
const batchFail = ref<string[]>([]);
const batchCost = ref(0);
const UNIT: Record<string, number> = {
  "doubao-seedance-2-0-mini-260615": 2.5,
  "doubao-seedance-2-5-260628": 8.4,
};
async function runBatch() {
  const lines = batchText.value.split("\n").map(x => x.trim()).filter(Boolean);
  if (!lines.length) return message.warning("每行粘贴一个镜头描述");
  if (!arkKey.value.trim()) return message.warning("先填入火山方舟 API Key");
  localStorage.setItem("oe.arkKey", arkKey.value.trim());
  batchRunning.value = true; batchDone.value = 0; batchFail.value = []; batchCost.value = 0;
  for (const [i, line] of lines.entries()) {
    try {
      await api.generateVideo(line, arkKey.value.trim(), undefined, batchModel.value, batchRatio.value);
      batchDone.value++;
      batchCost.value += UNIT[batchModel.value] ?? 0;
    } catch (e) {
      batchFail.value.push(`第${i + 1}行: ${String(e).slice(0, 80)}`);
    }
    await refresh();
  }
  batchRunning.value = false;
  message.success(`批量完成：${batchDone.value}/${lines.length} 条入库，估算 ≈¥${batchCost.value.toFixed(1)}`);
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
          <span style="font-size: 12px; color: #71717c">挂到项目</span>
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
        <span style="font-size: 12px; color: #71717c">
          智谱 CogView · cogview-3-flash 免费档 · 需在模型中心配置智谱 Key
        </span>
      </NSpace>
    </NCard>

    <NCard size="small" style="margin-bottom: 16px">
      <template #header>🧊 生成 3D 模型（Hyper3D Rodin）</template>
      <NInput
        v-model:value="meshPrompt" type="textarea" :rows="2"
        placeholder="描述 3D 模型：例如「一个黄铜咖啡壶，低多边形，手绘贴图风格」"
        @keydown.enter.exact.prevent="generateMesh"
      />
      <NSpace style="margin-top: 10px" align="center" :wrap="false">
        <NInput
          v-model:value="rodinKey" type="password" show-password-on="click"
          placeholder="Hyper3D API Key（只存在本机浏览器）" style="max-width: 260px"
        />
        <NSelect
          v-model:value="rodinBase" size="small" style="width: 180px" clearable
          :options="[
            { label: '国际站 hyper3d.com', value: '' },
            { label: '国内站 hyper3d.cn', value: 'https://hyper3d.cn/api/v2' },
          ]" placeholder="站点"
        />
        <NButton type="primary" :loading="meshGenerating" @click="generateMesh">
          {{ meshGenerating ? "生成中（约 1-3 分钟）…" : "🧊 生成 3D 模型" }}
        </NButton>
      </NSpace>
      <p style="font-size: 12px; color: #71717c; margin: 8px 0 0">
        生成 GLB 模型入库；挂进场景随素材中心 M2（换肤/角色槽）跟进。
      </p>
    </NCard>

    <NCard size="small" style="margin-bottom: 16px">
      <template #header>🎬 生成视频（火山方舟 Seedance）</template>
      <NInput
        v-model:value="videoPrompt" type="textarea" :rows="2"
        placeholder="描述镜头：例如「深夜空旷地铁站台，雨从通风口斜飘，镜头缓慢下摇，湿地面反射霓虹，电影感青蓝色调」"
        @keydown.enter.exact.prevent="generateVideo"
      />
      <NSpace style="margin-top: 10px" align="center" :wrap="false">
        <NInput
          v-model:value="arkKey" type="password" show-password-on="click"
          placeholder="方舟 API Key（只存在本机浏览器）" style="max-width: 250px"
        />
        <NSelect
          v-model:value="videoModel" size="small" style="width: 200px"
          :options="[
            { label: '2.0-mini 草稿档（约¥2.5/5s）', value: 'doubao-seedance-2-0-mini-260615' },
            { label: '2.5 高光档（约¥8.4/5s）', value: 'doubao-seedance-2-5-260628' },
          ]"
        />
        <NSelect
          v-model:value="videoRatio" size="small" style="width: 110px"
          :options="[
            { label: '16:9 横屏', value: '16:9' },
            { label: '9:16 竖屏', value: '9:16' },
          ]"
        />
        <NButton type="primary" :loading="videoGenerating" @click="generateVideo">
          {{ videoGenerating ? "生成中（实测约 1.5-3.5 分钟）…" : "🎬 生成视频" }}
        </NButton>
      </NSpace>
      <p style="font-size: 12px; color: #71717c; margin: 8px 0 0">
        5 秒 720p 无水印 MP4 入库；开通 Seedance 需方舟账户余额 ≥200 元（wire 实测见 interfilm/docs/R0-report.md）。
      </p>
    </NCard>

    <NCard size="small" style="margin-bottom: 16px">
      <template #header>🎞️ 批量镜头量产</template>
      <NInput
        v-model:value="batchText" type="textarea" :rows="4"
        placeholder="每行一个镜头，例如：\n深夜空旷地铁站台，雨从通风口斜飘，镜头缓慢下摇\n车厢尽头黑衣人剪影，黑伞拄地，暖黄顶灯闪烁"
      />
      <NSpace style="margin-top: 10px" align="center" :wrap="false">
        <NSelect v-model:value="batchModel" size="small" style="width: 200px"
          :options="[
            { label: '2.0-mini 草稿档（≈¥2.5/条）', value: 'doubao-seedance-2-0-mini-260615' },
            { label: '2.5 高光档（≈¥8.4/条，闲时约55%）', value: 'doubao-seedance-2-5-260628' },
          ]" />
        <NSelect v-model:value="batchRatio" size="small" style="width: 110px"
          :options="[{ label: '16:9 横屏', value: '16:9' }, { label: '9:16 竖屏', value: '9:16' }]" />
        <NButton type="primary" :loading="batchRunning" @click="runBatch">
          {{ batchRunning ? `生成中 ${batchDone.value}…（每条约 2-4 分钟）` : "🎞️ 开跑批量" }}
        </NButton>
        <span v-if="batchCost" style="font-size: 12px; color: #ff9a3c">已入账 ≈¥{{ batchCost.toFixed(1) }}</span>
      </NSpace>
      <div v-if="batchFail.length" style="font-size: 12px; color: #e5484d; margin-top: 6px">
        失败 {{ batchFail.length }} 条：{{ batchFail.slice(0, 3).join("；") }}
      </div>
      <p style="font-size: 12px; color: #71717c; margin: 8px 0 0">
        串行提交防限流；夜间闲时跑 2.5 档约省 45%。单条失败自动继续，结束汇总。
      </p>
    </NCard>

    <NSpin :show="false">
      <NEmpty v-if="assets.length === 0" description="还没有资产——在上面输入描述生成第一张吧" style="margin: 40px 0" />
      <NGrid v-else :cols="4" :x-gap="14" :y-gap="14" responsive="screen" item-responsive>
        <NGi v-for="a in assets" :key="a.id" span="4 m:1">
          <NCard size="small" class="asset-card">
            <template #cover>
              <div v-if="a.kind === 'mesh'" class="asset-thumb asset-mesh">
                <span style="font-size: 34px">🧊</span>
                <span style="font-size: 11px; color: #6fd3a0">GLB · {{ a.file.split('/').pop() }}</span>
              </div>
              <video v-else-if="a.kind === 'video' && port" :src="assetUrl(a.file)"
                class="asset-thumb" controls preload="metadata" muted />
              <img v-else-if="port" :src="assetUrl(a.file)" class="asset-thumb" :alt="a.prompt" loading="lazy" />
              <div v-else class="asset-thumb asset-empty">内置服务未就绪</div>
            </template>
            <p class="asset-prompt" :title="a.prompt">{{ a.prompt }}</p>
            <NSpace size="small" justify="end">
              <template v-if="a.kind !== 'mesh' && a.kind !== 'video'">
                <NSelect
                  size="tiny" style="width: 130px" placeholder="挂到项目"
                  :options="projects.map(p => ({ label: p.name, value: p.id }))"
                  @update:value="(v: string) => { targetProject = v; addToProject(a); }"
                />
                <NButton size="tiny" :loading="busyId === a.id" @click="addToProject(a)">挂载</NButton>
              </template>
              <NTag v-else-if="a.kind === 'video'" size="tiny" type="warning" round>视频镜头</NTag>
              <NTag v-else size="tiny" type="success" round>3D 模型</NTag>
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
.asset-empty { display: flex; align-items: center; justify-content: center; color: #55555f; font-size: 12px; background: #0e0e12; }
.asset-mesh { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 4px; height: 160px; background: linear-gradient(160deg, #16233f, #0e0e12); }
.asset-prompt { font-size: 12px; color: #71717c; line-height: 1.5; margin: 0 0 8px;
  display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
</style>
