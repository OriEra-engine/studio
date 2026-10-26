<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  NButton, NCard, NGrid, NGi, NInput, NTag, NSpace, NEmpty, NPopconfirm,
  NSelect, useMessage, NSpin, NPopover, NInputNumber,
} from "naive-ui";
import { api, type ProjectMeta } from "../api";

interface AssetMeta {
  id: string;
  file: string;
  prompt: string;
  created: string;
  kind?: string; // image（插画）| mesh（3D 模型，Rodin）；旧数据缺省 = image
  cost_est?: string | null; // 估算成本（如 "¥2.50"）；空 = 未登记牌价（Rodin，不发明数字）
}

const message = useMessage();
const prompt = ref("");
const generating = ref(false);
// Rodin 文生 3D（key 进系统凭据管理器，只写不读——明文不进 webview/磁盘）
const meshPrompt = ref("");
const rodinKeyInput = ref(""); // 只写：粘贴即保存，保存后立即清空
const rodinKeySaved = ref(false);
const rodinBase = ref(localStorage.getItem("oe.rodinBase") || "");
const meshGenerating = ref(false);
// Seedance 文生视频（同上，凭据条目 ark）
const videoPrompt = ref("");
const arkKeyInput = ref("");
const arkKeySaved = ref(false);

async function refreshSecrets() {
  try {
    rodinKeySaved.value = await api.secretHas("rodin");
    arkKeySaved.value = await api.secretHas("ark");
  } catch { /* 凭据管理器异常不阻断页面 */ }
}
// 一次性迁移：历史 localStorage 明文 key 收进凭据管理器后即删
async function migrateLegacyKeys() {
  const legacy: [string, string][] = [["rodin", "oe.rodinKey"], ["ark", "oe.arkKey"]];
  for (const [name, lsKey] of legacy) {
    const old = localStorage.getItem(lsKey);
    if (old && old.trim()) {
      try { await api.secretSave(name, old.trim()); } catch { /* 迁移失败不阻断 */ }
      localStorage.removeItem(lsKey);
    }
  }
}
async function saveRodinKey() {
  const v = rodinKeyInput.value.trim();
  if (!v) return message.warning("粘贴 Hyper3D API Key（hyper3d.ai 控制台申请）");
  try {
    await api.secretSave("rodin", v);
    rodinKeyInput.value = "";
    rodinKeySaved.value = true;
    message.success("Hyper3D Key 已存入系统凭据管理器（不落盘、不回显）");
  } catch (e) { message.error(String(e)); }
}
async function saveArkKey() {
  const v = arkKeyInput.value.trim();
  if (!v) return message.warning("粘贴火山方舟 API Key（console.volcengine.com/ark 创建）");
  try {
    await api.secretSave("ark", v);
    arkKeyInput.value = "";
    arkKeySaved.value = true;
    message.success("方舟 Key 已存入系统凭据管理器（不落盘、不回显）");
  } catch (e) { message.error(String(e)); }
}
const videoModel = ref<string>("doubao-seedance-2-0-mini-260615");
const videoRatio = ref<string>("16:9");
const videoGenerating = ref(false);
const assets = ref<AssetMeta[]>([]);
const port = ref<number | null>(null);
const projects = ref<ProjectMeta[]>([]);
const targetProject = ref<string | null>(null);
const busyId = ref("");

