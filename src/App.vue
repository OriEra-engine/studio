<script setup lang="ts">
// 构建期注入（vite define），与 package.json 版本恒一致
const APP_VERSION = __APP_VERSION__;
import { h, ref, onBeforeUnmount, onMounted, Component } from "vue";
import { useRoute, RouterLink, RouterView } from "vue-router";
import { NConfigProvider, NLayout, NLayoutSider, NLayoutContent, NMenu, NMessageProvider, NDialogProvider, darkTheme, zhCN, dateZhCN } from "naive-ui";
import type { MenuOption } from "naive-ui";
import CommandPalette from "./components/CommandPalette.vue";
import CopilotPanel from "./components/CopilotPanel.vue";

const route = useRoute();
const collapsed = ref(false);
const palette = ref<InstanceType<typeof CommandPalette>>();

function onGlobalKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "j") {
    e.preventDefault();
    openCopilot();
  }
}
function openCopilot() {
  window.dispatchEvent(new CustomEvent("oriera:copilot:open"));
}
onMounted(() => window.addEventListener("keydown", onGlobalKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onGlobalKey));

function icon(svg: string) {
  return () => h("span", { style: "display:inline-flex;width:18px;height:18px;margin-right:6px;", innerHTML: svg });
}

const menuOptions: MenuOption[] = [
  {
    label: () => h(RouterLink, { to: "/" }, { default: () => "仪表盘" }),
    key: "dashboard",
    icon: icon('<svg viewBox="0 0 24 24" fill="none" stroke="#9fb4d8" stroke-width="2"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>'),
  },
  {
    label: () => h(RouterLink, { to: "/demos" }, { default: () => "Demo 演示厅" }),
    key: "demos",
    icon: icon('<svg viewBox="0 0 24 24" fill="none" stroke="#9fb4d8" stroke-width="2"><polygon points="6 3 20 12 6 21 6 3"/></svg>'),
  },
  {
    label: () => h(RouterLink, { to: "/new-game" }, { default: () => "新建游戏" }),
    key: "new-game",
    icon: icon('<svg viewBox="0 0 24 24" fill="none" stroke="#9fb4d8" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>'),
  },
  {
    label: () => h(RouterLink, { to: "/models" }, { default: () => "模型中心" }),
    key: "models",
    icon: icon('<svg viewBox="0 0 24 24" fill="none" stroke="#9fb4d8" stroke-width="2"><rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6"/></svg>'),
  },
  {
    label: () => h(RouterLink, { to: "/projects" }, { default: () => "我的项目" }),
    key: "projects",
    icon: icon('<svg viewBox="0 0 24 24" fill="none" stroke="#9fb4d8" stroke-width="2"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z"/></svg>'),
  },
  {
    label: () => h(RouterLink, { to: "/settings" }, { default: () => "设置" }),
    key: "settings",
    icon: icon('<svg viewBox="0 0 24 24" fill="none" stroke="#9fb4d8" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>'),
  },
];
</script>

<template>
  <NConfigProvider :theme="darkTheme" :locale="zhCN" :date-locale="dateZhCN" style="height: 100%">
    <NLayout style="height: 100%" has-sider>
      <NLayoutSider
        bordered
        collapse-mode="width"
        :collapsed-width="64"
        :width="200"
        :collapsed="collapsed"
        show-trigger
        @collapse="collapsed = true"
        @expand="collapsed = false"
      >
        <div style="padding: 18px 16px 10px; font-weight: 800; font-size: 16px; white-space: nowrap; overflow: hidden">
          <span v-if="!collapsed">OriEra <span style="color: #6ea8fe">Studio</span></span>
          <span v-else style="color: #6ea8fe">OE</span>
        </div>
        <NMenu :collapsed="collapsed" :collapsed-width="64" :options="menuOptions" :value="route.name as string" />
        <div v-if="!collapsed" style="position: absolute; bottom: 12px; left: 16px; color: #5a627a; font-size: 11px">
          世界引擎工作台 v{{ APP_VERSION }}<br />
          <span style="color: #6ea8fe; cursor: pointer" @click="palette?.show()">Ctrl+K 命令面板</span> ·
          <span style="color: #6ea8fe; cursor: pointer" @click="openCopilot()">Ctrl+J 奥粒</span>
        </div>
      </NLayoutSider>
      <NLayoutContent content-style="height: 100%; overflow-y: auto;">
        <NMessageProvider>
          <NDialogProvider>
            <RouterView />
          </NDialogProvider>
        </NMessageProvider>
      </NLayoutContent>
    </NLayout>
    <CommandPalette ref="palette" />
    <CopilotPanel />
  </NConfigProvider>
</template>
