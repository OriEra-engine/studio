<script setup lang="ts">
// 预览抽屉（M2）：「完整试玩」= 导出同款播放器模板经 iframe srcdoc 注入
// （WYSIWYG，Rust canvas_preview_html 渲染）；「连播巡检」= 逐镜过片 +
// 画布跟随高亮 + 看片批注（模板不可改，批注走画布 flags）。
import { computed, ref, watch } from "vue";
import { NButton, NInput, NSpace, NTag } from "naive-ui";
import { walkSequence, type RawStory, type ShotRef } from "../../lib/canvas/storywalk";

const props = defineProps<{
  show: boolean;
  projectId: string;
  previewHtml: string;
  story: RawStory | null;
  clips: Record<string, string>;
  vo: Record<string, string>;
  serverBase: string;
  flags: Record<string, string>;
}>();
const emit = defineEmits<{
  (e: "update:show", v: boolean): void;
  (e: "highlight", nodeRef: string | null): void;
  (e: "flag", nodeRef: string, text: string): void;
  (e: "unflag", nodeRef: string): void;
}>();

const mode = ref<"play" | "walk">("walk");
const idx = ref(0);
const stopped = ref(false);
const flagInput = ref("");
const flagTarget = ref<string | null>(null);

const flat = computed<{ node: string; shot: ShotRef }[]>(() => {
  if (!props.story) return [];
  return walkSequence(props.story).flatMap(({ node, shots }) => shots.map((s) => ({ node: node.id, shot: s })));
});
const current = computed(() => flat.value[idx.value] ?? null);

function mediaUrl(src: string) {
  return src.startsWith("http") ? src : `${props.serverBase}/${src.replace(/^\/+/, "")}`;
}
const clipUrl = computed(() => (current.value && props.clips[current.value.shot.id] ? mediaUrl(props.clips[current.value.shot.id]!) : null));

let audio: HTMLAudioElement | null = null;
let ambTimer: ReturnType<typeof setTimeout> | null = null;

function clearTimers() {
  if (audio) { audio.pause(); audio = null; }
  if (ambTimer) { clearTimeout(ambTimer); ambTimer = null; }
}
function playCurrent() {
  clearTimers();
  const cur = current.value;
  if (!cur) return;
  emit("highlight", cur.node);
  const voFile = props.vo[cur.shot.id];
  if (voFile) {
    audio = new Audio(`${props.serverBase}/projects/${props.projectId}/film/vo/${voFile}`);
    void audio.play().catch(() => { /* 自动播放被拦时静音巡检继续 */ });
  }
  if (!clipUrl.value) {
    ambTimer = setTimeout(() => next(), Math.max(1800, (cur.shot.dur || 4) * 700));
  }
}
watch([idx, mode], () => { if (mode.value === "walk" && !stopped.value) playCurrent(); });
watch(() => props.show, (s) => { if (!s) stop(); else { stopped.value = false; idx.value = 0; if (mode.value === "walk") playCurrent(); } });

function next() {
  if (idx.value < flat.value.length - 1) idx.value += 1;
  else stop(true);
}
function prev() {
  if (idx.value > 0) idx.value -= 1;
}
function stop(finished = false) {
  stopped.value = true;
  clearTimers();
  emit("highlight", null);
  if (finished) idx.value = 0;
}
function askFlag() {
  const cur = current.value;
  if (!cur) return;
  flagTarget.value = cur.node;
  flagInput.value = props.flags[cur.node] ?? "";
}
function commitFlag() {
  if (flagTarget.value && flagInput.value.trim()) emit("flag", flagTarget.value, flagInput.value.trim());
  flagTarget.value = null;
}
function switchMode(m: "play" | "walk") {
  stop();
  mode.value = m;
  if (m === "walk") { stopped.value = false; playCurrent(); }
}
</script>

