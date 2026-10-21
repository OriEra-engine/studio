// 向导状态跨路由暂存 —— 模块级 reactive，组件卸载不丢。
// 此前向导状态全在组件内：第 3 步跳去模型中心再回来，玩法/剧情/主题全部清零重选。
import { reactive } from "vue";
import type { StoryBeat } from "./types";

export const wizard = reactive({
  step: 1,
  // 第 1 步
  gameplayId: "",
  // 第 2 步
  frameworkId: "pack-default",
  appliedFramework: "pack-default",
  userIdea: "",
  premise: "",
  beats: [] as StoryBeat[],
  // 第 4 步
  theme: "space",
  agentSel: {} as Record<string, boolean>,
  // 第 5 步
  projectName: "",
});

export function resetWizard() {
  wizard.step = 1;
  wizard.gameplayId = "";
  wizard.frameworkId = "pack-default";
  wizard.appliedFramework = "pack-default";
  wizard.userIdea = "";
  wizard.premise = "";
  wizard.beats.splice(0);
  wizard.theme = "space";
  wizard.agentSel = {};
  wizard.projectName = "";
}
