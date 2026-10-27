<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import { onBeforeRouteLeave, useRoute, useRouter } from "vue-router";
import {
  NButton, NCard, NInput, NGi, NGrid, NTag, NSpace, NAlert, NSpin,
  NPopconfirm, NSelect, useMessage,
} from "naive-ui";
import { humanizeError } from "../utils/errors";
import { api, type ProjectDetail } from "../api";
import { store } from "../store";
import { charDiff, type Seg } from "../utils/diff";
import type { BeatChoice, StoryBeat } from "../types";

const route = useRoute();
const router = useRouter();
const message = useMessage();
const id = route.params.id as string;

const loading = ref(true);
const gameName = ref("");
const theme = ref("");
const framework = ref("");
const premise = ref("");
const beats = reactive<{ phase: string; title: string; desc: string; choices?: BeatChoice[] | null;
  countdown?: number | null; video?: string | null; ending?: boolean | null;
  settle?: Record<string, string> | null; ending_titles?: Record<string, string> | null;
  ending_lines?: Record<string, string> | null }[]>([]);
const aiBusy = ref(-1);
const instruction = reactive<Record<number, string>>({});
const saving = ref(false);
const upgBusy = ref(false);
const zipBusy = ref(false);

// ---- AI 重写 diff 确认制：预览不落盘，采纳才写盘 ----
const diffIndex = ref(-1); // 正在对比的节拍下标
const diffOld = ref<StoryBeat | null>(null);
const diffNew = ref<StoryBeat | null>(null);
const diffDesc = computed<{ old: Seg[]; new: Seg[] } | null>(() => {
  if (diffIndex.value < 0 || !diffOld.value || !diffNew.value) return null;
  return charDiff(diffOld.value.desc, diffNew.value.desc);
});
const diffTitle = computed(() => {
  if (!diffOld.value || !diffNew.value) return null;
  return diffOld.value.title !== diffNew.value.title ? charDiff(diffOld.value.title, diffNew.value.title) : null;
});
const applying = ref(false);

onMounted(async () => {
  try {
    const d: ProjectDetail = await api.readProject(id);
    gameName.value = String(d.game.name ?? "");
    theme.value = String(d.game.theme ?? "");
    framework.value = d.story.framework ?? "";
    premise.value = d.story.premise ?? "";
    beats.splice(0, beats.length, ...d.story.beats.map(b => ({ ...b })));
  } catch (e) {
    message.error(humanizeError(e));
  } finally {
    loading.value = false;
  }
  baseline = snapshot();
  window.addEventListener("keydown", onKey);
  window.addEventListener("beforeunload", onBeforeUnload);
});
function onBeforeUnload(e: BeforeUnloadEvent) {
  if (dirty.value) {
    e.preventDefault();
    e.returnValue = "";
  }
}
onUnmounted(() => {
  window.removeEventListener("keydown", onKey);
  window.removeEventListener("beforeunload", onBeforeUnload);
});

// ---- 2026-10-26 制作人测评 P0：改一个字必须鼠标点保存、误删无法回退、
// ---- 切页面静默丢弃——脏检查 + Ctrl+S + 离开守卫 + 结构操作撤销栈。
const dirty = ref(false);
let baseline = "";
function snapshot(): string {
  return JSON.stringify({ premise: premise.value, beats });
}
watch([premise, beats], () => {
  if (!loading.value) dirty.value = snapshot() !== baseline;
}, { deep: true });

