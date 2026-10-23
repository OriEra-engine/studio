<script setup lang="ts">
// 向导内联模型快配：预设速选 → 粘 key → 测试 → 保存并植入。
// 消除「跳去模型中心 → 配置 → 跳回 → 手动刷新」的流程断点；配置完成经全局 store 即时传导。
import { computed, ref } from "vue";
import { NButton, NInput, NTag, NSpace, NAlert, useMessage } from "naive-ui";
import { api, CLOUD_PRESETS, type ProviderCfg } from "../api";
import { store, refreshStatus, saveProvider } from "../store";

const message = useMessage();

// 速选池：免费/国内直连优先，最多 6 个；其余走「自定义」
const QUICK = computed(() => {
  const free = CLOUD_PRESETS.filter(p => p.free);
  const cn = CLOUD_PRESETS.filter(p => p.cn && !p.free);
  return [...free, ...cn].slice(0, 6);
});

const form = ref<ProviderCfg>({ kind: "", key: "", model: "", base: "" });
const pickedKind = computed(() => QUICK.value.find(p => p.kind === form.value.kind) ?? null);
const modelOptions = computed(() => pickedKind.value?.models ?? []);
const showAdvanced = ref(false);

const saving = ref(false);
const testing = ref(false);
const testResult = ref<{ ok: boolean; text: string } | null>(null);

function pick(kind: string) {
  const p = CLOUD_PRESETS.find(x => x.kind === kind);
  if (!p) return;
  form.value.kind = p.kind;
  form.value.model = p.model;
  form.value.base = p.base;
  testResult.value = null;
}

async function save() {
  if (!form.value.kind) return message.warning("先选择一个模型");
  if (!form.value.key && form.value.key !== "ollama") return message.warning("请粘贴 API Key");
  saving.value = true;
  try {
    await saveProvider({ ...form.value });
    message.success(`已植入 ${form.value.model}——本游戏与已打开的窗口即时生效`);
  } catch (e) {
    message.error(`保存失败：${e}`);
  } finally {
    saving.value = false;
  }
}

async function test() {
  testing.value = true;
  testResult.value = null;
  try {
    const reply = await api.testProvider();
    testResult.value = { ok: true, text: reply };
  } catch (e) {
    testResult.value = { ok: false, text: String(e) };
  } finally {
    testing.value = false;
  }
}

const ready = computed(() => !!store.status?.provider_configured);
</script>

<template>
  <div class="qsetup">
    <NAlert v-if="ready" type="success" :show-icon="false" style="margin-bottom: 4px">
      ✅ 已植入：<b>{{ store.status?.provider?.model }}</b> —— 可直接下一步，或换一个模型。
    </NAlert>

    <div class="qs-label">1 · 选一个模型（免费/国内直连优先）</div>
    <div class="qs-presets">
      <div
        v-for="p in QUICK" :key="p.kind"
        class="qs-preset" :class="{ sel: form.kind === p.kind }" role="button" tabindex="0"
        :aria-label="`选择 ${p.label}`"
        @click="pick(p.kind)" @keydown.enter.prevent="pick(p.kind)" @keydown.space.prevent="pick(p.kind)"
      >
        <b>{{ p.label }}</b>
        <span v-if="p.free" class="qs-free">免费</span>
        <div class="qs-model">{{ p.model }}</div>
      </div>
    </div>

    <div class="qs-label">2 · 粘贴 API Key <span class="qs-hint">（存入系统凭据管理器，不落盘；<a :href="pickedKind?.url || 'https://open.bigmodel.cn'" target="_blank">没有 key？点此注册{{ pickedKind?.label ? ` ${pickedKind.label}` : '' }} ↗</a>）</span></div>
    <NInput v-model:value="form.key" type="password" show-password-on="click" placeholder="粘贴密钥" />

    <div class="qs-label">
      模型名
      <template v-if="modelOptions.length">
        <NTag
          v-for="m in modelOptions" :key="m" size="tiny" round style="margin-left: 6px; cursor: pointer"
          :type="form.model === m ? 'primary' : 'default'" @click="form.model = m"
        >{{ m }}</NTag>
      </template>
    </div>
    <NInput v-model:value="form.model" placeholder="模型名称" size="small" />

    <div style="margin-top: 8px">
      <NButton size="tiny" quaternary @click="showAdvanced = !showAdvanced">{{ showAdvanced ? '收起高级选项' : '高级选项（Base URL / 其他厂商）' }}</NButton>
    </div>
    <div v-if="showAdvanced" style="margin-top: 6px">
      <div class="qs-label">服务商 / Base URL</div>
      <NSpace size="small" style="margin-bottom: 6px" :wrap="true">
        <NTag
          v-for="p in CLOUD_PRESETS" :key="p.kind" size="tiny" round style="cursor: pointer"
          :type="form.kind === p.kind ? 'primary' : 'default'" @click="pick(p.kind)"
        >{{ p.label }}</NTag>
      </NSpace>
      <NInput v-model:value="form.base" placeholder="https://...（OpenAI 兼容端点）" size="small" />
    </div>

    <NSpace style="margin-top: 12px">
      <NButton type="primary" :loading="saving" @click="save">保存并植入</NButton>
      <NButton secondary :loading="testing" :disabled="!form.kind" @click="test">测试连接</NButton>
    </NSpace>
    <div v-if="testResult" style="margin-top: 10px">
      <NAlert :type="testResult.ok ? 'success' : 'error'" :show-icon="false" style="font-size: 12.5px">
        {{ testResult.ok ? '✅ 连接成功：' + testResult.text : '❌ ' + testResult.text }}
      </NAlert>
    </div>
  </div>
</template>

<style scoped>
.qs-label { font-size: 12px; color: #74747f; margin: 12px 0 6px; }
.qs-label:first-child { margin-top: 0; }
.qs-hint { color: #55555f; font-weight: 400; }
.qs-hint a { color: #7eb8ff; text-decoration: none; }
.qs-presets { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
.qs-preset { background: #161a28; border: 1.5px solid #232a3d; border-radius: 10px; padding: 10px 12px; cursor: pointer; font-size: 13px; position: relative; }
.qs-preset:hover { border-color: #3a3a46; }
.qs-preset.sel { border-color: #7eb8ff; background: #17223a; }
.qs-preset:focus-visible { outline: 2px solid #7eb8ff; outline-offset: 2px; }
.qs-free { position: absolute; top: 8px; right: 8px; font-size: 10px; color: #35d07f; border: 1px solid #2a6e4d; border-radius: 8px; padding: 0 6px; }
.qs-model { color: #74747f; font-size: 11px; margin-top: 4px; }
</style>
