<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import {
  NCard, NGrid, NGi, NTag, NButton, NSpace, NInput, NAlert, NSpin, NProgress,
  NRadioGroup, NRadio, NPopconfirm, NSelect, useMessage,
} from "naive-ui";
import { listen } from "@tauri-apps/api/event";
import { api, type HardwareInfo, type Recommendation, type ProviderCfg, type OllamaStatus } from "../api";
import { CLOUD_PRESETS, type CloudPreset } from "../api";
import { store, refreshStatus, saveProvider as storeSaveProvider, clearProvider as storeClearProvider } from "../store";

const message = useMessage();
const hw = ref<HardwareInfo | null>(null);
const rec = ref<Recommendation | null>(null);
const detecting = ref(true);

const form = reactive<ProviderCfg>({ kind: "", key: "", model: "", base: "" });
const saving = ref(false);
const testing = ref(false);
const testResult = ref<{ ok: boolean; text: string } | null>(null);
// 已植入状态来自全局 store：其他页面保存后回到这里无需重拉
const configured = computed(() => store.status?.provider_configured ?? false);

// ---- 全部厂商(筛选) ----
type FilterId = "all" | "free" | "cn" | "vision";
const FILTERS: { id: FilterId; label: string }[] = [
  { id: "all", label: "全部" },
  { id: "free", label: "免费" },
  { id: "cn", label: "国内直连" },
  { id: "vision", label: "带视觉" },
];
const filter = ref<FilterId>("all");
const filteredPresets = computed(() => {
  switch (filter.value) {
    case "free": return CLOUD_PRESETS.filter(p => p.free);
    case "cn": return CLOUD_PRESETS.filter(p => p.cn);
    case "vision": return CLOUD_PRESETS.filter(p => p.vision);
    default: return CLOUD_PRESETS;
  }
});
function pickPreset(p: CloudPreset, model?: string) {
  form.kind = p.kind;
  form.model = model || p.model;
  form.base = p.base;
  testResult.value = null;
  message.info(`已选择 ${model || p.model},粘贴 API Key 后保存`);
}

// ---- 任务槽位(RFC 0002 槽位路由 · 数据先行预览) ----
const SLOTS = [
  { id: "npc_dialog", name: "NPC 对话", duty: "高频 · 低延迟 · 成本敏感", recKind: "zhipu", recModel: "glm-4-flash", why: "免费 Flash 档,零成本高频对话", live: true },
  { id: "director", name: "剧情导演", duty: "长上下文 · 世界观一致", recKind: "moonshot", recModel: "kimi-k2.6", why: "256K 装得下整本世界书", live: true },
  // v0.7.5（评审铁律：追不到管线的按钮不许上屏）：runtime 尚未消费的槽位置灰展示
  { id: "codegen", name: "行为生成", duty: "代码与推理优先", recKind: "deepseek", recModel: "deepseek-chat", why: "代码性价比", live: false },
  { id: "reviewer", name: "生成审校", duty: "视觉 + 文本评审", recKind: "zhipu", recModel: "glm-4.6v-flash", why: "免费多模态,截图评审零成本", live: false },
];
const ROUTING_KEY = "oriera.routing.v1";
// v0.7.3：槽位条目升级为 {kind, model, key?}（key 留空=同厂商继承全局 Key；
// 跨厂商未填 Key 运行时诚实回退全局）。旧字符串格式自动迁移为对象。
interface SlotEntry { kind: string; model?: string; key?: string }
type SlotVal = SlotEntry | string | undefined;
const routing = ref<Record<string, SlotVal>>({});
try { routing.value = JSON.parse(localStorage.getItem(ROUTING_KEY) || "{}"); } catch { /* 坏数据忽略 */ }
const slotOptions = CLOUD_PRESETS.map(p => ({ label: p.label, value: p.kind }));
function slotEntry(s: { id: string; recKind: string; recModel: string }): SlotEntry {
  const v = routing.value[s.id];
  if (typeof v === "string") return { kind: v, model: CLOUD_PRESETS.find(p => p.kind === v)?.model };
  if (v && typeof v === "object" && v.kind) return v;
  return { kind: s.recKind, model: s.recModel };
}
function persistRouting() {
  localStorage.setItem(ROUTING_KEY, JSON.stringify(routing.value));
}
function onSlotChange(id: string, kind: string) {
  const prev = slotEntry(SLOTS.find(x => x.id === id)!);
  routing.value[id] = { kind, model: prev.kind === kind ? (prev.model || "") : (CLOUD_PRESETS.find(p => p.kind === kind)?.model || "") };
  persistRouting();
  const p = CLOUD_PRESETS.find(x => x.kind === kind);
  message.success(`槽位 ${id} → ${p?.label ?? kind}（已接入运行时，重开游戏生效）`);
}
function onSlotModel(id: string, model: string) {
  const e = slotEntry(SLOTS.find(x => x.id === id)!);
  routing.value[id] = { ...e, model };
  persistRouting();
}
function onSlotKey(id: string, key: string) {
  const e = slotEntry(SLOTS.find(x => x.id === id)!);
  routing.value[id] = { ...e, key };
  persistRouting();
}
/** 跨厂商且未填 Key → 运行时回退全局（界面如实提示） */
function slotFallbackHint(s: { id: string }): string {
  const e = slotEntry(s);
  const g = store.status?.provider;
  if (e.kind === "rule") return "跟随全局";
  if (g && e.kind !== g.kind && !e.key) return "未填该厂商 Key，运行时回退全局";
  return "";
}
function slotKindOf(s: { id: string; recKind: string }): string {
  return slotEntry(s).kind;
}