// 撤销栈：结构操作（增/删/移/采纳 diff）前压整帧快照，容量 50
// （undoDepth 是响应式镜像——普通数组的变化模板读不到）
const undoStack: string[] = [];
const undoDepth = ref(0);
function pushUndo() {
  undoStack.push(snapshot());
  if (undoStack.length > 50) undoStack.shift();
  undoDepth.value = undoStack.length;
}
function undo() {
  const prev = undoStack.pop();
  undoDepth.value = undoStack.length;
  if (prev === undefined) return message.info("没有可撤销的结构操作");
  try {
    const s = JSON.parse(prev) as { premise: string; beats: typeof beats };
    premise.value = s.premise;
    beats.splice(0, beats.length, ...s.beats);
    dirty.value = snapshot() !== baseline;
    message.success("已撤销（记得保存）");
  } catch { message.error("撤销失败：快照损坏"); }
}
function onKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
    e.preventDefault();
    if (dirty.value && !saving.value) void save();
  }
}
onBeforeRouteLeave(() => {
  if (!dirty.value) return true;
  // WebView2 原生 confirm：同步语义正合路由守卫
  return window.confirm("有未保存的修改——离开会丢失。确定离开吗？（先 Ctrl+S 保存可安全离开）");
});

async function save() {
  if (!premise.value.trim() || beats.length === 0) {
    message.warning("前提与至少一个节拍不能为空");
    return;
  }
  saving.value = true;
  try {
    await api.updateStory(id, {
      framework: framework.value,
      premise: premise.value,
      beats: beats.map(b => ({
        phase: b.phase, title: b.title, desc: b.desc,
        // #120 影游腿扩展字段：编辑器暂不提供编辑 UI，但保存时必须原样保留（保存 ≠ 降级）
        ...(b.countdown != null ? { countdown: b.countdown } : {}),
        ...(b.video ? { video: b.video } : {}),
        ...(b.ending ? { ending: b.ending } : {}),
        ...(b.settle ? { settle: b.settle } : {}),
        ...(b.ending_titles ? { ending_titles: b.ending_titles } : {}),
        ...(b.ending_lines ? { ending_lines: b.ending_lines } : {}),
        ...(b.choices?.length ? { choices: b.choices.map(c => ({
          label: c.label,
          ...(c.next !== null && c.next !== undefined ? { next: c.next } : {}),
          ...(c.effect ? { effect: c.effect } : {}),
        })) } : {}),
      })),
    });
    message.success("剧情已保存，运行窗口已自动刷新（旧版本备份在 .history/，保留 10 份）");
    baseline = snapshot();
    dirty.value = false;
    undoStack.length = 0; // 保存=新基线，撤销栈清零（撤销只管未保存的结构改动）
    undoDepth.value = 0;
    refreshPreview();
  } catch (e) {
    message.error(humanizeError(e));
  } finally {
    saving.value = false;
  }
}

async function rewrite(i: number) {
  aiBusy.value = i;
  try {
    const pv = await api.previewRegenerateBeat(id, i, instruction[i] ?? "");
    diffIndex.value = i;
    diffOld.value = pv.old;
    diffNew.value = pv.new;
  } catch (e) {
    message.error(humanizeError(e));
  } finally {
    aiBusy.value = -1;
  }
}

async function applyDiff() {
  if (diffIndex.value < 0 || !diffNew.value) return;
  pushUndo();
  applying.value = true;
  try {
    await api.applyBeat(id, diffIndex.value, diffNew.value);
    beats[diffIndex.value] = { ...diffNew.value };
    message.success(`第 ${diffIndex.value + 1} 拍已采纳，运行窗口已自动刷新`);
    refreshPreview();
    diffIndex.value = -1;
    diffOld.value = diffNew.value = null;
  } catch (e) {
    message.error(humanizeError(e));
  } finally {
    applying.value = false;
  }
}
function discardDiff() {
  diffIndex.value = -1;
  diffOld.value = diffNew.value = null;
}

function addBeat(i: number) {
  pushUndo();
  beats.splice(i + 1, 0, { phase: beats[i]?.phase ?? "新阶段", title: "新节拍", desc: "描述…" });
}

