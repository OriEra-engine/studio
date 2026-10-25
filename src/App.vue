<script setup lang="ts">
// 构建期注入（vite define），与 package.json 版本恒一致
const APP_VERSION = __APP_VERSION__;
import { computed, ref, onBeforeUnmount, onMounted } from "vue";
import { useRoute, RouterLink, RouterView } from "vue-router";
import { NConfigProvider, NMessageProvider, NDialogProvider, darkTheme, zhCN, dateZhCN } from "naive-ui";
import type { GlobalThemeOverrides } from "naive-ui";
import CommandPalette from "./components/CommandPalette.vue";
import CopilotPanel from "./components/CopilotPanel.vue";
import { store, refreshStatus } from "./store";

const route = useRoute();
const collapsed = ref(localStorage.getItem("oe.siderCollapsed") === "1");
const palette = ref<InstanceType<typeof CommandPalette>>();

/* ZCode 中性黑：naive 全家与自绘组件同一套灰阶；蓝只保留在主按钮/链接 */
const themeOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: "#4f8ae8",
    primaryColorHover: "#6ba1f2",
    primaryColorPressed: "#3d76cf",
    primaryColorSuppl: "#4f8ae8",
    infoColor: "#4f8ae8",
    infoColorHover: "#6ba1f2",
    infoColorPressed: "#3d76cf",
    infoColorSuppl: "#4f8ae8",
    borderRadius: "8px",
    borderRadiusSmall: "6px",
    cardColor: "#141418",
    modalColor: "#16161b",
    popoverColor: "#1a1a20",
    borderColor: "#30303a",
    dividerColor: "#202027",
    inputColor: "#0a0a0c",
    tableColor: "#141418",
    bodyColor: "#0b0b0e",
  },
};

function toggleSider() {
  collapsed.value = !collapsed.value;
  localStorage.setItem("oe.siderCollapsed", collapsed.value ? "1" : "0");
}

function onGlobalKey(e: KeyboardEvent) {
  const k = e.key.toLowerCase();
  if ((e.ctrlKey || e.metaKey) && k === "j") {
    e.preventDefault();
    window.dispatchEvent(new CustomEvent("oriera:copilot:open"));
  } else if ((e.ctrlKey || e.metaKey) && k === "b") {
    e.preventDefault();
    toggleSider();
  }
}
onMounted(() => {
  window.addEventListener("keydown", onGlobalKey);
  // 全局状态唯一初始化点：之后模型保存/清除经 store 即时传导各页
  refreshStatus();
});
onBeforeUnmount(() => window.removeEventListener("keydown", onGlobalKey));

/* ---- 顶栏状态徽章（模型 / 引擎） ---- */
const provider = computed(() => store.status?.provider);
const providerOn = computed(() => !!store.status?.provider_configured);
const port = computed(() => store.status?.server?.port);