<template>
  <div v-if="show" class="pd-mask" @click.self="emit('update:show', false)">
    <div class="pd">
      <div class="pd-head">
        <NSpace size="small" align="center">
          <NButton size="tiny" :type="mode === 'walk' ? 'primary' : 'default'" @click="switchMode('walk')">🎞 连播巡检</NButton>
          <NButton size="tiny" :type="mode === 'play' ? 'primary' : 'default'" @click="switchMode('play')">🎮 完整试玩（导出同款）</NButton>
          <NTag v-if="mode === 'walk' && flat.length" size="tiny" :bordered="false">{{ idx + 1 }}/{{ flat.length }}</NTag>
        </NSpace>
        <span class="spacer"></span>
        <NButton size="tiny" @click="emit('update:show', false)">关闭</NButton>
      </div>

      <!-- 连播巡检：逐镜过片 + 跟随高亮 + 批注 -->
      <div v-if="mode === 'walk'" class="pd-body">
        <div class="pd-screen">
          <video v-if="current && clipUrl" :key="current.shot.id" class="pd-video" :src="clipUrl!"
            autoplay @ended="next" @error="next" />
          <div v-else class="pd-ambience">
            <p class="mono">{{ current?.shot.id ?? "—" }}</p>
            <p>缺片 · 播放器将以 CSS 氛围兜底</p>
          </div>
          <div v-if="current" class="pd-caption">
            <code class="mono">{{ current.shot.id }}</code>
            <span v-if="current.shot.frame"> · {{ current.shot.frame.slice(0, 60) }}</span>
          </div>
        </div>
        <div class="pd-side">
          <NSpace vertical size="small">
            <NSpace size="small">
              <NButton size="tiny" @click="prev">◀ 上一镜</NButton>
              <NButton size="tiny" :disabled="idx >= flat.length - 1" @click="next">下一镜 ▶</NButton>
              <NButton size="tiny" @click="stop()">停止</NButton>
            </NSpace>
            <NButton size="tiny" type="warning" :disabled="!current" @click="askFlag">🚩 标记待改</NButton>
            <div v-if="current" class="pd-node-line">
              当前节拍：<code class="mono">{{ current.node }}</code>
              <NTag v-if="flags[current.node]" size="tiny" type="warning" :bordered="false">已有批注</NTag>
            </div>
            <div class="pd-flag-list">
              <div class="pd-flag-title">批注（{{ Object.keys(flags).length }}）</div>
              <div v-for="(f, node) in flags" :key="node" class="pd-flag-row">
                <code class="mono">{{ node }}</code>
                <span class="pd-flag-text">{{ f }}</span>
                <NButton size="tiny" quaternary @click="emit('unflag', node)">删</NButton>
              </div>
              <p v-if="!Object.keys(flags).length" class="pd-hint">巡检时点「标记待改」落旗，批注会显示在画布节点上。</p>
            </div>
          </NSpace>
        </div>
      </div>

      <!-- 完整试玩：与导出 zip 同一模板，绝对只读 -->
      <div v-else class="pd-body">
        <iframe class="pd-frame" :srcdoc="previewHtml" sandbox="allow-scripts" title="完整试玩" />
      </div>

      <!-- 批注输入 -->
      <div v-if="flagTarget" class="pd-flag-dialog" @keydown.enter="commitFlag">
        <NInput v-model:value="flagInput" size="small" placeholder="这句台词不行 / 镜头构图重生成…" autofocus />
        <NButton size="small" type="primary" @click="commitFlag">落旗</NButton>
        <NButton size="small" @click="flagTarget = null">取消</NButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.pd-mask { position: fixed; inset: 0; background: rgba(0,0,0,.6); z-index: 90; display: flex; align-items: center; justify-content: center; }
.pd { width: 900px; max-width: 94vw; height: 640px; max-height: 90vh; background: #101014; border: 1px solid #30303a; border-radius: 12px; display: flex; flex-direction: column; overflow: hidden; }
.pd-head { display: flex; align-items: center; padding: 10px 14px; border-bottom: 1px solid #202027; gap: 8px; }
.pd-head .spacer { flex: 1; }
.pd-body { flex: 1; display: flex; min-height: 0; }
.pd-screen { flex: 1; min-width: 0; background: #05070a; display: flex; align-items: center; justify-content: center; position: relative; }
.pd-video { max-width: 100%; max-height: 100%; }
.pd-ambience { color: #3d5666; text-align: center; background: radial-gradient(ellipse at 50% 30%, #16303f 0%, #0a141c 55%, #04080c 100%); width: 100%; height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; }
.pd-caption { position: absolute; left: 12px; bottom: 10px; font-size: 11px; color: #93a7b4; background: rgba(0,0,0,.5); border-radius: 6px; padding: 4px 8px; max-width: 80%; }
.pd-side { width: 260px; flex: none; border-left: 1px solid #202027; padding: 12px; overflow-y: auto; }
.pd-node-line { font-size: 12px; color: #93a7b4; display: flex; gap: 6px; align-items: center; }
.pd-flag-list { margin-top: 8px; border-top: 1px solid #202027; padding-top: 8px; }
.pd-flag-title { font-size: 12px; color: #71717c; margin-bottom: 6px; }
.pd-flag-row { display: flex; align-items: center; gap: 6px; padding: 3px 0; font-size: 12px; }
.pd-flag-text { flex: 1; color: #e8d9ad; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.pd-hint { font-size: 11px; color: #5b6474; line-height: 1.6; }
.pd-frame { flex: 1; border: 0; background: #02050a; }
.pd-flag-dialog { position: absolute; left: 50%; bottom: 26px; transform: translateX(-50%); display: flex; gap: 8px; width: 480px; background: #16161b; border: 1px solid #30303a; border-radius: 10px; padding: 10px; }
.mono { font-family: Consolas, monospace; }
</style>
