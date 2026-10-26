<script setup lang="ts">
// 构建期注入（vite define），与 package.json 版本恒一致
const APP_VERSION = __APP_VERSION__;
import { onMounted, ref } from "vue";
import { NCard, NSpace, NButton, NTag, NAlert, NModal, useMessage, NSwitch, NInput, NRadioGroup, NRadioButton } from "naive-ui";
import { api, type ServerInfo, type FeedbackResult } from "../api";
import { invoke } from "@tauri-apps/api/core";

const message = useMessage();
const info = ref<ServerInfo | null>(null);
const showDev = ref(false);
const petAuto = ref(localStorage.getItem("pet.auto") === "1");
function onPetAuto(v: boolean) {
  localStorage.setItem("pet.auto", v ? "1" : "0");
  const ch = localStorage.getItem("pet.character") === "era" ? "era" : "ori";
  if (v) invoke("open_pet", { character: ch }).catch(() => {});
  else invoke("pet_close").catch(() => {});
}
const developerMode = ref(false);

// ---- 反馈链路 ----
const fbKind = ref("bug");
const fbTitle = ref("");
const fbBody = ref("");
const fbSending = ref(false);
const fbResult = ref<FeedbackResult | null>(null);
const fbHasPat = ref(false);
const fbPat = ref("");
const fbShowPat = ref(false);

async function refreshPat() {
  try { fbHasPat.value = await api.hasFeedbackPat(); } catch { /* ignore */ }
}

async function submitFeedback() {
  if (!fbTitle.value.trim()) { message.warning("请填写标题"); return; }
  fbSending.value = true;
  fbResult.value = null;
  try {
    const r = await api.submitFeedback(fbKind.value, fbTitle.value, fbBody.value);
    fbResult.value = r;
    if (r.mode === "direct") {
      message.success("反馈已提交到引擎仓库 ✓");
      fbTitle.value = ""; fbBody.value = "";
    } else {
      message.info("已生成反馈存档，请在打开的页面点击 Submit 发布");
      try { window.open(r.url, "_blank"); } catch { /* anchor fallback below */ }
    }
  } catch (e) {
    message.error(String(e));
  } finally {
    fbSending.value = false;
  }
}

async function savePat() {
  try {
    await api.saveFeedbackPat(fbPat.value.trim());
    fbPat.value = "";
    await refreshPat();
    message.success("已保存：之后反馈将直提引擎仓库（无需浏览器）");
  } catch (e) { message.error(String(e)); }
}

async function clearPat() {
  try {
    await api.clearFeedbackPat();
    await refreshPat();
    message.success("已清除直提通道，恢复浏览器提交模式");
  } catch (e) { message.error(String(e)); }
}

onMounted(async () => {
  const status = await api.getStatus();
  info.value = status.server;
  try {
    const s = await api.getSettings();
    developerMode.value = !!(s as unknown as { developer_mode?: boolean }).developer_mode;
  } catch { /* ignore */ }
  refreshPat();
});

async function toggleDev(v: boolean) {
  developerMode.value = v;
  try {
    await api.setDeveloperMode(v);
    message.success(v ? "开发者模式已开启：之后打开的 demo/游戏窗口会自动带 DevTools" : "开发者模式已关闭");
  } catch (e) {
    developerMode.value = !v;
    message.error(String(e));
  }
}

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.success("已复制");
  } catch {
    message.error("复制失败，请手动选择复制");
  }
}

const rsproxyScript = `# 配置 Rust 国内镜像（rsproxy.cn），在 PowerShell 执行：
Copy-Item -Path "$env:USERPROFILE\\.cargo\\config.toml" -Destination "$env:USERPROFILE\\.cargo\\config.toml.bak" -ErrorAction SilentlyContinue
@'
[source.crates-io]
replace-with = 'rsproxy-sparse'
[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
[registries.rsproxy]
index = "sparse+https://rsproxy.cn/index/"
[net]
git-fetch-with-cli = true
'@ | Set-Content "$env:USERPROFILE\\.cargo\\config.toml"

# 安装 Rust（如未安装，官方脚本已内置国内可达节点；也可用 rsproxy 镜像）：
winget install Rustlang.Rustup
# 安装完成后：
rustup default stable-x86_64-pc-windows-msvc   # Tauri 推荐工具链（需 VS Build Tools）
rustup target add x86_64-pc-windows-msvc

# 获取引擎源码（gitee 镜像，免科学上网）：
git clone https://gitee.com/oriera-engine/engine.git oriera-engine
cd oriera-engine
cargo run -p oriera-pc`;
</script>