/* ---- 分支选项(v0.3 叙事分支) ---- */
function addChoice(i: number) {
  const b = beats[i];
  if (!b.choices) b.choices = [];
  if (b.choices.length >= 3) return message.warning("一拍最多 3 个选项(别把玩家绕晕)");
  pushUndo();
  b.choices.push({ label: "新选项", next: null, effect: null });
}
function delChoice(i: number, ci: number) {
  pushUndo();
  beats[i].choices?.splice(ci, 1);
  if (beats[i].choices && beats[i].choices.length === 0) beats[i].choices = null;
}
const nextOptions = (i: number) => {
  const opts = [{ label: `顺延(第 ${i + 2} 拍)`, value: -1 }];
  beats.forEach((b, j) => {
    if (j !== i) opts.push({ label: `第 ${j + 1} 拍 · ${b.title}`, value: j });
  });
  opts.push({ label: "结局(走完)", value: beats.length });
  return opts;
};
function delBeat(i: number) {
  if (beats.length <= 1) return message.warning("至少保留一个节拍");
  pushUndo();
  beats.splice(i, 1);
}
function move(i: number, d: -1 | 1) {
  const j = i + d;
  if (j < 0 || j >= beats.length) return;
  pushUndo();
  [beats[i], beats[j]] = [beats[j], beats[i]];
}

async function run() {
  try { await api.openProject(id); } catch (e) { message.error(humanizeError(e)); }
}
async function upgrade() {
  upgBusy.value = true;
  try {
    const r = await api.upgradeRuntime(id);
    message.success(r || "已升级");
  } catch (e) { message.error(humanizeError(e)); } finally { upgBusy.value = false; }
}
async function exportZip() {
  zipBusy.value = true;
  try {
    const p = await api.exportProjectZip(id);
    if (p) message.success(`已导出：${p}（对方解压到自己的 projects 目录即可玩）`);
  } catch (e) { message.error(humanizeError(e)); } finally { zipBusy.value = false; }
}

/* ---- Godot 式工作台：右侧实时预览 ---- */
const previewKey = ref(0);
const previewUrl = computed(() => {
  const port = store.status?.server?.port;
  return port ? `http://127.0.0.1:${port}/projects/${id}/` : "";
});
function refreshPreview() { previewKey.value++; }

const htmlBusy = ref(false);
async function exportHtml() {
  htmlBusy.value = true;
  try {
    const p = await api.exportProjectSinglefile(id);
    if (p) message.success(`已导出双击即玩 HTML：${p}（引擎已内联，发给谁都能直接玩）`);
  } catch (e) { message.error(humanizeError(e)); } finally { htmlBusy.value = false; }
}
</script>

