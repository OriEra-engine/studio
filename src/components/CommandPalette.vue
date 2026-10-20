<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";

/**
 * Ctrl+K 命令面板 —— ZCode 式键盘直达:导航、动作、召唤奥粒。
 */
const router = useRouter();
const route = useRoute();

interface Action {
  id: string;
  title: string;
  hint: string;
  keys?: string;
  keywords: string;
  run: () => void;
}

const open = ref(false);
const query = ref("");
const active = ref(0);
const inputEl = ref<HTMLInputElement>();

const actions: Action[] = [
  { id: "nav-dash", title: "前往 · 仪表盘", hint: "总览与快速入口", keywords: "dashboard home 首页", run: () => router.push("/") },
  { id: "nav-new", title: "前往 · 新建游戏", hint: "五步向导,三分钟出成品", keywords: "new game 新建 创建 wizard 向导", run: () => router.push("/new-game") },
  { id: "nav-models", title: "前往 · 模型中心", hint: "云端 / Ollama 双轨配置", keywords: "models llm 模型 key api ollama", run: () => router.push("/models") },
  { id: "nav-projects", title: "前往 · 我的项目", hint: "运行 / 编辑 / 备份", keywords: "projects 项目 存档", run: () => router.push("/projects") },
  { id: "nav-demos", title: "前往 · Demo 演示厅", hint: "引擎 11 个示例", keywords: "demos 示例 demo 演示", run: () => router.push("/demos") },
  { id: "nav-settings", title: "前往 · 设置", hint: "反馈直达引擎仓库", keywords: "settings 设置 feedback 反馈", run: () => router.push("/settings") },
  {
    id: "copilot-open",
    title: "召唤奥粒(工作台助手)",
    hint: "随时问我怎么做出游戏",
    keys: "Ctrl+J",
    keywords: "copilot 奥粒 助手 agent help 帮助",
    run: () => window.dispatchEvent(new CustomEvent("oriera:copilot:open")),
  },
  {
    id: "copilot-reset",
    title: "清空奥粒的记忆",
    hint: "删除本地对话历史",
    keywords: "copilot reset 清空 记忆 history 隐私",
    run: () => window.dispatchEvent(new CustomEvent("oriera:copilot:reset")),
  },
];

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return actions;
  return actions.filter((a) => (a.title + " " + a.hint + " " + a.keywords).toLowerCase().includes(q));
});

watch(filtered, (list) => {
  active.value = list.length ? Math.min(active.value, list.length - 1) : 0;
});
watch(query, () => {
  active.value = 0;
});

function show() {
  open.value = true;
  query.value = "";
  active.value = 0;
  nextTick(() => inputEl.value?.focus());
}
function hide() {
  open.value = false;
}
function run(a: Action) {
  hide();
  a.run();
}
function move(d: number) {
  const n = filtered.value.length;
  if (!n) return;
  active.value = (active.value + d + n) % n;
}
function onKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
    e.preventDefault();
    open.value ? hide() : show();
    return;
  }
  if (!open.value) return;
  if (e.key === "Escape") { hide(); }
  else if (e.key === "ArrowDown") { e.preventDefault(); move(1); }
  else if (e.key === "ArrowUp") { e.preventDefault(); move(-1); }
  else if (e.key === "Enter") {
    e.preventDefault();
    const a = filtered.value[active.value];
    if (a) run(a);
  }
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));

defineExpose({ show, hide });
</script>

<template>
  <Transition name="cp-fade">
    <div v-if="open" class="cp-backdrop" @click.self="hide">
      <div class="cp-box" role="dialog" aria-label="命令面板">
        <div class="cp-inputrow">
          <span class="cp-glyph">⌘</span>
          <input
            ref="inputEl"
            v-model="query"
            placeholder="输入命令或页面名…(↑↓ 选择,Enter 执行,Esc 关闭)"
            @keydown.enter.prevent="filtered[active] && run(filtered[active])"
            @keydown.arrow-down.prevent="move(1)"
            @keydown.arrow-up.prevent="move(-1)"
            @keydown.esc.prevent="hide"
          />
          <span class="cp-kbd">Ctrl+K</span>
        </div>
        <div v-if="!filtered.length" class="cp-empty">没有匹配的命令 —— 试试「模型」「新建」「奥粒」</div>
        <ul class="cp-list">
          <li
            v-for="(a, i) in filtered"
            :key="a.id"
            :class="{ active: i === active, current: a.id === 'nav-' + String(route.name) }"
            @mouseenter="active = i"
            @click="run(a)"
          >
            <span class="cp-title">{{ a.title }}</span>
            <span class="cp-hint">{{ a.hint }}</span>
            <span v-if="a.keys" class="cp-keys">{{ a.keys }}</span>
            <span v-else-if="a.id === 'nav-' + String(route.name)" class="cp-keys cur">当前页</span>
          </li>
        </ul>
        <div class="cp-foot">OriEra Studio · {{ filtered.length }} 个命令 · Ctrl+J 召唤奥粒</div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.cp-backdrop {
  position: fixed; inset: 0; z-index: 120; background: rgba(6, 8, 14, 0.55);
  display: flex; justify-content: center; align-items: flex-start; padding-top: 12vh;
}
.cp-fade-enter-active, .cp-fade-leave-active { transition: opacity 0.14s ease; }
.cp-fade-enter-from, .cp-fade-leave-to { opacity: 0; }
.cp-box {
  width: 560px; max-width: calc(100vw - 32px); background: #161b28; border: 1px solid #2c3959;
  border-radius: 12px; overflow: hidden; box-shadow: 0 18px 60px rgba(0, 0, 0, 0.6);
}
.cp-inputrow { display: flex; align-items: center; gap: 10px; padding: 12px 14px; border-bottom: 1px solid #232d45; }
.cp-glyph { color: #6ea8fe; font-size: 15px; }
.cp-inputrow input {
  flex: 1; background: none; border: none; outline: none; color: #e6e8ef; font-size: 14px; font-family: inherit;
}
.cp-inputrow input::placeholder { color: #5a627a; }
.cp-kbd { font-size: 10.5px; color: #8a90a6; border: 1px solid #2c3959; border-radius: 5px; padding: 1px 6px; }
.cp-empty { padding: 22px 16px; color: #8a90a6; font-size: 12.5px; text-align: center; }
.cp-list { list-style: none; margin: 0; padding: 6px; max-height: 320px; overflow-y: auto; }
.cp-list li {
  display: flex; align-items: center; gap: 10px; padding: 9px 12px; border-radius: 8px;
  cursor: pointer; font-size: 13px;
}
.cp-list li.active { background: #223055; }
.cp-list li.active .cp-title { color: #cfe4ff; }
.cp-title { color: #dbe2f2; white-space: nowrap; }
.cp-hint { flex: 1; color: #6b7590; font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.cp-keys { font-size: 10.5px; color: #8a90a6; border: 1px solid #2c3959; border-radius: 5px; padding: 1px 6px; }
.cp-keys.cur { color: #3fd68f; border-color: #215437; }
.cp-foot { padding: 8px 14px; border-top: 1px solid #232d45; color: #5a627a; font-size: 11px; }
</style>
