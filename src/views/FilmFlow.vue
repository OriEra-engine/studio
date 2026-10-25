<script setup lang="ts">
// 影游制作工作流 —— 五步向导：三锁 → 剧情体检 → 镜头量产 → TTS 配音 → 组装导出。
// 管线沉淀自《末班站台》实战（interfilm/）；状态落项目 film/flow.json 跨会话可续作。
import { computed, onMounted, ref, watch } from "vue";
import {
  NAlert, NButton, NCard, NInput, NSelect, NSpace, NSteps, NStep, NTag, useMessage,
} from "naive-ui";
import { api, type FilmFlow, type ProjectMeta } from "../api";
import { FilmStory } from "../lib/filmStory";

const message = useMessage();

/* ── 项目挂载 ── */
const projects = ref<ProjectMeta[]>([]);
const project = ref<string | null>(null);

/* ── 工作流状态 ── */
const step = ref(1);
const locks = ref({ protagonist: "", supporting: "", style: "" });
const storyText = ref("");
const voices = ref<{ role: string; speaker: string }[]>([]);
const clips = ref<Record<string, string>>({});
const voDone = ref<Record<string, string>>({});
const exportedZip = ref("");

const arkKey = ref(localStorage.getItem("oe.arkKey") || "");
const ttsKey = ref(localStorage.getItem("oe.ttsKey") || "");
const clipTier = ref("doubao-seedance-2-0-mini-260615");
const clipRatio = ref("9:16");

/* ── 模板（末班站台缩样，带 shots/vo 演示全管线） ── */
const TEMPLATE = JSON.stringify({
  meta: { title: "末班站台", subtitle: "雨夜 · 三十分钟 · 信谁", aspect: "9:16" },
  vars: { affinity_lin: 0 },
  nodes: [
    { id: "s001", type: "scene", title: "末班地铁", scene: "雨夜站台",
      text: "末班地铁，23点47分。站台只剩你一个人。",
      shots: [
        { frame: "深夜空旷地铁站台全景，青蓝色荧光灯，雨幕斜飘，湿地面反射霓虹橙红光，电影感", motion: "镜头缓慢下摇", dur: 6 },
        { frame: "站台候车线特写，黄色安全线被雨水打湿反光，一双皮鞋停在线后", motion: "缓慢推近", dur: 5, vo: "narrator: 末班地铁，23点47分。站台只剩你一个人。" },
      ], goto: "c001" },
    { id: "c001", type: "choice", title: "站台 · 她出现了", timeout: 10, options: [
      { text: "「你没带伞？我这把给你。」", set: { affinity_lin: "+1" }, goto: "s002a" },
      { text: "装作没看见", set: { affinity_lin: "-1" }, goto: "s002b" } ] },
    { id: "s002a", type: "scene", title: "她的道谢", scene: "站台",
      text: "她接过伞，指尖很凉。",
      shots: [{ frame: "年轻女子近景接过一把深色长柄伞，指尖颤抖，背景站台灯青蓝虚化", motion: "手持轻晃", dur: 6, vo: "lin: 谢谢。……你也会等这一班吗？" }], goto: "hub" },
    { id: "s002b", type: "scene", title: "她的目光", scene: "站台",
      text: "她看了你一眼，收回了目光。",
      shots: [{ frame: "年轻女子中景侧影退后半步，风衣收紧，霓虹橙红虚化", motion: "轻微横移", dur: 6 }], goto: "hub" },
    { id: "hub", type: "hub", routes: [
      { when: "affinity_lin >= 1", goto: "e_truth" }, { goto: "e_gone" } ] },
    { id: "e_truth", type: "ending", title: "雨夜真相", scene: "警局",
      text: "她跟你走进了那片红蓝光。雨停之前，你听见她说出第一个证人的名字。",
      shots: [{ frame: "警局值班台近景，牛皮纸信封摊开，红蓝警灯透过百叶窗扫过桌面", motion: "缓慢推近定格", dur: 6 }] },
    { id: "e_gone", type: "ending", title: "消失的站台", scene: "台阶",
      text: "台阶上只剩那把黑伞，安静地立在雨里。末班地铁，再没有来过这个站台。",
      shots: [{ frame: "雨夜台阶上一把收拢的黑伞直立，无人，橙色路灯，红蓝警灯远闪", motion: "缓慢拉远", dur: 6 }] },
  ],
}, null, 2);