// ===== 成本护栏：本月支出条 + 预算上限（护栏不是锁——超预算确认后仍可生成） =====
// 口径：meta.json 的 cost_est 按 created 时间戳落在当月的才计入合计；
// Rodin 无公开牌价（cost_est 为空）显示条数但不计入合计。
function inCurrentMonth(a: AssetMeta): boolean {
  const d = new Date(Number(a.created) * 1000);
  const now = new Date();
  return d.getFullYear() === now.getFullYear() && d.getMonth() === now.getMonth();
}
const monthSpend = computed(() =>
  assets.value.reduce((sum, a) => {
    if (!a.cost_est || !inCurrentMonth(a)) return sum;
    const v = parseFloat(a.cost_est.replace(/[^\d.]/g, ""));
    return Number.isNaN(v) ? sum : sum + v;
  }, 0)
);
const monthUnpriced = computed(
  () => assets.value.filter((a) => !a.cost_est && inCurrentMonth(a)).length
);
// 预算存 localStorage（数字元）；0 / 空 = 不设上限
const monthlyBudget = ref<number | null>(null);
const savedBudget = Number(localStorage.getItem("oe.monthlyBudget"));
if (Number.isFinite(savedBudget) && savedBudget > 0) monthlyBudget.value = savedBudget;
const budgetDraft = ref<number | null>(monthlyBudget.value);
const overBudget = computed(
  () => monthlyBudget.value !== null && monthSpend.value >= monthlyBudget.value
);
const budgetLabel = computed(() => (monthlyBudget.value ? `¥${monthlyBudget.value.toFixed(2)}` : "未设"));
const overBudgetMsg = computed(
  () => `本月 AI 支出已超预算（¥${monthSpend.value.toFixed(2)}/${budgetLabel.value}），确认继续？`
);
function saveBudget() {
  const v = budgetDraft.value;
  if (v !== null && Number.isFinite(v) && v > 0) {
    monthlyBudget.value = v;
    localStorage.setItem("oe.monthlyBudget", String(v));
    message.success(`本月预算已设为 ¥${v.toFixed(2)}`);
  } else {
    monthlyBudget.value = null;
    localStorage.removeItem("oe.monthlyBudget");
    message.success("预算已清除（不设上限）");
  }
}
// 超预算时点生成：弹受控 NPopconfirm 确认，确认后照常继续
const askImage = ref(false);
const askMesh = ref(false);
const askVideo = ref(false);
const askBatch = ref(false);

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

// 生成入口：超预算先弹确认（NPopconfirm），确认后直接开跑——护栏不是锁
function generate() {
  if (overBudget.value) { askImage.value = true; return; }
  doGenerate();
}
function generateMesh() {
  if (overBudget.value) { askMesh.value = true; return; }
  doGenerateMesh();
}
function generateVideo() {
  if (overBudget.value) { askVideo.value = true; return; }
  doGenerateVideo();
}
function runBatch() {
  if (overBudget.value) { askBatch.value = true; return; }
  doRunBatch();
}

