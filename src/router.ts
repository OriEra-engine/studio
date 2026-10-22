import { createRouter, createWebHistory } from "vue-router";

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/", name: "dashboard", component: () => import("./views/Dashboard.vue"), meta: { title: "仪表盘" } },
    { path: "/demos", name: "demos", component: () => import("./views/Demos.vue"), meta: { title: "Demo 演示厅" } },
    { path: "/new-game", name: "new-game", component: () => import("./views/NewGame.vue"), meta: { title: "新建游戏" } },
    { path: "/models", name: "models", component: () => import("./views/Models.vue"), meta: { title: "模型中心" } },
    { path: "/projects", name: "projects", component: () => import("./views/Projects.vue"), meta: { title: "我的项目" } },
    { path: "/assets", name: "assets", component: () => import("./views/Assets.vue"), meta: { title: "AI 素材中心" } },
    { path: "/editor/:id", name: "editor", component: () => import("./views/Editor.vue"), meta: { title: "剧情编辑器" } },
    { path: "/settings", name: "settings", component: () => import("./views/Settings.vue"), meta: { title: "设置" } },
    { path: "/play/:id", name: "play", component: () => import("./views/Play.vue"), meta: { title: "游玩", chrome: false } },
  ],
});