<template>
  <div class="page">
    <div style="display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 10px">
      <div>
        <h2 class="page-title" style="margin-bottom: 2px">✏️ {{ gameName || '剧情编辑器' }}</h2>
        <p class="page-sub" style="margin-bottom: 0">
          叙事：{{ framework || '三幕式' }} · {{ beats.length }} 拍 · 修改后记得保存；AI 重写先对比再采纳
        </p>
      </div>
      <NSpace size="small">
        <NButton size="small" secondary @click="router.push('/projects')">← 项目列表</NButton>
        <NButton size="small" type="primary" ghost @click="run">▶ 运行</NButton>
        <NButton size="small" secondary :loading="upgBusy" @click="upgrade">⬆ 升级运行时</NButton>
        <NButton size="small" secondary :loading="zipBusy" @click="exportZip">📦 导出分享</NButton>
        <NButton size="small" secondary :loading="htmlBusy" @click="exportHtml">🌐 双击即玩 HTML</NButton>
      </NSpace>
    </div>

    <div class="wb">
      <div class="wb-left">
        <NSpin :show="loading">
          <NCard title="世界观前提" size="small" style="margin-top: 14px">
        <NInput v-model:value="premise" type="textarea" :rows="2" placeholder="一句话世界观前提" />
      </NCard>

      <NCard :title="`剧情节拍（${beats.length}）`" size="small" style="margin-top: 14px">
        <div v-for="(b, i) in beats" :key="i" class="beat-editor">
          <div class="beat-toolbar">
            <span class="beat-no">{{ i + 1 }}</span>
            <NInput v-model:value="b.phase" size="tiny" style="width: 110px" placeholder="阶段" />
            <NInput v-model:value="b.title" size="tiny" style="flex: 1" placeholder="节拍标题" />
            <NButton size="tiny" quaternary @click="move(i, -1)" :disabled="i === 0">↑</NButton>
            <NButton size="tiny" quaternary @click="move(i, 1)" :disabled="i === beats.length - 1">↓</NButton>
            <NButton size="tiny" quaternary @click="addBeat(i)">＋</NButton>
            <NPopconfirm @positive-click="delBeat(i)">
              <template #trigger><NButton size="tiny" quaternary type="error">✕</NButton></template>
              删除第 {{ i + 1 }} 拍？
            </NPopconfirm>
          </div>
          <NInput v-if="diffIndex !== i" v-model:value="b.desc" type="textarea" :rows="2" placeholder="剧情描述" />
          <!-- AI 重写对比：左侧原版（划除），右侧新版（新增高亮）；采纳才落盘 -->
          <div v-else class="diff-panel">
            <div class="diff-cols">
              <div class="diff-side">
                <div class="diff-cap">原版（将被替换）</div>
                <p class="diff-text">
                  <template v-for="(s, si) in diffDesc?.old" :key="si"><del v-if="s.kind === 'del'" class="d-del">{{ s.text }}</del><span v-else>{{ s.text }}</span></template>
                </p>
              </div>
              <div class="diff-side">
                <div class="diff-cap diff-cap-new">AI 新版</div>
                <p class="diff-text">
                  <template v-for="(s, si) in diffDesc?.new" :key="si"><ins v-if="s.kind === 'add'" class="d-add">{{ s.text }}</ins><span v-else>{{ s.text }}</span></template>
                </p>
              </div>
            </div>
            <p v-if="diffTitle" style="margin: 8px 0 0; font-size: 12.5px">
              标题：<del class="d-del">{{ diffOld?.title }}</del> → <ins class="d-add">{{ diffNew?.title }}</ins>
            </p>
            <NSpace size="small" style="margin-top: 10px">
              <NButton size="small" type="primary" :loading="applying" @click="applyDiff">✅ 采纳新版</NButton>
              <NButton size="small" quaternary @click="discardDiff">✕ 放弃</NButton>
              <NButton size="small" secondary :loading="aiBusy === i" @click="rewrite(i)">🔄 再试一次</NButton>
            </NSpace>
          </div>
          <div v-if="b.choices?.length" class="choice-list">
            <div v-for="(c, ci) in b.choices" :key="ci" class="choice-row">
              <NTag size="small" type="info" :bordered="false">分支</NTag>
              <NInput v-model:value="c.label" size="tiny" style="width: 150px" placeholder="选项(≤12字)" />
              <NSelect v-model:value="c.next" size="tiny" style="width: 170px" :options="nextOptions(i)" placeholder="跳转到" />
              <NInput v-model:value="c.effect" size="tiny" style="width: 130px" placeholder="资源±数量(可空)" />
              <NButton size="tiny" quaternary type="error" @click="delChoice(i, ci)">✕</NButton>
            </div>
          </div>
          <div class="ai-row" v-if="diffIndex !== i">
            <NButton size="tiny" dashed @click="addChoice(i)">⑂ 加分支选项</NButton>
            <NInput v-model:value="instruction[i]" size="tiny" style="flex: 1" placeholder="AI 重写要求（留空=自由发挥更精彩）" />
            <NButton size="tiny" type="primary" ghost :loading="aiBusy === i" @click="rewrite(i)">✨ 重写这一拍</NButton>
          </div>
        </div>
      </NCard>

      <div style="margin-top: 16px; display: flex; gap: 12px; align-items: center">
        <NButton type="primary" :loading="saving" @click="save">💾 保存剧情（Ctrl+S）</NButton>
        <NButton size="small" secondary :disabled="undoDepth === 0" @click="undo">↩ 撤销结构操作</NButton>
        <span v-if="dirty" style="color: #ff9a3c; font-size: 12px" role="status">● 有未保存修改</span>
        <span style="color: #71717c; font-size: 12px">保存前旧版自动备份；保存后运行窗口自动刷新出新剧情</span>
      </div>
        </NSpin>
      </div>
      <div class="wb-right">
        <div class="wb-bar">
          <span class="wb-dot"></span>
          <b>实时预览</b>
          <span class="wb-tip">保存 / 采纳重写后自动刷新</span>
          <span style="flex: 1"></span>
          <NButton size="tiny" quaternary @click="refreshPreview">⟳ 刷新</NButton>
          <NButton size="tiny" quaternary @click="run">↗ 浏览器打开</NButton>
        </div>
        <iframe
          v-if="previewUrl"
          :key="previewKey"
          :src="previewUrl"
          class="wb-frame"
          title="游戏实时预览"
        ></iframe>
        <div v-else class="wb-empty">内置服务未就绪，稍候自动可用。</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* ---- Godot 式工作台分屏 ---- */