const DEFAULT_VOICES = [
  { role: "narrator", speaker: "zh_male_shenyeboke_uranus_bigtts" },
  { role: "lin", speaker: "zh_female_linxiao_uranus_bigtts" },
  { role: "umbrella", speaker: "zh_male_gaolengchenwen_uranus_bigtts" },
];
const SPEAKERS = [
  { label: "深夜播客 2.0（旁白 · 深夜氛围）", value: "zh_male_shenyeboke_uranus_bigtts" },
  { label: "林潇 2.0（女主 · 清冷干净）", value: "zh_female_linxiao_uranus_bigtts" },
  { label: "高冷沉稳 2.0（反派 · 克制压迫）", value: "zh_male_gaolengchenwen_uranus_bigtts" },
  { label: "悬疑解说 2.0（旁白 · 低沉神秘）", value: "zh_male_xuanyijieshuo_uranus_bigtts" },
  { label: "温柔妈妈 2.0（治愈 · 中年女声）", value: "zh_female_wenroumama_uranus_bigtts" },
  { label: "译制片男 2.0（浑厚磁性）", value: "zh_male_yizhipiannan_uranus_bigtts" },
  { label: "高冷御姐 2.0（御姐 · 清冷疏离）", value: "zh_female_gaolengyujie_uranus_bigtts" },
  { label: "庄周 2.0（空灵 · 仙气）", value: "zh_male_zhuangzhou_uranus_bigtts" },
];

onMounted(async () => {
  projects.value = await api.listProjects();
  if (projects.value.length === 0) return;
  project.value = projects.value[0].id;
  const flow = await api.filmFlowLoad(project.value);
  if (flow) {
    step.value = flow.step || 1;
    locks.value = { ...locks.value, ...flow.locks };
    storyText.value = flow.story || "";
    voices.value = flow.voices?.length ? flow.voices : [...DEFAULT_VOICES];
    clips.value = flow.clips || {};
    voDone.value = flow.vo || {};
    exportedZip.value = flow.exported_zip || "";
    if (storyText.value) message.info("已恢复上次工作流进度");
  } else {
    storyText.value = TEMPLATE;
    voices.value = [...DEFAULT_VOICES];
  }
});

watch(project, async id => {
  if (!id) return;
  const flow = await api.filmFlowLoad(id);
  step.value = flow?.step || 1;
  storyText.value = flow?.story || TEMPLATE;
  clips.value = flow?.clips || {};
  voDone.value = flow?.vo || {};
});

async function save(next = step.value) {
  if (!project.value) return;
  const flow: FilmFlow = {
    step: next,
    locks: locks.value,
    story: storyText.value,
    voices: voices.value,
    clips: clips.value,
    vo: voDone.value,
    exported_zip: exportedZip.value || null,
  };
  await api.filmFlowSave(project.value, flow);
}

/* ── Step2 体检（引擎 L9 内核镜像） ── */
const findings = ref<any[]>([]);
const checked = ref(false);
function check() {
  try {
    const g = FilmStory.parse(storyText.value);
    findings.value = FilmStory.validate(g);
  } catch (e) {
    findings.value = [{ severity: "error", node: "-", kind: "json", detail: String(e) }];
  }
  checked.value = true;
  if (findings.value.length === 0) message.success("体检全绿 —— 可以进入镜头量产");
}
const storyGreen = computed(() => checked.value && findings.value.length === 0);