// ---- XLite 实时模型清单（公开 pricing 接口，经本机代理免 CORS） ----
const serverPort = ref<number | null>(null);
const xliteLive = ref<{ models: string[]; count: number } | null>(null);
async function loadXliteLive() {
  if (!serverPort.value || xliteLive.value) return;
  try {
    const r = await fetch(`http://127.0.0.1:${serverPort.value}/proxy/xlite/api/pricing`);
    const d = await r.json();
    const text = ((d.data || []) as { model_name: string; catalog_categories?: string[] }[])
      .filter(m => (m.catalog_categories || []).includes("text_chat"));
    xliteLive.value = { models: text.slice(0, 12).map(m => m.model_name), count: text.length };
  } catch { /* 拉不到就静默回退静态 pills */ }
}

// Ollama
const ollama = ref<OllamaStatus | null>(null);
const pulling = ref("");
const pullPercent = ref<number | null>(null);
const pullStatus = ref("");

async function detect() {
  detecting.value = true;
  try {
    hw.value = await api.detectHardware();
    rec.value = await api.getRecommendations(hw.value);
  } finally {
    detecting.value = false;
  }
}

async function loadCurrent() {
  await refreshStatus();
  const provider = store.status?.provider;
  // XLite 实时模型清单：经本机代理拉公开 pricing 接口（拉不到静默回退静态 pills）
  serverPort.value = store.status?.server?.port ?? null;
  void loadXliteLive();
  if (provider) {
    form.kind = provider.kind;
    form.model = provider.model;
    form.base = provider.base;
    form.key = ""; // key 不回显
  }
}

function pickCloud(kind: string, model: string, base: string) {
  form.kind = kind; form.model = model; form.base = base;
  testResult.value = null;
  message.info(`已选择 ${model}，粘贴 API Key 后保存`);
}

function pickLocal() {
  if (!rec.value?.local) return;
  const p = rec.value.local.provider;
  form.kind = p.kind; form.model = p.model; form.base = p.base; form.key = p.key;
  testResult.value = null;
  message.info("已选择 Ollama 本地方案（需要本机已安装并拉取模型）");
}

async function save() {
  if (!form.kind) return message.warning("先选择一个模型方案");
  if (!form.key && form.key !== "ollama") return message.warning("请粘贴 API Key");
  saving.value = true;
  try {
    await storeSaveProvider({ ...form });
    message.success("已保存并植入引擎——已打开的 demo/游戏窗口将自动重载生效");
  } catch (e) {
    message.error(`保存失败：${e}`);
  } finally {
    saving.value = false;
  }
}