.wb { display: flex; gap: 14px; margin-top: 14px; align-items: stretch; }
.wb-left { flex: 1 1 480px; min-width: 0; max-width: 620px; }
.wb-right { flex: 1.15 1 420px; min-width: 340px; display: flex; flex-direction: column;
  background: var(--bg-deep, #0e0e12); border: 1px solid var(--line, #202027); border-radius: 12px; overflow: hidden; }
.wb-bar { display: flex; align-items: center; gap: 8px; padding: 8px 12px;
  background: #141419; border-bottom: 1px solid var(--line, #202027); font-size: 12.5px; color: #a8a8b3; }
.wb-dot { width: 8px; height: 8px; border-radius: 50%; background: #35d07f; box-shadow: 0 0 8px #35d07f; }
.wb-tip { color: #55555f; font-size: 11px; }
.wb-frame { flex: 1; width: 100%; border: 0; background: #0a0a0c; }
.wb-empty { flex: 1; display: flex; align-items: center; justify-content: center; color: #55555f; font-size: 13px; }
@media (max-width: 1100px) {
  .wb { flex-direction: column; }
  .wb-left { max-width: none; }
  .wb-right { min-height: 62vh; }
}
.beat-editor { background: #141418; border: 1px solid #202027; border-radius: 10px; padding: 10px 12px; margin-bottom: 10px; }
.beat-toolbar { display: flex; gap: 6px; align-items: center; margin-bottom: 8px; }
.beat-no { color: #6f6f7a; font-size: 12px; width: 20px; text-align: center; flex: none; }
.ai-row { display: flex; gap: 8px; margin-top: 8px; }
.choice-list { display: flex; flex-direction: column; gap: 6px; margin-top: 8px; padding: 8px; background: #141419; border: 1px dashed #30303a; border-radius: 8px; }
.choice-row { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
.diff-panel { background: #0e0e12; border: 1px solid #30303a; border-radius: 8px; padding: 10px 12px; margin-top: 2px; }
.diff-cols { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
@media (max-width: 720px) { .diff-cols { grid-template-columns: 1fr; } }
.diff-cap { font-size: 11px; color: #6f6f7a; margin-bottom: 5px; }
.diff-cap-new { color: #35d07f; }
.diff-text { font-size: 12.5px; line-height: 1.75; color: #cdd6ec; margin: 0; white-space: pre-wrap; word-break: break-word; }
.d-del { background: #3d1d24; color: #ff9aa8; text-decoration: line-through; border-radius: 3px; padding: 0 1px; }
.d-add { background: #143726; color: #7ee2a8; text-decoration: none; border-radius: 3px; padding: 0 1px; }
</style>