/* ── Step3 镜头清单（story.shots 扫描） ── */
interface Shot { id: string; frame: string; motion: string; dur: number; status: string }
const shots = computed<Shot[]>(() => {
  try {
    // 原始 JSON 扫描（FilmStory.parse 是白名单重塑，不透传 shots）
    const g = JSON.parse(storyText.value);
    const out: Shot[] = [];
    for (const n of g.nodes ?? []) {
      (n.shots ?? []).forEach((s: any, i: number) => {
        const id = `${n.id}_${i}`;
        out.push({
          id, frame: s.frame || "", motion: s.motion || "", dur: s.dur || 5,
          status: clips.value[id] ? "done" : "pending",
        });
      });
    }
    return out;
  } catch { return []; }
});
const clipBusy = ref("");
const clipMsg = ref("");
const clipCost = computed(() =>
  shots.value.length * (clipTier.value.includes("mini") ? 2.5 : 8.4));

async function genShot(shot: Shot) {
  if (!project.value) return message.warning("先选择项目");
  if (!arkKey.value.trim()) return message.warning("先填入方舟 API Key");
  localStorage.setItem("oe.arkKey", arkKey.value.trim());
  clipBusy.value = shot.id;
  clipMsg.value = `生成 ${shot.id}（约 1.5-3.5 分钟）…`;
  try {
    const meta = await api.generateVideo(
      shot.frame + (shot.motion ? `，${shot.motion}` : ""), arkKey.value.trim(),
      undefined, clipTier.value, clipRatio.value);
    clips.value[shot.id] = meta.file;
    await save();
    message.success(`${shot.id} 完成`);
  } catch (e) {
    message.error(`${shot.id} 失败：${String(e).slice(0, 120)}`);
  } finally {
    clipBusy.value = "";
    clipMsg.value = "";
  }
}
async function genAllShots() {
  for (const s of shots.value) {
    if (clips.value[s.id]) continue;
    await genShot(s);
    if (!clips.value[s.id] && clipBusy.value) break; // 失败中断，避免连环烧钱
  }
}

/* ── Step4 配音清单（shots.vo 扫描 + 结局旁白） ── */
interface VoItem { id: string; role: string; text: string; done: boolean }
const voItems = computed<VoItem[]>(() => {
  try {
    // 原始 JSON 扫描：shots.vo（"role: 台词"）+ 无 vo 的 ending 旁白（text 前段）
    const g = JSON.parse(storyText.value);
    const out: VoItem[] = [];
    for (const n of g.nodes ?? []) {
      (n.shots ?? []).forEach((s: any, i: number) => {
        if (!s.vo) return;
        const m = String(s.vo).match(/^(\w+)\s*[:：]\s*(.+)$/);
        out.push({
          id: `${n.id}_${i}`,
          role: m ? m[1] : "narrator",
          text: m ? m[2] : String(s.vo),
          done: !!voDone.value[`${n.id}_${i}`],
        });
      });
      if (n.type === "ending" && !(n.shots ?? []).some((s: any) => s.vo)) {
        out.push({ id: `${n.id}_0`, role: "narrator", text: n.text || "", done: !!voDone.value[`${n.id}_0`] });
      }
    }
    return out.filter(v => v.text.trim());
  } catch { return []; }
});
function speakerOf(role: string) {
  return voices.value.find(v => v.role === role)?.speaker || "zh_male_shenyeboke_uranus_bigtts";
}
const setSpeaker = (role: string, sp: string) => {
  const hit = voices.value.find(v => v.role === role);
  if (hit) hit.speaker = sp; else voices.value.push({ role, speaker: sp });
};
const voBusy = ref("");
async function genVo(item: VoItem) {
  if (!project.value) return message.warning("先选择项目");
  if (!ttsKey.value.trim()) return message.warning("先填入豆包语音 API Key");
  localStorage.setItem("oe.ttsKey", ttsKey.value.trim());
  voBusy.value = item.id;
  try {
    const f = await api.ttsDoubao(project.value, ttsKey.value.trim(), speakerOf(item.role), item.text, item.id);
    voDone.value[item.id] = f;
    await save();
    message.success(`${item.id} 配音完成`);
  } catch (e) {
    message.error(`${item.id} 失败：${String(e).slice(0, 120)}`);
  } finally { voBusy.value = ""; }
}
async function genAllVo() {
  for (const v of voItems.value) {
    if (voDone.value[v.id]) continue;
    await genVo(v);
  }
}

