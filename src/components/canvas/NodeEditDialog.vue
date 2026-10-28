<script setup lang="ts">
// 统一编辑弹层（M0 铁律：画布改剧情只走这里，story 单一真源整体回写）。
// 结构化编辑 title/text/scene/shots/options/routes/goto，未知字段原样保留
// （parse 进来的原始对象直接改，JSON.stringify 整体落回，不丢 shots 外字段）。
import { computed, ref, watch } from "vue";
import { NButton, NInput, NInputNumber, NSelect, NSpace, NTag } from "naive-ui";
import type { RawNode, RawStory } from "../../lib/canvas/storywalk";

const props = defineProps<{
  show: boolean;
  story: RawStory | null;
  nodeRef: string | null;
  pro: boolean;
}>();
const emit = defineEmits<{
  (e: "update:show", v: boolean): void;
  /** 把整份 story 序列化回写（父级负责写回 flow.story 并落盘） */
  (e: "save", storyText: string): void;
}>();

const draft = ref<RawNode | null>(null);
const lockedConfirm = ref(false);

watch(
  () => [props.show, props.nodeRef] as const,
  ([show, ref]) => {
    if (!show || !props.story || !ref) return;
    const hit = props.story.nodes.find((n) => n.id === ref);
    draft.value = hit ? JSON.parse(JSON.stringify(hit)) : null;
    lockedConfirm.value = false;
  },
  { immediate: true },
);

const nodeOptions = computed(() =>
  (props.story?.nodes ?? []).map((n) => ({ label: `${n.id} · ${n.title || n.type}`, value: n.id })),
);

const KIND_NAME: Record<string, string> = { scene: "场景", choice: "抉择", hub: "分流", ending: "结局" };

function addShot() {
  if (!draft.value) return;
  if (!Array.isArray(draft.value.shots)) draft.value.shots = [];
  draft.value.shots.push({ frame: "", motion: "", dur: 5, vo: "" });
}
function rmShot(i: number) {
  draft.value?.shots?.splice(i, 1);
}
function addOption() {
  if (!draft.value) return;
  if (!Array.isArray(draft.value.options)) draft.value.options = [];
  draft.value.options.push({ text: "", set: null, goto: nodeOptions.value[0]?.value ?? "" });
}
function rmOption(i: number) {
  draft.value?.options?.splice(i, 1);
}
function addRoute() {
  if (!draft.value) return;
  if (!Array.isArray(draft.value.routes)) draft.value.routes = [];
  draft.value.routes.push({ when: null, goto: nodeOptions.value[0]?.value ?? "" });
}
function rmRoute(i: number) {
  draft.value?.routes?.splice(i, 1);
}

function setJsonSet(i: number, text: string) {
  const o = draft.value?.options?.[i];
  if (!o) return;
  const t = text.trim();
  if (!t) { o.set = null; return; }
  try { o.set = JSON.parse(t); } catch { /* 输入过程中容忍非法 JSON，落盘前再校验 */ }
}

function doSave() {
  if (!draft.value || !props.story) return;
  const idx = props.story.nodes.findIndex((n) => n.id === draft.value!.id);
  if (idx < 0) return;
  props.story.nodes[idx] = draft.value;
  emit("save", JSON.stringify(props.story, null, 2));
  emit("update:show", false);
}
</script>