<template>
  <div class="page">
    <h2 class="page-title">设置</h2>
    <p class="page-sub">引擎与工作台信息。</p>

    <NCard title="🧱 内置引擎快照" size="small" style="margin-bottom: 14px">
      <template #header-extra><NTag type="success" size="small" round>已内置 · 无需联网</NTag></template>
      <NSpace vertical size="small">
        <div class="kv"><span>版本</span><b>{{ info?.snapshot_version || '—' }}</b></div>
        <div class="kv"><span>快照目录</span><code>{{ info?.snapshot_dir || '—' }}</code></div>
        <div class="kv"><span>项目目录</span><code>{{ info?.projects_dir || '—' }}</code></div>
        <div class="kv"><span>本地服务</span><code>127.0.0.1:{{ info?.port ?? '—' }}</code></div>
      </NSpace>
      <NAlert type="info" :show-icon="false" style="margin-top: 12px; font-size: 12.5px">
        引擎快照随安装包内置，Demo 演示厅与游戏生成全部离线可用。
        在线更新（gitee 镜像拉取最新 examples）将在后续版本提供。
      </NAlert>
    </NCard>

    <NCard title="🐾 桌面小宠物" size="small" style="margin-bottom: 14px">
      <div style="display: flex; align-items: center; justify-content: space-between; gap: 12px">
        <div>
          <div style="font-size: 13.5px; color: #e6e8ef">启动 Studio 时自动召唤桌宠</div>
          <div style="font-size: 12px; color: #a0a0ab; margin-top: 2px">
            Ori（白蓝悬浮）/ Era（橙棕机械）陪伴在桌面角落；也可随时在奥粒面板 🐾🤖 召唤
          </div>
        </div>
        <NSwitch v-model:value="petAuto" @update:value="onPetAuto" />
      </div>
    </NCard>

    <NCard title="🧑‍💻 开发者模式" size="small" style="margin-bottom: 14px">
      <div style="display: flex; justify-content: space-between; align-items: center; gap: 14px">
        <p style="color: #a0a0ab; font-size: 13px; line-height: 1.8; margin: 0">
          开启后，新打开的 demo / 游戏窗口会自动附带 DevTools（检查元素、看控制台）。
          所有窗口可用 <b>F5</b> 刷新（改了项目文件后刷新即生效）。
        </p>
        <NSwitch :value="developerMode" @update:value="toggleDev" />
      </div>
    </NCard>

    <NCard title="📮 反馈直达引擎仓库" size="small" style="margin-bottom: 14px">
      <template #header-extra><NTag type="info" size="small" round>github.com/OriEra-engine/engine</NTag></template>
      <p style="color: #a0a0ab; font-size: 13px; line-height: 1.8; margin: 0 0 10px">
        使用中遇到问题、有功能建议，直接从这里反馈给 OriEra 世界引擎仓库。
        提交时会自动附带环境信息（版本 / 系统 / GPU），方便定位问题。
      </p>
      <NSpace vertical size="small">
        <NRadioGroup v-model:value="fbKind" size="small">
          <NRadioButton value="bug">🐛 问题反馈</NRadioButton>
          <NRadioButton value="idea">💡 功能建议</NRadioButton>
          <NRadioButton value="question">❓ 使用咨询</NRadioButton>
          <NRadioButton value="other">💬 其他</NRadioButton>
        </NRadioGroup>
        <NInput v-model:value="fbTitle" placeholder="标题：一句话说清问题或建议" size="small" />
        <NInput v-model:value="fbBody" type="textarea" :rows="4" placeholder="详细描述：做了什么、期望什么、实际发生什么（选填）" size="small" />
        <NSpace justify="space-between" align="center">
          <NButton type="primary" size="small" :loading="fbSending" @click="submitFeedback">提交反馈</NButton>
          <NButton size="tiny" quaternary @click="fbShowPat = !fbShowPat">
            {{ fbHasPat ? '直提通道：已配置（点击管理）' : '协作者？配置直提通道' }}
          </NButton>
        </NSpace>
        <div v-if="fbShowPat" style="background: #0a0a0c; border: 1px solid #202027; border-radius: 10px; padding: 12px">
          <p style="color: #6f6f7a; font-size: 12px; line-height: 1.7; margin: 0 0 8px">
            配置 GitHub Personal Access Token（需对 OriEra-engine/engine 有建 issue 权限）后，
            反馈将一键直提仓库，不再跳浏览器。Token 只存本机凭据管理器，不随项目/反馈外泄。
          </p>
          <NSpace>
            <NInput v-model:value="fbPat" type="password" show-password-on="click" placeholder="ghp_…（repo 权限）" size="small" style="width: 280px" />
            <NButton size="small" secondary @click="savePat">保存</NButton>
            <NButton v-if="fbHasPat" size="small" quaternary type="error" @click="clearPat">清除</NButton>
          </NSpace>
        </div>
        <NAlert v-if="fbResult?.mode === 'direct'" type="success" :show-icon="false" style="font-size: 12.5px">
          已创建 issue：<a :href="fbResult.url" target="_blank" style="color: #9fc3ff">{{ fbResult.url }}</a>
        </NAlert>
        <NAlert v-if="fbResult?.mode === 'manual'" type="info" :show-icon="false" style="font-size: 12.5px">
          反馈已存档到 <code>{{ fbResult.file }}</code><br />
          若浏览器没有自动打开，请点击
          <a :href="fbResult.url" target="_blank" style="color: #9fc3ff">这里前往 GitHub 发布页</a>
          （标题和内容已预填，点 Submit 即可）。无法访问 GitHub 时，把存档文件发给社区即可。
        </NAlert>
      </NSpace>
    </NCard>

    <NCard size="small" style="margin-bottom: 14px">
      <NCollapse>
        <template #header>🛠️ 引擎源码开发引导（Rust · 进阶）</template>
      <p style="color: #a0a0ab; font-size: 13px; line-height: 1.8; margin: 0 0 12px">
        进阶用户可获取引擎 Rust 源码、参与引擎开发。全程走国内镜像（rsproxy / gitee），
        <b>无需科学上网</b>。需要约 10GB 磁盘与一定命令行基础。
      </p>
      <NButton type="primary" ghost @click="showDev = true">查看引导手册</NButton>
          </NCollapse>
    </NCard>

    <NCard title="ℹ️ 关于" size="small">
      <p style="color: #74747f; font-size: 12.5px; line-height: 1.8; margin: 0">
        OriEra Studio v{{ APP_VERSION }} —— OriEra 世界引擎的开箱即用桌面工作台。<br />
        生成的游戏原型为 Three.js 3D 场景 + AI NPC + 剧情导演系统的「可运行原型」，
        引擎完整渲染内核（L5/wgpu）仍在路线图中，届时原型可平滑升级。
      </p>
    </NCard>

    <NModal v-model:show="showDev" preset="card" title="开发者引导手册" style="max-width: 760px">
      <NAlert type="warning" :show-icon="false" style="margin-bottom: 12px; font-size: 12.5px">
        以下命令请在 PowerShell 中逐段执行；rsproxy 镜像配置会覆盖 ~/.cargo/config.toml（已有自动备份）。
      </NAlert>
      <pre class="dev-script">{{ rsproxyScript }}</pre>
      <NSpace justify="end">
        <NButton size="small" secondary @click="copy(rsproxyScript)">复制全部命令</NButton>
      </NSpace>
    </NModal>
  </div>
</template>

<style scoped>
.kv { display: flex; gap: 12px; font-size: 13px; align-items: baseline; }
.kv span { color: #6f6f7a; width: 70px; flex: none; }
.kv code { font-size: 12px; color: #9fc3ff; word-break: break-all; }
.dev-script { background: #0a0a0c; border: 1px solid #202027; border-radius: 10px; padding: 14px;
  font-size: 12px; line-height: 1.7; color: #cdd6ec; overflow-x: auto; white-space: pre-wrap; }
</style>