async function test() {
  testing.value = true;
  testResult.value = null;
  try {
    const reply = await api.testProvider();
    testResult.value = { ok: true, text: reply };
  } catch (e) {
    testResult.value = { ok: false, text: String(e) };
  } finally {
    testing.value = false;
  }
}

async function clearCfg() {
  await storeClearProvider();
  form.key = "";
  message.success("已清除模型配置，回到离线规则引擎");
}

// ---- Ollama ----
const gpuVram = ref<[number, number] | null>(null);
async function refreshOllama() {
  ollama.value = await api.ollamaStatus();
  try { gpuVram.value = await api.gpuVramUsage(); } catch { gpuVram.value = null; }
}
async function deleteModel(name: string) {
  try {
    await api.ollamaDeleteModel(name);
    message.success(`已删除 ${name}`);
    await refreshOllama();
  } catch (e) {
    message.error(String(e));
  }
}
async function serveOllama() {
  try {
    await api.ollamaServe();
    await refreshOllama();
    message.success("Ollama 服务已启动");
  } catch (e) {
    message.error(String(e));
  }
}
async function pullRecommended() {
  const m = rec.value?.local?.provider.model;
  if (!m) return;
  pulling.value = m;
  pullPercent.value = 0;
  pullStatus.value = "准备下载…";
  try {
    await api.ollamaPull(m);
    message.success(`${m} 拉取完成，已可使用`);
    await refreshOllama();
    pickLocal();
  } catch (e) {
    message.error(String(e));
  } finally {
    pulling.value = "";
    pullPercent.value = null;
  }
}
async function useOllamaModel(name: string) {
  form.kind = "openai";
  form.model = name;
  form.base = "http://127.0.0.1:11434/v1";
  form.key = "ollama";
  await save();
}

onMounted(async () => {
  listen<{ model: string; status: string; percent: number | null }>("ollama-pull-progress", e => {
    pullStatus.value = e.payload.status || "";
    pullPercent.value = e.payload.percent ?? null;
  });
  await detect();
  await loadCurrent();
  await refreshOllama();
});

const vramGb = () => (hw.value?.max_vram_mb ? (hw.value.max_vram_mb / 1024).toFixed(1) : null);
</script>

