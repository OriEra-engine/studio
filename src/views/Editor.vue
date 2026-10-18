<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  NButton, NCard, NInput, NGi, NGrid, NTag, NSpace, NAlert, NSpin,
  NPopconfirm, useMessage,
} from "naive-ui";
import { api, type ProjectDetail } from "../api";

const route = useRoute();
const router = useRouter();
const message = useMessage();
const id = route.params.id as string;

const loading = ref(true);
const gameName = ref("");
const theme = ref("");
const framework = ref("");
const premise = ref("");
const beats = reactive<{ phase: string; title: string; desc: string }[]>([]);
const aiBusy = ref(-1);
const instruction = reactive<Record<number, string>>({});
const saving = ref(false);
const upgBusy = ref(false);
const zipBusy = ref(false);

onMounted(async () => {
  try {
    const d: ProjectDetail = await api.readProject(id);
    gameName.value = String(d.game.name ?? "");
    theme.value = String(d.game.theme ?? "");
    framework.value = d.story.framework ?? "";
    premise.value = d.story.premise ?? "";
    beats.splice(0, beats.length, ...d.story.beats.map(b => ({ ...b })));
  } catch (e) {
    message.error(String(e));
  } finally {
    loading.value = false;
  }
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
      beats: beats.map(b => ({ phase: b.phase, title: b.title, desc: b.desc })),
    });
    message.success("剧情已保存（旧版本自动备份在项目 .history/，保留最近 10 份）");
  } catch (e) {
    message.error(String(e));
  } finally {
    saving.value = false;
  }
}

async function rewrite(i: number) {
  aiBusy.value = i;
  try {
    const beat = await api.regenerateBeat(id, i, instruction[i] ?? "");
    beats[i] = beat;
    message.success(`第 ${i + 1} 拍已重写（已自动落盘）`);
  } catch (e) {
    message.error(String(e));
  } finally {
    aiBusy.value = -1;
  }
}

function addBeat(i: number) {
  beats.splice(i + 1, 0, { phase: beats[i]?.phase ?? "新阶段", title: "新节拍", desc: "描述…" });
}
function delBeat(i: number) {
  if (beats.length <= 1) return message.warning("至少保留一个节拍");
  beats.splice(i, 1);
}
function move(i: number, d: -1 | 1) {
  const j = i + d;
  if (j < 0 || j >= beats.length) return;
  [beats[i], beats[j]] = [beats[j], beats[i]];
}

async function run() {
  try { await api.openProject(id); } catch (e) { message.error(String(e)); }
}
async function upgrade() {
  upgBusy.value = true;
  try {
    const r = await api.upgradeRuntime(id);
    message.success(r || "已升级");
  } catch (e) { message.error(String(e)); } finally { upgBusy.value = false; }
}
async function exportZip() {
  zipBusy.value = true;
  try {
    const p = await api.exportProjectZip(id);
    if (p) message.success(`已导出：${p}（对方解压到自己的 projects 目录即可玩）`);
  } catch (e) { message.error(String(e)); } finally { zipBusy.value = false; }
}
</script>

<template>
  <div class="page">
    <div style="display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 10px">
      <div>
        <h2 class="page-title" style="margin-bottom: 2px">✏️ {{ gameName || '剧情编辑器' }}</h2>
        <p class="page-sub" style="margin-bottom: 0">
          叙事：{{ framework || '三幕式' }} · {{ beats.length }} 拍 · 修改后记得保存，AI 重写会自动落盘
        </p>
      </div>
      <NSpace size="small">
        <NButton size="small" secondary @click="router.push('/projects')">← 项目列表</NButton>
        <NButton size="small" type="primary" ghost @click="run">▶ 运行</NButton>
        <NButton size="small" secondary :loading="upgBusy" @click="upgrade">⬆ 升级运行时</NButton>
        <NButton size="small" secondary :loading="zipBusy" @click="exportZip">📦 导出分享</NButton>
      </NSpace>
    </div>

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
          <NInput v-model:value="b.desc" type="textarea" :rows="2" placeholder="剧情描述" />
          <div class="ai-row">
            <NInput v-model:value="instruction[i]" size="tiny" style="flex: 1" placeholder="AI 重写要求（留空=自由发挥更精彩）" />
            <NButton size="tiny" type="primary" ghost :loading="aiBusy === i" @click="rewrite(i)">✨ 重写这一拍</NButton>
          </div>
        </div>
      </NCard>

      <div style="margin-top: 16px; display: flex; gap: 12px; align-items: center">
        <NButton type="primary" :loading="saving" @click="save">💾 保存剧情</NButton>
        <span style="color: #5a627a; font-size: 12px">保存前旧版自动备份；「运行」窗口按 F5 可刷新生效</span>
      </div>
    </NSpin>
  </div>
</template>

<style scoped>
.beat-editor { background: #161a28; border: 1px solid #232a3d; border-radius: 10px; padding: 10px 12px; margin-bottom: 10px; }
.beat-toolbar { display: flex; gap: 6px; align-items: center; margin-bottom: 8px; }
.beat-no { color: #7c86a3; font-size: 12px; width: 20px; text-align: center; flex: none; }
.ai-row { display: flex; gap: 8px; margin-top: 8px; }
</style>