/* ---- 分组导航（图标 stroke=currentColor，随选中态变色） ---- */
interface NavItem { key: string; to: string; label: string; svg: string; activeNames?: string[] }
const NAV: { group: string; items: NavItem[] }[] = [
  {
    group: "创造",
    items: [
      { key: "dashboard", to: "/", label: "仪表盘", activeNames: ["dashboard"], svg: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>' },
      { key: "new-game", to: "/new-game", label: "新建游戏", activeNames: ["new-game"], svg: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>' },
      { key: "demos", to: "/demos", label: "Demo 演示厅", activeNames: ["demos"], svg: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polygon points="6 3 20 12 6 21 6 3"/></svg>' },
      { key: "projects", to: "/projects", label: "我的项目", activeNames: ["projects", "editor"], svg: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z"/></svg>' },
      { key: "film", to: "/film", label: "影游工坊", activeNames: ["film"], svg: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="5" width="20" height="14" rx="2"/><path d="M7 5v14M17 5v14M2 12h20"/></svg>' },
    ],
  },
  {
    group: "资源",
    items: [
      { key: "models", to: "/models", label: "模型中心", activeNames: ["models"], svg: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6"/></svg>' },
      { key: "assets", to: "/assets", label: "AI 素材中心", activeNames: ["assets"], svg: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="18" height="14" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg>' },
    ],
  },
  {
    group: "系统",
    items: [
      { key: "settings", to: "/settings", label: "设置", activeNames: ["settings"], svg: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>' },
    ],
  },
];

const navRender = computed(() =>
  NAV.map(g => ({ ...g, items: g.items.map(it => ({ ...it, active: !!it.activeNames?.includes(route.name as string) })) })),
);

function openCopilot() {
  window.dispatchEvent(new CustomEvent("oriera:copilot:open"));
}
</script>

<template>
  <NConfigProvider :theme="darkTheme" :theme-overrides="themeOverrides" :locale="zhCN" :date-locale="dateZhCN" style="height: 100%">
    <div class="oe-root">
      <!-- 侧栏：分组导航，选中 accent 左条（ZCode 式） -->
      <aside class="oe-sider" :class="{ collapsed }">
        <div class="oe-logo">
          <span class="oe-logo-mark">∞</span>
          <span v-if="!collapsed" class="oe-logo-name">OriEra <em>Studio</em></span>
        </div>
        <nav class="oe-nav">
          <template v-for="g in navRender" :key="g.group">
            <div v-if="!collapsed" class="oe-nav-group">{{ g.group }}</div>
            <RouterLink
              v-for="it in g.items" :key="it.key" :to="it.to" class="oe-nav-item"
              :class="{ active: it.active }" :title="collapsed ? it.label : undefined"
              :style="collapsed ? 'justify-content:center;padding:8px 0' : ''"
            >
              <span v-html="it.svg"></span>
              <span v-if="!collapsed">{{ it.label }}</span>
            </RouterLink>
          </template>
        </nav>
        <div v-if="!collapsed" class="oe-sider-foot">
          世界引擎工作台 v{{ APP_VERSION }}
          <div class="btns">
            <button class="oe-mini-btn" title="命令面板 (Ctrl+K)" @click="palette?.show()">⌘K 命令面板</button>
            <button class="oe-mini-btn" title="奥粒助手 (Ctrl+J)" @click="openCopilot">✦ 奥粒</button>
          </div>
        </div>
        <div v-else class="oe-sider-foot" style="text-align:center">∞</div>
      </aside>

      <!-- 主区：顶栏 + 内容 -->
      <div class="oe-main">
        <header class="oe-topbar">
          <button class="oe-mini-btn" style="flex:none;width:30px;height:26px;padding:0" title="折叠侧栏 (Ctrl+B)" @click="toggleSider">☰</button>
          <span class="oe-topbar-title">{{ (route.meta.title as string) || 'OriEra Studio' }}</span>
          <span class="spacer"></span>
          <RouterLink v-if="port" to="/settings" class="oe-badge" title="内置演示服务器（点击查看设置）">
            <span class="status-dot on"></span><span class="mono">127.0.0.1:{{ port }}</span>
          </RouterLink>
          <RouterLink to="/models" class="oe-badge clickable" :title="providerOn ? '当前 AI provider（点击去模型中心）' : '尚未配置模型（点击去模型中心）'">
            <span class="status-dot" :class="providerOn ? 'on' : ''"></span>
            <span v-if="providerOn" class="mono">{{ provider?.model }}</span>
            <span v-else>未配置模型</span>
          </RouterLink>
          <button class="oe-mini-btn" style="height:26px" title="命令面板 (Ctrl+K)" @click="palette?.show()">⌘K</button>
        </header>
        <div class="oe-content">
          <NMessageProvider>
            <NDialogProvider>
              <RouterView />
            </NDialogProvider>
          </NMessageProvider>
        </div>
      </div>
    </div>
    <CommandPalette ref="palette" />
    <CopilotPanel />
  </NConfigProvider>
</template>

<style scoped>
.oe-root { display: flex; height: 100%; }
.oe-main { flex: 1; min-width: 0; display: flex; flex-direction: column; height: 100%; }
.oe-content { flex: 1; overflow-y: auto; min-height: 0; }
</style>