<template>
  <div class="page">
    <h2 class="page-title">模型中心</h2>
    <p class="page-sub">体检你的电脑 → 选择推荐的模型方案 → 一键植入引擎。配置一次，所有 Demo 与生成的游戏全部生效。</p>

    <!-- 硬件体检 -->
    <NSpin :show="detecting">
      <NCard title="🖥️ 硬件体检" size="small" style="margin-bottom: 14px">
        <template #header-extra>
          <NButton size="tiny" quaternary @click="detect">重新检测</NButton>
        </template>
        <NGrid :cols="4" :x-gap="12" responsive="screen" item-responsive>
          <NGi span="4 m:1"><div class="hw-item"><div class="hw-k">处理器</div><div class="hw-v">{{ hw?.cpu || '—' }}</div></div></NGi>
          <NGi span="4 m:1"><div class="hw-item"><div class="hw-k">内存</div><div class="hw-v">{{ hw ? hw.ram_gb.toFixed(0) + ' GB' : '—' }}</div></div></NGi>
          <NGi span="4 m:1"><div class="hw-item"><div class="hw-k">显卡</div><div class="hw-v">{{ hw?.gpus?.[0]?.name || '未检测到独立显卡' }}</div></div></NGi>
          <NGi span="4 m:1"><div class="hw-item"><div class="hw-k">显存</div>
            <div class="hw-v">
              {{ vramGb() ? vramGb() + ' GB' : '—' }}
              <NTag v-if="hw?.gpus?.[0]" size="tiny" round style="margin-left: 6px">{{ hw.gpus[0].vram_source }}</NTag>
            </div></div></NGi>
        </NGrid>
      </NCard>
    </NSpin>

    <!-- 推荐方案 -->
    <NCard v-if="rec" :title="`🧠 ${rec.headline}`" size="small" style="margin-bottom: 14px">
      <p style="color: #9aa2b8; font-size: 13px; line-height: 1.7; margin: 0 0 14px">{{ rec.detail }}</p>
      <NGrid :cols="3" :x-gap="12" :y-gap="12" responsive="screen" item-responsive>
        <NGi v-for="c in rec.cloud" :key="c.kind" span="3 m:1">
          <div class="rec-card" :class="{ sel: form.kind === c.kind }" @click="pickCloud(c.kind, c.model, c.base)">
            <div class="rec-head">
              <b>{{ c.label }}</b>
              <NTag v-if="c.free" type="success" size="tiny" round>免费额度</NTag>
            </div>
            <div class="rec-note">{{ c.note }}</div>
            <div class="rec-actions">
              <NButton size="tiny" secondary @click.stop="pickCloud(c.kind, c.model, c.base)">选择</NButton>
              <NButton v-if="c.signup_url" size="tiny" quaternary tag="a" :href="c.signup_url" target="_blank">注册入口 ↗</NButton>
            </div>
          </div>
        </NGi>
        <NGi v-if="rec.local" span="3 m:1">
          <div class="rec-card local" :class="{ sel: form.model === rec.local.provider.model }" @click="pickLocal">
            <div class="rec-head">
              <b>🏠 本地 Ollama · {{ rec.local.ollama_model }}</b>
              <NTag type="info" size="tiny" round>零 token 费</NTag>
            </div>
            <div class="rec-note">{{ rec.local.size_note }} · 约需 {{ rec.local.vram_need_gb }}GB 显存 / {{ rec.local.disk_gb }}GB 磁盘</div>
            <div class="rec-actions">
              <NButton size="tiny" secondary @click.stop="pickLocal">选择</NButton>
              <NButton size="tiny" quaternary tag="a" href="https://ollama.com/download" target="_blank">下载 Ollama ↗</NButton>
            </div>
          </div>
        </NGi>
      </NGrid>
    </NCard>

    <!-- 全部云厂商 -->
    <NCard title="☁️ 全部云厂商 · 自由选择" size="small" style="margin-bottom: 14px">
      <template #header-extra>
        <NSpace size="small" :wrap="false">
          <NTag
            v-for="f in FILTERS" :key="f.id" size="small" round
            :type="filter === f.id ? 'primary' : 'default'" style="cursor: pointer"
            @click="filter = f.id"
          >{{ f.label }}</NTag>
        </NSpace>
      </template>
      <NGrid :cols="3" :x-gap="12" :y-gap="12" responsive="screen" item-responsive>
        <NGi v-for="p in filteredPresets" :key="p.kind" span="3 m:1">
          <div
            class="rec-card" :class="{ sel: form.kind === p.kind }"
            role="button" tabindex="0" :aria-pressed="form.kind === p.kind" :aria-label="`选择厂商 ${p.label}`"
            @click="pickPreset(p)" @keydown.enter.prevent="pickPreset(p)" @keydown.space.prevent="pickPreset(p)"
          >
            <div class="rec-head">
              <b>{{ p.label }}</b>
              <span>
                <NTag v-if="p.badge" size="tiny" round type="success" style="margin-right: 4px">{{ p.badge }}</NTag>
                <NTag v-if="p.free" type="success" size="tiny" round>免费</NTag>
              </span>
            </div>
            <div class="rec-note">{{ p.note }}</div>
            <div v-if="p.models" class="model-row">
              <span
                v-for="m in p.models" :key="m"
                class="model-pill-mini" :class="{ on: form.model === m && form.kind === p.kind }"
                role="button" tabindex="0" :aria-label="`选择模型 ${m}`"
                @click.stop="pickPreset(p, m)" @keydown.enter.stop.prevent="pickPreset(p, m)" @keydown.space.stop.prevent="pickPreset(p, m)"
              >{{ m }}</span>
            </div>
            <div v-if="p.kind === 'xlite' && xliteLive" class="model-row">
              <span class="model-pill-mini live-tag" title="来自网关实时价目接口">实时 · {{ xliteLive.count }} 个文本模型</span>
              <span
                v-for="m in xliteLive.models" :key="`live-${m}`"
                class="model-pill-mini" :class="{ on: form.model === m && form.kind === p.kind }"
                role="button" tabindex="0" :aria-label="`选择模型 ${m}`"
                @click.stop="pickPreset(p, m)" @keydown.enter.stop.prevent="pickPreset(p, m)" @keydown.space.stop.prevent="pickPreset(p, m)"
              >{{ m }}</span>
            </div>
            <div class="rec-actions">
              <NButton size="tiny" secondary @click.stop="pickPreset(p)">选择</NButton>
              <NButton v-if="p.url" size="tiny" quaternary tag="a" :href="p.url" target="_blank">注册入口 ↗</NButton>
            </div>
          </div>
        </NGi>
      </NGrid>
      <p style="color: #8a90a6; font-size: 11.5px; margin: 12px 0 0">
        价格与免费额度以各厂商定价页为准(引擎不背书价格);任何 OpenAI 兼容端点也可在下方「其他 OpenAI 兼容」自由填写。
      </p>
    </NCard>

    <!-- 任务槽位(v0.7.3 已接入运行时:对话/导演槽生效;行为/审校预留) -->
    <NCard size="small" style="margin-bottom: 14px">
      <template #header>🧭 任务槽位 <NTag size="tiny" type="success" round style="margin-left: 6px" :bordered="false">已接入运行时</NTag></template>
      <p style="color: #9aa2b8; font-size: 12.5px; line-height: 1.7; margin: 0 0 12px">
        按「任务」而非全局选模型:对话走 npc_dialog 槽、剧情推进走 director 槽(游戏与咖啡馆 demo 已生效);
        行为生成/生成审校为预留槽位。Key 留空=同厂商继承全局 Key(XLite 一 Key 多模型最顺手),
        跨厂商需填该槽自己的 Key,未填则运行时诚实回退全局模型。槽位表存本机(<code>oriera.routing.v1</code>),重开游戏生效。
      </p>
      <NGrid :cols="4" :x-gap="12" :y-gap="12" responsive="screen" item-responsive>
        <NGi v-for="s in SLOTS" :key="s.id" span="4 m:1">
          <div class="slot-card" :class="{ 'slot-off': !s.live }">
            <div class="slot-head">
              <b>{{ s.name }}</b>
              <span class="slot-id">{{ s.live ? s.id : s.id + ' · v0.8 接入' }}</span>
            </div>
            <div class="slot-duty">{{ s.duty }}</div>
            <NSelect
              size="small" :value="slotKindOf(s)" :options="slotOptions" :disabled="!s.live"
              @update:value="(v: string) => onSlotChange(s.id, v)"
            />
            <NInput
              size="small" style="margin-top: 6px" :value="slotEntry(s).model || ''" :disabled="!s.live"
              placeholder="模型名（默认用该厂商推荐档）"
              @update:value="(v: string) => onSlotModel(s.id, v)"
            />
            <NInput
              size="small" style="margin-top: 6px" type="password" show-password-on="click" :disabled="!s.live"
              :value="slotEntry(s).key || ''" placeholder="该槽 Key（留空=同厂商用全局）"
              @update:value="(v: string) => onSlotKey(s.id, v)"
            />
            <div v-if="slotFallbackHint(s)" class="slot-fb">{{ slotFallbackHint(s) }}</div>
            <div class="slot-why">推荐:{{ CLOUD_PRESETS.find(p => p.kind === s.recKind)?.label }} · {{ s.why }}</div>
            <div v-if="!s.live" class="slot-why" style="color:#e6a23c">运行时尚未消费此槽位，配置暂不生效</div>
          </div>
        </NGi>
      </NGrid>
    </NCard>

    <!-- Ollama 本地管理 -->
    <NCard v-if="rec?.local" title="🏠 本地模型（Ollama）" size="small" style="margin-bottom: 14px">
      <template #header-extra>
        <NTag :type="ollama?.running ? 'success' : ollama?.installed ? 'warning' : 'default'" size="small" round>
          {{ ollama?.running ? "运行中" : ollama?.installed ? "已安装未运行" : "未安装" }}
        </NTag>
      </template>
      <NAlert v-if="!ollama?.installed" type="warning" :show-icon="false" style="margin-bottom: 10px; font-size: 12.5px">
        未检测到 Ollama。下载安装（约 500MB，国内可直连）：
        <a href="https://ollama.com/download" target="_blank" style="margin-left: 4px">ollama.com/download ↗</a>
        ，装完回到这里点「刷新状态」。
      </NAlert>
      <NSpace style="margin-bottom: 10px">
        <NButton size="small" secondary @click="refreshOllama">刷新状态</NButton>
        <NButton v-if="ollama?.installed && !ollama?.running" size="small" type="primary" ghost @click="serveOllama">启动 Ollama 服务</NButton>
        <NButton
          v-if="ollama?.running && !ollama?.models.some(m => m.name === rec.local!.provider.model)"
          size="small" type="primary" :loading="!!pulling" @click="pullRecommended"
        >
          拉取推荐模型 {{ rec.local.provider.model }}（{{ rec.local.disk_gb }}GB）
        </NButton>
      </NSpace>
      <div v-if="pulling">
        <NProgress :percentage="pullPercent ?? 0" processing indicator-placement="inside" />
        <p style="color: #7c86a3; font-size: 12px; margin: 6px 0 0">{{ pullStatus }}</p>
      </div>
      <div v-if="gpuVram" style="margin-top: 8px; font-size: 12px; color: #8a93ad">
        🎮 显存：{{ (gpuVram[0]/1024).toFixed(1) }} / {{ (gpuVram[1]/1024).toFixed(1) }} GB
        <NTag v-if="gpuVram[1] && gpuVram[0]/gpuVram[1] > 0.85" type="warning" size="tiny" round style="margin-left: 6px">占用偏高</NTag>
      </div>
      <div v-if="ollama?.models.length" style="margin-top: 10px">
        <div style="font-size: 12px; color: #7c86a3; margin-bottom: 8px">本机模型（点击使用 / ✕ 删除）：</div>
        <NSpace size="small">
          <div v-for="m in ollama.models" :key="m.name" class="model-pill">
            <span style="cursor: pointer" @click="useOllamaModel(m.name)">{{ m.name }}（{{ m.size_gb.toFixed(1) }}GB）</span>
            <NPopconfirm @positive-click="deleteModel(m.name)">
              <template #trigger><span class="model-del">✕</span></template>
              删除模型 {{ m.name }}？（释放磁盘，不影响已生成项目）
            </NPopconfirm>
          </div>
        </NSpace>
      </div>
    </NCard>

    <!-- 当前配置 -->
    <NCard title="🔌 植入引擎" size="small">
      <template #header-extra>
        <NTag :type="configured ? 'success' : 'warning'" size="small" round>{{ configured ? '已植入' : '未配置' }}</NTag>
      </template>
      <NGrid :cols="2" :x-gap="16" responsive="screen" item-responsive>
        <NGi span="2 m:1">
          <NSpace vertical size="small">
            <div class="fld"><label>服务商 / 端点类型</label>
              <NSelect
                v-model:value="form.kind" size="small"
                :options="[...slotOptions, { label: '其他 OpenAI 兼容(自由填写)', value: 'openai-compat' }]"
                placeholder="选择或已在上方卡片选择"
              />
            </div>
            <div class="fld"><label>模型名称</label><NInput v-model:value="form.model" placeholder="例如 glm-4-flash" /></div>
            <div class="fld"><label>API Base URL</label><NInput v-model:value="form.base" placeholder="https://..." /></div>
            <div class="fld"><label>API Key<span style="color:#8a90a6">（存入系统凭据管理器，不落盘）</span></label>
              <NInput v-model:value="form.key" type="password" show-password-on="click" placeholder="粘贴密钥" /></div>
          </NSpace>
        </NGi>
        <NGi span="2 m:1">
          <NAlert type="info" :show-icon="false" style="margin-bottom: 12px">
            未配置模型时，所有 demo 与游戏自动降级为<b>离线规则引擎</b>，依然可玩。
            配置后：AI 咖啡馆的 NPC、生成游戏里的角色与导演系统全部由大模型驱动。
          </NAlert>
          <NSpace>
            <NButton type="primary" :loading="saving" @click="save">保存并植入</NButton>
            <NButton :loading="testing" :disabled="!configured" @click="test">测试连接</NButton>
            <NButton quaternary type="error" :disabled="!configured" @click="clearCfg">清除配置</NButton>
          </NSpace>
          <div v-if="testResult" style="margin-top: 12px">
            <NAlert :type="testResult.ok ? 'success' : 'error'" :show-icon="false" style="font-size: 12.5px">
              {{ testResult.ok ? '✅ 连接成功：' + testResult.text : '❌ ' + testResult.text }}
            </NAlert>
          </div>
        </NGi>
      </NGrid>
    </NCard>
  </div>
