// 全局应用状态 —— 单一来源，模型保存/清除后所有页面即时传导。
// 此前每个视图 mount 时各自 getStatus()，向导里配完模型要手点「刷新状态」。
import { reactive } from "vue";
import { api, type AppStatus, type ProviderCfg } from "./api";

export const store = reactive({
  status: null as AppStatus | null,
  loaded: false,
});

export function providerReady(): boolean {
  return !!store.status?.provider_configured;
}

export function providerModel(): string {
  return store.status?.provider?.model ?? "";
}

export async function refreshStatus(): Promise<void> {
  try {
    store.status = await api.getStatus();
  } catch {
    /* 纯浏览器/降级环境：保留现有状态 */
  } finally {
    store.loaded = true;
  }
}

/** 保存并全局生效（后端同时热重载已打开的 demo/游戏窗口）。 */
export async function saveProvider(cfg: ProviderCfg): Promise<void> {
  await api.saveProvider(cfg);
  await refreshStatus();
}

export async function clearProvider(): Promise<void> {
  await api.clearProvider();
  await refreshStatus();
}