/* ── Step5 导出 ── */
const exporting = ref(false);
async function doExport() {
  if (!project.value) return;
  exporting.value = true;
  try {
    const flow: FilmFlow = {
      step: 5, locks: locks.value, story: storyText.value, voices: voices.value,
      clips: clips.value, vo: voDone.value, exported_zip: exportedZip.value || null,
    };
    const zip = await api.exportFilm(project.value, flow);
    exportedZip.value = zip;
    await save(5);
    message.success("导出完成 —— 解压后双击 index.html 即玩");
  } catch (e) {
    message.error(`导出失败：${String(e).slice(0, 140)}`);
  } finally { exporting.value = false; }
}

const canNext = computed(() => {
  if (step.value === 2) return storyGreen.value;
  return true;
});
function go(next: number) {
  if (next > step.value && !canNext.value) {
    message.warning(step.value === 2 ? "先通过一键体检（全绿）再进入量产" : "");
    return;
  }
  step.value = next;
  save(next);
}
</script>

<template>
  <div>
    <NAlert v-if="projects.length === 0" type="warning" style="margin-bottom: 12px">
      还没有项目 —— 去「新建游戏」建一个项目后回来，工作流状态会落在项目目录 film/ 下。
    </NAlert>
    <NSpace v-else align="center" style="margin-bottom: 14px">
      <span style="font-size: 12px; color: #71717c">项目</span>
      <NSelect v-model:value="project" size="small" style="width: 220px"
        :options="projects.map(p => ({ label: p.name, value: p.id }))" />
    </NSpace>

    <NSteps :current="step" size="small" style="margin-bottom: 18px" @update:current="go">
      <NStep title="三锁" description="角色与风格锁定" />
      <NStep title="剧情图" description="编辑 + 一键体检" />
      <NStep title="镜头" description="Seedance 量产" />
      <NStep title="配音" description="豆包 TTS 2.0" />
      <NStep title="导出" description="单文件播放器 zip" />
    </NSteps>

    <!-- Step 1 三锁 -->
    <NCard v-if="step === 1" size="small" title="🔒 三锁 —— 把角色和风格焊死，量产不跑偏">
      <NSpace vertical size="large">
        <NInput v-model:value="locks.protagonist" type="textarea" :rows="2"
          placeholder="主角卡：26 岁女子，齐肩黑发，米白风衣，左眉一道疤，声音很轻（一句话焊死外形+气质）" />
        <NInput v-model:value="locks.supporting" type="textarea" :rows="2"
          placeholder="配角卡：黑伞男，黑色长大衣，伞拄地如拐杖，面部阴影（反派剪影不要正脸）" />
        <NInput v-model:value="locks.style" type="textarea" :rows="2"
          placeholder="风格锁：电影感青蓝色调，雨夜霓虹橙红点缀，手持摄影感（色调+光影+机位感）" />
        <NSpace>
          <NButton type="primary" @click="go(2)">下一步：剧情图 →</NButton>
          <span style="font-size: 12px; color: #71717c; line-height: 34px">
            三锁会拼进每个镜头提示词（模板 story 的 shots 里手写效果相同）
          </span>
        </NSpace>
      </NSpace>
    </NCard>

    <!-- Step 2 剧情图 -->
    <NCard v-if="step === 2" size="small" title="📜 剧情图 story.json（单源真源）">
      <NSpace style="margin-bottom: 10px">
        <NButton size="small" @click="storyText = TEMPLATE">载入末班站台模板</NButton>
        <NButton size="small" type="primary" @click="check">🩺 一键体检</NButton>
        <NTag v-if="checked" size="small" :type="storyGreen ? 'success' : 'error'" round>
          {{ storyGreen ? "全绿" : findings.length + " 项待修" }}
        </NTag>
      </NSpace>
      <NInput v-model:value="storyText" type="textarea" :rows="16"
        style="font-family: Consolas, monospace; font-size: 12px" />
      <div v-if="checked && findings.length" style="margin-top: 8px">
        <div v-for="(f, i) in findings" :key="i" style="font-size: 12px; padding: 2px 0">
          <NTag size="tiny" :type="f.severity === 'error' ? 'error' : 'warning'" round>{{ f.severity }}</NTag>
          <code style="color:#ff9a3c; margin: 0 6px">{{ f.kind }}</code>
          <code style="margin-right: 6px">{{ f.node }}</code>
          <span style="color:#71717c">{{ f.detail }}</span>
        </div>
      </div>
      <NSpace style="margin-top: 12px">
        <NButton :disabled="!storyGreen" type="primary" @click="go(3)">
          {{ storyGreen ? "下一步：镜头量产 →" : "体检全绿后解锁" }}
        </NButton>
      </NSpace>
    </NCard>

    <!-- Step 3 镜头量产 -->
    <NCard v-if="step === 3" size="small" title="🎬 镜头量产（火山方舟 Seedance · 实测约 2-4 分钟/条）">
      <NSpace style="margin-bottom: 12px" align="center" :wrap="false">
        <NInput v-model:value="arkKey" type="password" show-password-on="click" size="small"
          placeholder="方舟 API Key（只存本机浏览器）" style="max-width: 230px" />
        <NSelect v-model:value="clipTier" size="small" style="width: 200px" :options="[
          { label: '2.0-mini 草稿档（≈¥2.5/条）', value: 'doubao-seedance-2-0-mini-260615' },
          { label: '2.5 高光档（≈¥8.4/条，闲时约55%）', value: 'doubao-seedance-2-5-260628' }]" />
        <NSelect v-model:value="clipRatio" size="small" style="width: 110px" :options="[
          { label: '9:16 竖屏', value: '9:16' }, { label: '16:9 横屏', value: '16:9' }]" />
        <NButton size="small" type="primary" :loading="!!clipBusy" @click="genAllShots">
          ▶ 跑全部缺片（{{ shots.filter(s => !clips[s.id]).length }} 条 · 估 ¥{{ clipCost.toFixed(1) }}）
        </NButton>
      </NSpace>
      <p style="font-size: 12px; color: #71717c; margin: 0 0 10px">
        {{ clipMsg }}已就绪 {{ Object.keys(clips).length }}/{{ shots.length }} 条；缺失镜头播放器自动 CSS 氛围兜底，导出不会被卡。
      </p>
      <div style="max-height: 300px; overflow-y: auto">
        <div v-for="s in shots" :key="s.id"
          style="display: flex; gap: 10px; align-items: center; padding: 6px 0; border-bottom: 1px solid rgba(255,255,255,.06)">
          <NTag size="small" :type="clips[s.id] ? 'success' : clipBusy === s.id ? 'info' : 'default'" round style="width: 74px; justify-content: center">
            {{ clips[s.id] ? "✓ 已就绪" : clipBusy === s.id ? "生成中…" : "待生成" }}
          </NTag>
          <div style="flex: 1; min-width: 0">
            <div style="font-size: 13px">{{ s.id }} · {{ s.frame.slice(0, 42) }}{{ s.frame.length > 42 ? "…" : "" }}</div>
            <div style="font-size: 11px; color: #71717c">{{ s.motion }} · {{ s.dur }}s</div>
          </div>
          <NButton size="tiny" :loading="clipBusy === s.id" @click="genShot(s)">
            {{ clips[s.id] ? "重生成" : "生成" }}
          </NButton>
        </div>
      </div>
      <NButton style="margin-top: 12px" type="primary" @click="go(4)">下一步：配音 →</NButton>
    </NCard>

    <!-- Step 4 配音 -->
    <NCard v-if="step === 4" size="small" title="🎙 配音（豆包语音合成 2.0 · 新版控制台 API Key，区别于方舟 Key）">
      <NSpace style="margin-bottom: 12px" align="center" :wrap="false">
        <NInput v-model:value="ttsKey" type="password" show-password-on="click" size="small"
          placeholder="豆包语音 API Key（console.volcengine.com/speech/new）" style="max-width: 260px" />
        <NButton size="small" type="primary" :loading="!!voBusy" @click="genAllVo">
          ▶ 合成全部（{{ voItems.filter(v => !voDone[v.id]).length }} 条待做）
        </NButton>
      </NSpace>
      <div style="max-height: 320px; overflow-y: auto">
        <div v-for="v in voItems" :key="v.id"
          style="display: flex; gap: 10px; align-items: center; padding: 6px 0; border-bottom: 1px solid rgba(255,255,255,.06)">
          <NTag size="small" :type="voDone[v.id] ? 'success' : voBusy === v.id ? 'info' : 'default'" round style="width: 74px; justify-content: center">
            {{ voDone[v.id] ? "✓ 已合成" : voBusy === v.id ? "合成中…" : "待合成" }}
          </NTag>
          <div style="flex: 1; min-width: 0">
            <div style="font-size: 13px">{{ v.id }} · {{ v.text.slice(0, 36) }}{{ v.text.length > 36 ? "…" : "" }}</div>
            <NSelect :value="speakerOf(v.role)" size="tiny" style="width: 260px; margin-top: 2px"
              :options="SPEAKERS" filterable tag
              @update:value="(val: string) => setSpeaker(v.role, val)" />
          </div>
          <NButton size="tiny" :loading="voBusy === v.id" @click="genVo(v)">合成</NButton>
        </div>
        <p v-if="voItems.length === 0" style="font-size: 12px; color: #71717c; padding: 8px 0">
          剧情里还没有台词 —— 在 story 的镜头里写 vo 字段（如 "lin: 谢谢。……你也会等这一班吗？"）后回来。
        </p>
      </div>
      <NButton style="margin-top: 12px" type="primary" @click="go(5)">下一步：导出 →</NButton>
    </NCard>

    <!-- Step 5 导出 -->
    <NCard v-if="step === 5" size="small" title="📦 组装导出 —— 单文件播放器 + 素材 zip">
      <NSpace vertical>
        <p style="font-size: 13px; color: #93a7b4; line-height: 2">
          镜头 {{ Object.keys(clips).length }}/{{ shots.length }} 条 · 配音 {{ Object.keys(voDone).length }}/{{ voItems.length }} 条。<br>
          导出物：index.html（打字机字幕/倒计时环/变量分支/存档/图鉴，缺片镜头自动 CSS 雨夜氛围兜底）+ clips/ + vo/ + story.json。
          解压后双击 index.html 即玩。
        </p>
        <NSpace>
          <NButton type="primary" :loading="exporting" @click="doExport">📦 组装导出</NButton>
          <NButton v-if="exportedZip" size="small" @click="navigator.clipboard.writeText(exportedZip)">
            复制 zip 路径
          </NButton>
        </NSpace>
        <NAlert v-if="exportedZip" type="success" :show-icon="false">
          导出完成：<code style="font-size: 12px">{{ exportedZip }}</code>
        </NAlert>
      </NSpace>
    </NCard>
  </div>
</template>