<template>
  <div v-if="show && draft" class="ned-mask" @click.self="emit('update:show', false)">
    <div class="ned">
      <div class="ned-head">
        <span class="ned-kind">{{ KIND_NAME[draft.type] || draft.type }}</span>
        <code class="mono">{{ draft.id }}</code>
        <span class="spacer"></span>
        <NTag v-if="pro" size="tiny" :bordered="false">编辑会整体回写 story.json</NTag>
        <NButton size="tiny" @click="emit('update:show', false)">关闭</NButton>
      </div>

      <div class="ned-body">
        <div class="ned-row">
          <label>标题</label>
          <NInput v-model:value="draft.title" size="small" placeholder="节拍标题" />
        </div>
        <div v-if="draft.type === 'scene' || draft.type === 'ending'" class="ned-row">
          <label>{{ draft.type === 'ending' ? '结局文案' : '剧情文本' }}</label>
          <NInput v-model:value="draft.text" type="textarea" :rows="2" size="small" />
        </div>
        <div v-if="draft.type === 'scene'" class="ned-row">
          <label>场景氛围</label>
          <NInput v-model:value="draft.scene" size="small" placeholder="如：雨夜站台（缺片时的 CSS 氛围兜底依据）" />
        </div>

        <template v-if="draft.type === 'scene' || draft.type === 'ending'">
          <div class="ned-sec">
            <div class="ned-sec-head">
              <span>镜头（shots）</span>
              <NButton size="tiny" @click="addShot">+ 加镜头</NButton>
            </div>
            <div v-for="(s, i) in draft.shots ?? []" :key="i" class="ned-shot">
              <NInput v-model:value="s.frame" size="small" type="textarea" :rows="2"
                :placeholder="`镜头 ${i + 1} 画面提示词（Seedance 用）`" />
              <div class="ned-shot-row">
                <NInput v-model:value="s.motion" size="small" placeholder="运镜" style="flex:1" />
                <NInputNumber v-model:value="s.dur" size="small" :min="1" :max="30" style="width:86px" />
                <NInput v-model:value="s.vo" size="small" placeholder="台词（role: 台词）" style="flex:2" />
                <NButton size="tiny" quaternary type="error" @click="rmShot(i)">删</NButton>
              </div>
            </div>
          </div>
          <div v-if="draft.type === 'scene'" class="ned-row">
            <label>结束后去</label>
            <NSelect v-model:value="draft.goto" size="small" clearable :options="nodeOptions" placeholder="选择下一节拍" />
          </div>
        </template>

        <template v-if="draft.type === 'choice'">
          <div class="ned-sec">
            <div class="ned-sec-head">
              <span>选项（options）</span>
              <NButton size="tiny" @click="addOption">+ 加选项</NButton>
            </div>
            <div v-for="(o, i) in draft.options ?? []" :key="i" class="ned-opt">
              <NInput v-model:value="o.text" size="small" placeholder="选项文案" />
              <div class="ned-shot-row">
                <NInput :value="o.set ? JSON.stringify(o.set) : ''" size="small" placeholder='变量结算 {"好感":"+1"}'
                  @update:value="(t: string) => { try { o.set = t.trim() ? JSON.parse(t) : null } catch { /* 容忍输入中 */ } }" style="flex:1" />
                <NSelect v-model:value="o.goto" size="small" :options="nodeOptions" style="width:180px" />
                <NButton size="tiny" quaternary type="error" @click="rmOption(i)">删</NButton>
              </div>
            </div>
          </div>
          <div class="ned-row">
            <label>超时秒数</label>
            <NInputNumber v-if="'timeout' in draft" v-model:value="(draft as any).timeout" size="small" :min="0" style="width:120px" />
          </div>
        </template>

        <template v-if="draft.type === 'hub'">
          <div class="ned-sec">
            <div class="ned-sec-head">
              <span>分流路由（按声明顺序取第一个命中）</span>
              <NButton size="tiny" @click="addRoute">+ 加路由</NButton>
            </div>
            <div v-for="(r, i) in draft.routes ?? []" :key="i" class="ned-opt">
              <div class="ned-shot-row">
                <NInput :value="r.when ?? ''" size="small" placeholder='条件，如 好感 >= 1（留空 = 默认）'
                  @update:value="(t: string) => { r.when = t.trim() || null }" style="flex:1" />
                <NSelect v-model:value="r.goto" size="small" :options="nodeOptions" style="width:180px" />
                <NButton size="tiny" quaternary type="error" @click="rmRoute(i)">删</NButton>
              </div>
            </div>
          </div>
        </template>
      </div>

      <div class="ned-foot">
        <span class="ned-hint">未知字段原样保留 · 改完到「体检」验证</span>
        <NButton size="small" @click="emit('update:show', false)">取消</NButton>
        <NButton size="small" type="primary" @click="doSave">保存并回写</NButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.ned-mask { position: fixed; inset: 0; background: rgba(0,0,0,.55); z-index: 100; display: flex; align-items: center; justify-content: center; }
.ned { width: 640px; max-width: 92vw; max-height: 84vh; display: flex; flex-direction: column; background: #16161b; border: 1px solid #30303a; border-radius: 12px; }
.ned-head { display: flex; align-items: center; gap: 8px; padding: 12px 16px; border-bottom: 1px solid #202027; }
.ned-kind { font-size: 13px; font-weight: 600; color: #4f8ae8; }
.ned-head .spacer { flex: 1; }
.ned-body { flex: 1; overflow-y: auto; padding: 12px 16px; }
.ned-row { display: flex; align-items: center; gap: 10px; margin-bottom: 10px; }
.ned-row > label { width: 72px; flex: none; font-size: 12px; color: #71717c; text-align: right; }
.ned-row :deep(.n-input), .ned-row :deep(.n-select) { flex: 1; }
.ned-sec { margin: 12px 0; }
.ned-sec-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 6px; font-size: 12px; color: #93a7b4; }
.ned-shot { margin-bottom: 10px; padding: 8px; border: 1px solid #202027; border-radius: 8px; }
.ned-opt { margin-bottom: 8px; padding: 8px; border: 1px solid #202027; border-radius: 8px; display: flex; flex-direction: column; gap: 6px; }
.ned-shot-row { display: flex; gap: 6px; align-items: center; margin-top: 6px; }
.ned-foot { display: flex; align-items: center; gap: 8px; padding: 12px 16px; border-top: 1px solid #202027; }
.ned-hint { flex: 1; font-size: 11px; color: #5b6474; }
.mono { font-family: Consolas, monospace; color: #71717c; font-size: 12px; }
</style>