</template>

<style scoped>
.hw-item { background: #161a28; border-radius: 10px; padding: 10px 14px; }
.hw-k { font-size: 11px; color: #7c86a3; margin-bottom: 4px; }
.hw-v { font-size: 13px; color: #cdd6ec; }
.rec-card { background: #161a28; border: 1.5px solid #232a3d; border-radius: 12px; padding: 14px; cursor: pointer; height: 100%; transition: border-color .12s; }
.rec-card:hover { border-color: #3a5e9e; }
.rec-card.sel { border-color: #7eb8ff; background: #17223a; }
.model-pill { display: inline-flex; align-items: center; gap: 8px; background: #161a28; border: 1px solid #2a3a5c; border-radius: 20px; padding: 4px 12px; font-size: 12px; }
.model-pill:hover { border-color: #3a5e9e; }
.model-del { cursor: pointer; color: #7c86a3; font-size: 11px; }
.model-del:hover { color: #ff8f8f; }
.rec-head { display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px; font-size: 13.5px; }
.rec-note { font-size: 12px; color: #8a93ad; line-height: 1.6; margin-bottom: 10px; }
.rec-actions { display: flex; gap: 8px; }
.rec-card:focus-visible, .model-pill-mini:focus-visible { outline: 2px solid #7eb8ff; outline-offset: 2px; }
.model-row { display: flex; gap: 5px; flex-wrap: wrap; margin-bottom: 10px; }
.model-pill-mini { font-size: 11px; color: #aeb6cc; background: #10141f; border: 1px solid #2a3a5c; border-radius: 10px; padding: 1px 8px; cursor: pointer; }
.model-pill-mini:hover { border-color: #3a5e9e; color: #9fd0ff; }
.model-pill-mini.on { border-color: #7eb8ff; color: #9fd0ff; background: #17223a; }
.live-tag { color: #7ce38b; border-style: dashed; cursor: default; }
.slot-card { background: #161a28; border: 1.5px solid #232a3d; border-radius: 12px; padding: 12px; height: 100%; }
.slot-card.slot-off { opacity: .55; }
.slot-head { display: flex; justify-content: space-between; align-items: baseline; margin-bottom: 4px; font-size: 13.5px; }
.slot-id { font-size: 10.5px; color: #8a90a6; font-family: Consolas, monospace; }
.slot-duty { font-size: 11.5px; color: #8a93ad; margin-bottom: 8px; }
.slot-why { font-size: 11px; color: #8a90a6; margin-top: 8px; line-height: 1.55; }
.slot-fb { font-size: 10.5px; color: #e6a23c; margin-top: 6px; }
.fld label { display: block; font-size: 12px; color: #8a93ad; margin-bottom: 6px; }
.fld { margin-bottom: 10px; }
</style>
