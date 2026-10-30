<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { NButton, NTag } from "naive-ui";
import { api, type SimExportReport } from "../api";
import { refreshStatus, store } from "../store";

/* 具身仿真台：iframe 内嵌引擎 example/15-embodied-sim（rapier3d wasm
   镜像内核），左侧控制面板经 postMessage 驱动；导出走 tauri 命令落盘
   LeRobotDataset v2.1（见 src-tauri/src/sim.rs）。 */

interface SimEpisode {
  task: string;
  rows: Array<{ state: number[]; action: number[] }>;
}

interface SimEpisodesMsg {
  source: string;
  type: "episodes";
  episodes: SimEpisode[];
  obsNames: string[];
  actionNames: string[];
  task: string;
  fps: number;
}

const url = computed(() => {
  const port = store.status?.server?.port;
  return port ? `http://127.0.0.1:${port}/examples/15-embodied-sim/` : "";
});

const frame = ref<HTMLIFrameElement>();
const ready = ref(false);
const loading = ref(true);
const recording = ref(false);
const episodeCount = ref(0);
const frameCount = ref(0);
const note = ref("");
const exporting = ref(false);
const report = ref<SimExportReport | null>(null);
const errMsg = ref("");

function send(cmd: Record<string, unknown>) {
  frame.value?.contentWindow?.postMessage({ source: "oriera-sim", ...cmd }, "*");
}

function doReset() { send({ cmd: "reset" }); }
function doPolicy() { send({ cmd: "policy" }); note.value = "脚本策略运行中（IK 抓放）"; }
function toggleRecord() {
  recording.value = !recording.value;
  send({ cmd: "record" });
  if (recording.value) note.value = "录制中：遥操作或跑策略，每 tick 一行 (state, action)";
}

async function copyTrainCmd() {
  const dir = report.value?.dir ?? "<导出目录>";
  const trainCmd = `python engine/scripts/jsonl_to_lerobot_v21.py "${dir}"
lerobot-train --dataset.repo_id=<user>/<ds> --policy.type=act`;
  try {
    await navigator.clipboard.writeText(trainCmd);
    note.value = "训练命令已复制到剪贴板";
  } catch {
    errMsg.value = "剪贴板不可用，请手动复制：" + trainCmd;
  }
}

async function doExport() {
  exporting.value = true;
  errMsg.value = "";
  try {
    send({ cmd: "export" }); // 触发 iframe 把 episodes 发回来（message 事件里落盘）
  } finally {
    setTimeout(() => { exporting.value = false; }, 800);
  }
}

async function onMessage(ev: MessageEvent) {
  const m = ev.data as Partial<SimEpisodesMsg> & { source?: string; type?: string; note?: string };
  if (!m || m.source !== "oriera-sim") return;
  if (m.type === "progress") {
    // 所见即所录：录制行数/集数实时跳动（内核每 30 行回报一次）
    frameCount.value = Number((m as { rows?: number }).rows ?? 0);
    episodeCount.value = Number((m as { episodes?: number }).episodes ?? 0);
    if (frameCount.value > 0) {
      note.value = `录制中：第 ${episodeCount.value} 集，已 ${frameCount.value} 行`;
    }
  } else if (m.type === "episodes") {
    const msg = m as unknown as SimEpisodesMsg;
    try {
      report.value = await api.simExportLerobot(JSON.stringify(msg));
      recording.value = false;
      note.value = `已导出 ${report.value.episodes} 集 / ${report.value.frames} 行 → ${report.value.dir}`;
    } catch (e) {
      errMsg.value = String(e);
    }
  } else if (m.type === "state" && m.note === "success") {
    note.value = "🎉 成功送达目标区！";
  }
}

onMounted(async () => {
  await refreshStatus();
  window.addEventListener("message", onMessage);
});
onBeforeUnmount(() => window.removeEventListener("message", onMessage));

function onFrameLoad() {
  loading.value = false;
  ready.value = true;
}
</script>