async function doGenerate() {
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

async function doGenerateMesh() {
  const p = meshPrompt.value.trim();
  if (!p) return message.warning("先描述你想要的 3D 模型（例如：一个圆润的机器人咖啡杯，低多边形风格）");
  if (!rodinKeySaved.value) return message.warning("先粘贴并保存 Hyper3D API Key（hyper3d.ai 控制台申请）");
  meshGenerating.value = true;
  try {
    localStorage.setItem("oe.rodinBase", rodinBase.value.trim());
    await api.generateMesh(p, rodinBase.value.trim() || undefined);
    message.success("3D 模型已生成并入库（GLB）");
    meshPrompt.value = "";
    await refresh();
  } catch (e) {
    message.error(String(e));
  } finally {
    meshGenerating.value = false;
  }
}

async function doGenerateVideo() {
  const p = videoPrompt.value.trim();
  if (!p) return message.warning("先描述你想要的镜头（例如：雨夜地铁站台空镜，镜头缓推，霓虹反射在湿地面上）");
  if (!arkKeySaved.value) return message.warning("先粘贴并保存火山方舟 API Key（console.volcengine.com/ark 创建）");
  videoGenerating.value = true;
  try {
    await api.generateVideo(p, undefined, videoModel.value, videoRatio.value);
    message.success("视频已生成并入库（MP4，无水印）——可在下方素材卡点「🎬 设为正片」挂进影游项目成为剧情拍", { duration: 5000 });
    videoPrompt.value = "";
    await refresh();
  } catch (e) {
    message.error(String(e));
  } finally {
    videoGenerating.value = false;
  }
}

// 批量镜头量产：每行一个 prompt，串行生成（2026-10-26 十角色测评：烧钱
// 操作必须有停止键——batchStop 在行间生效，当前条收尾即停）；实际计费以
// 后端落盘 cost_est 为唯一真源（防双倍扣费逻辑在 assets.rs），前端不自算
const batchText = ref("");
const batchModel = ref<string>("doubao-seedance-2-0-mini-260615");
const batchRatio = ref<string>("16:9");
const batchRunning = ref(false);
const batchDone = ref(0);
const batchFail = ref<string[]>([]);
const batchCost = ref(0);
const batchStop = ref(false);
const UNIT: Record<string, number> = {
  "doubao-seedance-2-0-mini-260615": 2.5,
  "doubao-seedance-2-5-260628": 7.5,
};
async function doRunBatch() {
  const lines = batchText.value.split("\n").map(x => x.trim()).filter(Boolean);
  if (!lines.length) return message.warning("每行粘贴一个镜头描述");
  if (!arkKeySaved.value) return message.warning("先粘贴并保存火山方舟 API Key");
  batchStop.value = false;
  batchRunning.value = true; batchDone.value = 0; batchFail.value = []; batchCost.value = 0;
  for (const [i, line] of lines.entries()) {
    if (batchStop.value) break;
    try {
      const meta = await api.generateVideo(line, undefined, batchModel.value, batchRatio.value);
      batchDone.value++;
      const est = parseFloat(String(meta.cost_est ?? "").replace(/[^\d.]/g, ""));
      batchCost.value += Number.isFinite(est) ? est : 0;
    } catch (e) {
      batchFail.value.push(`第${i + 1}行: ${String(e).slice(0, 80)}`);
    }
    await refresh();
  }
  batchRunning.value = false;
  message.success(
    `${batchStop.value ? "已停止" : "批量完成"}：${batchDone.value}/${lines.length} 条入库，实际计费 ≈¥${batchCost.value.toFixed(1)}` +
    (batchStop.value ? "（未跑的行未扣费）" : ""),
  );
}
function stopBatch() {
  batchStop.value = true;
  message.info("正在收尾当前一条，完成后即停……");
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

// ===== B1 影游 video 桥：把视频资产设为项目某拍正片 =====
// 拷贝进项目 clips/beatN.mp4 + 写 clips.json（素材库保留原件）；拍号 0-4 弹层选择。
const clipBeatDraft = ref<Record<string, number>>({});
const clipBeatBusy = ref("");
const beatOptions = [0, 1, 2, 3, 4].map((n) => ({ label: `第 ${n + 1} 拍`, value: n }));
async function setAsClip(a: AssetMeta) {
  if (!targetProject.value) return message.warning("先在右上角选择目标项目（正片要进项目才会被剧情拍引用）");
  const beat = clipBeatDraft.value[a.id] ?? 0;
  clipBeatBusy.value = a.id;
  try {
    await api.setVideoAsClip(a.id, targetProject.value, beat);
    message.success(`已拷贝为该项目第 ${beat + 1} 拍正片（clips/beat${beat}.mp4，运行中窗口已自动刷新）`);
  } catch (e) {
    message.error(String(e));
  } finally {
    clipBeatBusy.value = "";
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

onMounted(() => { void refresh(); void migrateLegacyKeys().then(refreshSecrets); });
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
        <NPopconfirm :show="askImage" @positive-click="() => { askImage = false; doGenerate(); }"
          @update:show="(v: boolean) => { if (!v) askImage = false; }">
          <template #trigger>
            <NButton type="primary" :loading="generating" @click="generate">
              {{ generating ? "生成中（约 10-20 秒）…" : "✨ 生成插画" }}
            </NButton>
          </template>
          {{ overBudgetMsg }}
        </NPopconfirm>
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
          v-model:value="rodinKeyInput" type="password" show-password-on="click"
          :placeholder="rodinKeySaved ? '已保存——粘贴新 Key 可替换' : 'Hyper3D API Key'"
          style="max-width: 230px" @keydown.enter.exact.prevent="saveRodinKey"
        />
        <NButton size="small" @click="saveRodinKey">保存</NButton>
        <NTag v-if="rodinKeySaved" size="small" type="success" :bordered="false">Key 已存凭据管理器</NTag>
        <NSelect
          v-model:value="rodinBase" size="small" style="width: 180px" clearable
          :options="[
            { label: '国际站 hyper3d.com', value: '' },
            { label: '国内站 hyper3d.cn', value: 'https://hyper3d.cn/api/v2' },
          ]" placeholder="站点"
        />
        <NPopconfirm :show="askMesh" @positive-click="() => { askMesh = false; doGenerateMesh(); }"
          @update:show="(v: boolean) => { if (!v) askMesh = false; }">
          <template #trigger>
            <NButton type="primary" :loading="meshGenerating" @click="generateMesh">
              {{ meshGenerating ? "生成中（约 1-3 分钟）…" : "🧊 生成 3D 模型" }}
            </NButton>
          </template>
          {{ overBudgetMsg }}
        </NPopconfirm>
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
          v-model:value="arkKeyInput" type="password" show-password-on="click"
          :placeholder="arkKeySaved ? '已保存——粘贴新 Key 可替换' : '方舟 API Key'"
          style="max-width: 220px" @keydown.enter.exact.prevent="saveArkKey"
        />
        <NButton size="small" @click="saveArkKey">保存</NButton>
        <NTag v-if="arkKeySaved" size="small" type="success" :bordered="false">Key 已存凭据管理器</NTag>
        <NSelect
          v-model:value="videoModel" size="small" style="width: 200px"
          :options="[
            { label: '2.0-mini 草稿档（约¥2.5/5s）', value: 'doubao-seedance-2-0-mini-260615' },
            { label: '2.5 高光档（约¥7.5/5s）', value: 'doubao-seedance-2-5-260628' },
          ]"
        />
        <NSelect
          v-model:value="videoRatio" size="small" style="width: 110px"
          :options="[
            { label: '16:9 横屏', value: '16:9' },
            { label: '9:16 竖屏', value: '9:16' },
          ]"
        />
        <NPopconfirm :show="askVideo" @positive-click="() => { askVideo = false; doGenerateVideo(); }"
          @update:show="(v: boolean) => { if (!v) askVideo = false; }">
          <template #trigger>
            <NButton type="primary" :loading="videoGenerating" @click="generateVideo">
              {{ videoGenerating ? "生成中（实测约 1.5-3.5 分钟）…" : "🎬 生成视频" }}
            </NButton>
          </template>
          {{ overBudgetMsg }}
        </NPopconfirm>
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
            { label: '2.5 高光档（≈¥7.5/条，闲时约55%）', value: 'doubao-seedance-2-5-260628' },
          ]" />
        <NSelect v-model:value="batchRatio" size="small" style="width: 110px"
          :options="[{ label: '16:9 横屏', value: '16:9' }, { label: '9:16 竖屏', value: '9:16' }]" />
        <NPopconfirm :show="askBatch" @positive-click="() => { askBatch = false; doRunBatch(); }"
          @update:show="(v: boolean) => { if (!v) askBatch = false; }">
          <template #trigger>
            <NButton type="primary" :loading="batchRunning" @click="runBatch">
              {{ batchRunning ? `生成中 ${batchDone}…（每条约 2-4 分钟）` : "🎞️ 开跑批量" }}
            </NButton>
          </template>
          {{ overBudgetMsg }}
        </NPopconfirm>
        <NButton v-if="batchRunning" size="small" type="warning" @click="stopBatch">⏹ 停止（当前条跑完即停）</NButton>
        <span v-if="batchCost" style="font-size: 12px; color: #ff9a3c">已入账 ≈¥{{ batchCost.toFixed(1) }}</span>
      </NSpace>
      <div v-if="batchFail.length" style="font-size: 12px; color: #e5484d; margin-top: 6px">
        失败 {{ batchFail.length }} 条：{{ batchFail.slice(0, 3).join("；") }}
      </div>
      <p style="font-size: 12px; color: #71717c; margin: 8px 0 0">
        串行提交防限流；夜间闲时跑 2.5 档约省 45%。单条失败自动继续，结束汇总。
      </p>
    </NCard>

    <div class="spend-bar">
      <span>
        本月 AI 支出 <b :class="{ over: overBudget }">¥{{ monthSpend.toFixed(2) }}</b> / 预算 {{ budgetLabel }}
        <span v-if="monthUnpriced" style="color: #71717c">
          （另有 {{ monthUnpriced }} 条 3D 未登记牌价，未计入）
        </span>
      </span>
      <NPopover trigger="click" placement="bottom-end"
        @update:show="(v: boolean) => { if (v) budgetDraft = monthlyBudget; }">
        <template #trigger>
          <NButton size="tiny" quaternary type="primary">预算</NButton>
        </template>
        <NSpace align="center" :wrap="false">
          <NInputNumber v-model:value="budgetDraft" size="small" :min="0" :show-button="false"
            placeholder="每月预算（元）" style="width: 150px" />
          <NButton size="small" type="primary" @click="saveBudget">保存</NButton>
        </NSpace>
        <p class="spend-tip">填 0 或留空 = 不设上限；超预算仅弹确认，不阻止生成。</p>
      </NPopover>
    </div>

    <NSpin :show="false">
      <NEmpty v-if="assets.length === 0" description="还没有资产——在上面输入描述生成第一张吧" style="margin: 40px 0" />
      <NGrid v-else :cols="4" :x-gap="14" :y-gap="14" responsive="screen" item-responsive>
        <NGi v-for="a in assets" :key="a.id" span="4 m:1">
          <NCard size="small" class="asset-card">
            <template #cover>
              <div v-if="a.kind === 'mesh'" class="asset-thumb asset-mesh">
                <span style="font-size: 34px">🧊</span>
                <span style="font-size: 11px; color: var(--good)">GLB · {{ a.file.split('/').pop() }}</span>
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
              <template v-else-if="a.kind === 'video'">
                <NTag size="tiny" type="warning" round>视频镜头</NTag>
                <NPopover trigger="click" placement="top">
                  <template #trigger>
                    <NButton size="tiny" type="primary" :loading="clipBeatBusy === a.id">🎬 设为正片</NButton>
                  </template>
                  <NSpace align="center" :wrap="false">
                    <NSelect
                      v-model:value="clipBeatDraft[a.id]" size="small" style="width: 110px"
                      :options="beatOptions" placeholder="第 1 拍"
                    />
                    <NButton size="small" type="primary" @click="setAsClip(a)">设为第 {{ (clipBeatDraft[a.id] ?? 0) + 1 }} 拍</NButton>
                  </NSpace>
                  <p style="font-size: 12px; color: #71717c; margin: 8px 0 0; max-width: 240px">
                    拷贝进目标项目 clips/ 成为该拍正片（素材库保留原件）；同拍重复设置会覆盖。
                  </p>
                </NPopover>
              </template>
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
.spend-bar { display: flex; align-items: center; justify-content: space-between; gap: 8px;
  margin-bottom: 10px; font-size: 13px; color: #d8d8de; }
.spend-bar b { color: var(--good); }
.spend-bar b.over { color: #e5484d; }
.spend-tip { font-size: 12px; color: #71717c; margin: 8px 0 0; }
.asset-card :deep(.n-card-cover) { height: 160px; overflow: hidden; }
.asset-thumb { width: 100%; height: 160px; object-fit: cover; display: block; }
.asset-empty { display: flex; align-items: center; justify-content: center; color: #55555f; font-size: 12px; background: #0e0e12; }
.asset-mesh { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 4px; height: 160px; background: linear-gradient(160deg, #16233f, #0e0e12); }
.asset-prompt { font-size: 12px; color: #71717c; line-height: 1.5; margin: 0 0 8px;
  display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
</style>
