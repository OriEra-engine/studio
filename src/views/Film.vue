<script setup lang="ts">
// 影游工坊 —— P1 建游的影游入口：剧情图编辑 + 一键体检（L9 内核镜像）+ 导出
// kernel = src/lib/filmStory.js（与 engine oriera-agent/story.rs 逐句对照）
import { computed, ref } from "vue";
import { NButton, NCard, NInput, NRadioButton, NRadioGroup, NSelect, NSpace, NTag, useMessage } from "naive-ui";
import { FilmStory } from "../lib/filmStory";
import FilmFlow from "./FilmFlow.vue";

// 双模式：向导（新手默认，五步走完整制作管线）/ 工坊（专家直编）
const mode = ref<"flow" | "shop">("flow");

const message = useMessage();

const BUILTIN = JSON.stringify({
  meta: { title: "末班站台（缩样）", aspect: "9:16" },
  vars: { affinity_lin: 0, trust: 0 },
  nodes: [
    { id: "s001", type: "scene", title: "末班地铁", goto: "c001" },
    { id: "c001", type: "choice", title: "站台", timeout: 10, options: [
      { text: "递伞", set: { affinity_lin: "+1" }, goto: "s002a" },
      { text: "无视", set: { affinity_lin: "-1" }, goto: "s002b" } ] },
    { id: "s002a", type: "scene", title: "道谢", goto: "hub" },
    { id: "s002b", type: "scene", title: "目光", goto: "hub" },
    { id: "hub", type: "hub", routes: [
      { when: "affinity_lin >= 1", goto: "e_good" }, { goto: "e_bad" } ] },
    { id: "e_good", type: "ending", title: "雨夜真相" },
    { id: "e_bad", type: "ending", title: "消失的站台" },
  ],
}, null, 2);

const storyText = ref(BUILTIN);
const findings = ref<{ severity: string; node: string; kind: string; detail?: string }[]>([]);
const checked = ref(false);

const kindOptions = ["scene", "choice", "hub", "ending"].map(k => ({ label: k, value: k }));

function load() { storyText.value = BUILTIN; checked.value = false; findings.value = []; }

function check() {
  let g;
  try { g = FilmStory.parse(storyText.value); }
  catch (e) { findings.value = [{ severity: "error", node: "-", kind: "json", detail: String(e) }]; checked.value = true; return; }
  findings.value = FilmStory.validate(g).map((f: any) => ({
    severity: f.severity, node: f.node, kind: f.kind, detail: f.detail ?? "",
  }));
  checked.value = true;
  if (findings.value.length === 0) message.success("体检全绿 —— 死路/可达/变量/默认臂全部通过");
}

function exportJson() {
  const blob = new Blob([storyText.value], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = "story.json";
  a.click();
  URL.revokeObjectURL(a.href);
}

function importJson(ev: Event) {
  const f = (ev.target as HTMLInputElement).files?.[0];
  if (!f) return;
  const r = new FileReader();
  r.onload = () => { storyText.value = String(r.result); checked.value = false; };
  r.readAsText(f);
}

// 氛围导演预览（vars 试算）
const tryTrust = ref(0);
const tryAff = ref(0);
const atmosphere = computed(() =>
  FilmStory.atmosphere(new Map([["trust", tryTrust.value], ["affinity_lin", tryAff.value]])));

const kindTag: Record<string, "info" | "success" | "warning" | "error"> = {
  scene: "info", choice: "success", hub: "warning", ending: "error",
};
const nodes = computed(() => {
  try { return FilmStory.parse(storyText.value).nodes; } catch { return []; }
});
</script>

<template>
  <div class="page">
    <h2 class="page-title">🎬 影游工坊</h2>
    <div style="margin-bottom: 12px">
      <NRadioGroup v-model:value="mode" size="small">
        <NRadioButton value="flow">🧭 工作流（推荐）</NRadioButton>
        <NRadioButton value="shop">🛠 工坊（专家）</NRadioButton>
      </NRadioGroup>
    </div>
    <FilmFlow v-if="mode === 'flow'" />
    <p v-if="mode === 'shop'" class="page-sub">剧情图单源真源：编辑 story.json → 一键体检（死路/可达/变量/默认臂）→ 导出给量产管线。体检内核与引擎 L9 逐句同源。</p>

    <template v-if="mode === 'shop'">
    <NCard size="small" style="margin-bottom: 16px">
      <template #header>剧本（story.json）</template>
      <template #header-extra>
        <NSpace size="small">
          <NButton size="small" @click="load">载入末班站台模板</NButton>
          <label class="import-btn">
            导入 JSON
            <input type="file" accept=".json" style="display:none" @change="importJson" />
          </label>
          <NButton size="small" @click="exportJson">导出</NButton>
          <NButton size="small" type="primary" @click="check">🩺 一键体检</NButton>
        </NSpace>
      </template>
      <NInput v-model:value="storyText" type="textarea" :rows="14"
        style="font-family: Consolas, monospace; font-size: 12px" />
    </NCard>

    <NCard v-if="checked" size="small" style="margin-bottom: 16px">
      <template #header>体检报告</template>
      <div v-if="findings.length === 0" style="color: #6fd3a0">✅ 全绿 —— 可以进入视听量产</div>
      <div v-else>
        <div v-for="(f, i) in findings" :key="i" style="display:flex; gap:10px; font-size:13px; padding:3px 0">
          <NTag size="tiny" :type="f.severity === 'error' ? 'error' : 'warning'" round>{{ f.severity }}</NTag>
          <code style="color:#ff9a3c">{{ f.kind }}</code>
          <code>{{ f.node }}</code>
          <span style="color:#71717c">{{ f.detail }}</span>
        </div>
      </div>
    </NCard>

    <NCard v-if="nodes.length" size="small" style="margin-bottom: 16px">
      <template #header>节点总览（{{ nodes.length }}）</template>
      <div style="display:flex; flex-wrap:wrap; gap:8px">
        <NTag v-for="n in nodes" :key="n.id" size="small" :type="kindTag[n.kind] ?? 'default'" round>
          {{ n.id }} · {{ n.title || n.kind }}
        </NTag>
      </div>
      <p style="font-size:12px; color:#71717c; margin:10px 0 0">
        scene 蓝 / choice 绿 / hub 橙 / ending 红 —— 分支经济学：choice 多而汇聚快，素材量线性。
      </p>
    </NCard>

    <NCard size="small">
      <template #header>氛围导演预览（vars → 氛围指令）</template>
      <NSpace align="center">
        <span style="font-size:12px;color:#71717c">trust</span>
        <NSelect v-model:value="tryTrust" size="small" style="width:90px"
          :options="[-2,-1,0,1,2].map(v => ({ label: String(v), value: v }))" />
        <span style="font-size:12px;color:#71717c">affinity</span>
        <NSelect v-model:value="tryAff" size="small" style="width:90px"
          :options="[-2,-1,0,1,2].map(v => ({ label: String(v), value: v }))" />
        <NTag type="info" round>{{ atmosphere }}</NTag>
        <span style="font-size:12px;color:#71717c">播放器把指令渲染为雨量/警灯/张力峰值</span>
      </NSpace>
    </NCard>
    </template>
  </div>
</template>

<style scoped>
.import-btn {
  font-size: 12px; padding: 0 10px; line-height: 28px; cursor: pointer;
  border: 1px solid rgba(255,255,255,.2); border-radius: 3px;
}
.import-btn:hover { border-color: #ff9a3c; }
</style>