<template>
  <div class="sim-root">
    <header class="sim-toolbar">
      <div class="sim-title">
        具身仿真台 <NTag size="small" type="info" :bordered="false">真实物理 · 一键导出可训练数据</NTag>
      </div>
      <div class="sim-actions">
        <NButton size="small" @click="doReset">↺ 重置</NButton>
        <NButton size="small" type="primary" @click="doPolicy">▶ 脚本策略</NButton>
        <NButton size="small" :type="recording ? 'error' : 'default'" @click="toggleRecord">
          {{ recording ? "■ 停止录制" : "● 开始录制" }}
        </NButton>
        <NButton size="small" :loading="exporting" :disabled="frameCount === 0 && episodeCount === 0" @click="doExport">
          ⤓ 导出数据集{{ frameCount > 0 ? `（${frameCount} 行）` : "" }}
        </NButton>
        <NButton v-if="report" size="small" quaternary @click="api.simRevealExport()">打开导出目录</NButton>
        <NButton v-if="report" size="small" quaternary @click="copyTrainCmd">复制训练命令</NButton>
      </div>
    </header>

    <div class="sim-main">
      <div class="sim-frame-wrap">
        <div v-if="!url" class="sim-hint">内置服务器未就绪——回到首页稍候再进。</div>
        <iframe
          v-else
          ref="frame"
          :src="url"
          class="sim-frame"
          @load="onFrameLoad"
        ></iframe>
        <div v-if="url && loading" class="sim-hint overlay">物理内核加载中（首次约 2 秒）…</div>
      </div>
      <aside class="sim-side">
        <h3>任务</h3>
        <p class="muted">把 <b class="cube">方块</b> 抓起来放进 <b class="goal">绿色目标圈</b>（9cm 落区即成功）。键鼠遥操作：Q/A 底座、W/S 肩、E/D 肘、R/F 腕、空格夹爪。</p>
        <h3>采集</h3>
        <p class="muted">
          录制以 60Hz 记 21 维观测 + 5 维动作；导出为
          <b>LeRobotDataset v2.1</b> 目录（meta 全套 + 每集回执），
          用 engine 仓脚本转 parquet 后 <code>lerobot-train</code> 直读。
        </p>
        <h3>状态</h3>
        <p class="muted">{{ note || "进场即预载桌面场景；重置换随机种子，脚本策略可全自动演示。" }}</p>
        <p v-if="errMsg" class="err">{{ errMsg }}</p>
        <p v-if="report" class="ok">
          最近导出：{{ report.dir }}
        </p>
        <h3>训练三步</h3>
        <ol class="muted small">
          <li>本页遥操作/跑策略 → 录制 → 导出</li>
          <li><code>python engine/scripts/jsonl_to_lerobot_v21.py &lt;导出目录&gt;</code></li>
          <li><code>lerobot-train --dataset.repo_id=&lt;user&gt;/&lt;ds&gt; --policy.type=act</code></li>
        </ol>
        <h3>确定性说明</h3>
        <p class="muted small">
          本页是 JS 镜像内核（交互优先）；批量采集与逐位回放请用引擎 CLI：
          <code>cargo run -p oriera-agent --example embodied_sim_collect --features rigid3d</code>
          （双跑逐位一致 + 每集 sim_fingerprint 回执）。
        </p>
      </aside>
    </div>
  </div>
</template>

<style scoped>
.sim-root { display: flex; flex-direction: column; height: 100vh; }
.sim-toolbar {
  display: flex; justify-content: space-between; align-items: center;
  padding: 10px 16px; border-bottom: 1px solid var(--n-border-color, #222633); gap: 12px;
}
.sim-title { font-weight: 600; display: flex; align-items: center; gap: 8px; }
.sim-actions { display: flex; gap: 8px; flex-wrap: wrap; }
.sim-main { flex: 1; display: flex; min-height: 0; }
.sim-frame-wrap { flex: 1; position: relative; min-width: 0; }
.sim-frame { width: 100%; height: 100%; border: 0; background: #0b0d12; }
.sim-hint { padding: 24px; color: #8a90a6; font-size: 13px; }
.sim-hint.overlay {
  position: absolute; inset: 0; display: flex; align-items: center; justify-content: center;
  background: rgba(11, 13, 18, 0.75);
}
.sim-side {
  width: 300px; border-left: 1px solid #222633; padding: 14px;
  overflow: auto; font-size: 12px; line-height: 1.7;
}
.sim-side h3 { font-size: 12px; margin: 14px 0 6px; color: #9fb6e8; }
.muted { color: #8a90a6; }
.small { font-size: 11px; }
.cube { color: #ffd479; }
.goal { color: #7be0a0; }
.err { color: #ff8098; }
.ok { color: #7be0a0; word-break: break-all; }
code { background: #1a2030; border-radius: 4px; padding: 1px 5px; }
</style>
