<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { api } from "../api";
import type { CopilotTurn } from "../api";

/**
 * 奥粒 Ori —— OriEra Studio 用户辅助 agent 的壳(形象 + 对话面板)。
 * v1 走 Tauri `copilot_chat`(L7 provider);失败/未配置 → 确定性规则应答(引擎 RuleProvider 哲学)。
 */

const router = useRouter();

interface Msg {
  role: "user" | "assistant";
  content: string;
  /** 规则模式应答标记 */
  rule?: boolean;
}

const SYSTEM_PROMPT = [
  "你是「奥粒」(Ori),OriEra Studio 世界引擎工作台的常驻小助手,一颗活着的元粒子。",
  "职责:帮玩家用最少步骤做出游戏。你熟悉工作台:仪表盘、Demo 演示厅(引擎示例)、新建游戏五步向导(玩法模板→叙事框架→AI 剧情生成→命名→生成项目)、模型中心(智谱/DeepSeek/Kimi/豆包等云端 + Ollama 本地双轨)、我的项目(一键运行/备份/导出)、剧情编辑器(节拍增删移 + 带前后文重写)、设置。",
  "风格:简体中文、口语化、短句;给步骤时不超过 6 步,用编号列表;适时引导用户去对应页面;活泼但不啰嗦,偶用一点俏皮,禁用客套话堆砌。",
  "用户要的功能若暂不存在:如实说当前版本没有,并说它排在哪条路线上,不要编造。",
].join("\n");

const HISTORY_KEY = "oriera.copilot.v1";

const open = ref(false);
const busy = ref(false);
const input = ref("");
const providerLabel = ref("连接中…");
const providerOnline = ref(false);
const msgs = ref<Msg[]>([]);
const listEl = ref<HTMLElement>();
const inputEl = ref<HTMLTextAreaElement>();

let typingTimer: number | undefined;

const QUICK_ACTIONS: { label: string; text: string }[] = [
  { label: "🎮 带我做第一个游戏", text: "带我从零做一个游戏,我是新手" },
  { label: "🧠 帮我选模型", text: "该怎么选模型?云端和本地有什么区别?" },
  { label: "📝 剧情卡住了", text: "我的剧情写不下去了,帮我续写一个转折" },
  { label: "💡 你能做什么", text: "你能做什么?" },
];

/* ---------- 规则降级应答(同输入同输出) ---------- */
function ruleReply(q: string): string {
  const t = q.toLowerCase();
  const has = (...ks: string[]) => ks.some((k) => t.includes(k));
  if (has("你是谁", "你能做什么", "帮助", "help"))
    return "我是奥粒,住在这个工作台里的一颗元粒子 ✨ 能帮你:\n1. 带你五步做出一个能玩的游戏(新建游戏向导)\n2. 帮你选模型(云端按量 / Ollama 本地免费)\n3. 陪你打磨剧情(项目 → 剧情编辑器,支持单拍重写)\n4. 随时 Ctrl+K 唤起命令面板,键盘直达任何页面\n先从哪个开始?";
  if (has("第一个", "新手", "从零", "怎么做游戏", "做游戏", "新游戏"))
    return "三分钟出第一个游戏:\n1. 左侧「新建游戏」\n2. 选一个玩法模板(15 个任选,不确定就选「咖啡店经营」)\n3. 选叙事框架(或跳过)\n4. 点「AI 生成」剧情——没配模型就用默认剧情,一样能玩\n5. 起名 → 生成 → 「我的项目」里点运行\n做完回来告诉我,我陪你迭代!";
  if (has("模型", "api", "key", "ollama", "配置"))
    return "模型中心两条路:\n· 云端(推荐):智谱 glm-4-flash 免费档 / DeepSeek / Kimi / 豆包,注册拿 key 填进去即可\n· 本地免费:装 Ollama → 拉一个模型(模型中心一键 serve/pull)\n配好后点「测试连接」。不想配也行,所有功能都有离线兜底。去「模型中心」?";
  if (has("剧情", "节拍", "重写", "续写", "卡住", "写不下"))
    return "剧情的事交给我:\n1. 「我的项目」→ 打开项目 → 剧情编辑器\n2. 卡住的那一拍 → 「AI 重写」,可加要求(比如\"更黑暗一点\")\n3. 想加戏就用「加一拍」,前后剧情自动带上\n4. 改坏了?右上有历史备份(.history 保留 10 份)\n把你现在的卡点发我,配置模型后我能直接帮你续。";
  if (has("3d", "资产", "模型生成", "图片", "mesh", "tripo", "seedance"))
    return "生成 3D 资产是引擎 L8 能力(Tripo / Seedance 已在引擎侧落地,showcase 可见)。Studio 的生成中心排在 v0.4:届时这里能一句话生成 GLB 并热替换进场景。现在想看效果 → Demo 演示厅的 AI 咖啡馆。";
  if (has("快捷键", "键盘", "命令"))
    return "键盘党福利:\n· Ctrl+K —— 命令面板,直达任何页面/动作\n· Ctrl+J —— 召唤/收起我\n· Esc —— 关掉弹层\n试试 Ctrl+K 输入「模型」?";
  if (has("导出", "备份", "存档"))
    return "项目数据安全:\n· 「我的项目」→ 项目详情里有存档导出/导入\n· 剧情编辑器自动备份最近 10 版\n· 项目目录可一键打开(reveal)自己拷贝";
  return "这个问题我在离线规则模式下答不好 😅\n两条路:\n1. 「模型中心」配一个模型(免费档即可),我立刻变聪明\n2. 先用快捷指令:带做游戏 / 选模型 / 剧情卡住了";
}

/* ---------- 会话 ---------- */
function loadHistory() {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    if (raw) msgs.value = JSON.parse(raw).slice(-40);
  } catch {
    /* 忽略坏数据 */
  }
  if (!msgs.value.length) {
    msgs.value = [
      {
        role: "assistant",
        content: "嗨,我是奥粒 ✨ 你的游戏搭子。想做游戏?说「带我做第一个游戏」,三分钟出成品。",
      },
    ];
  }
}
function saveHistory() {
  localStorage.setItem(HISTORY_KEY, JSON.stringify(msgs.value.slice(-40)));
}

async function scrollBottom() {
  await nextTick();
  const el = listEl.value;
  if (el) el.scrollTop = el.scrollHeight;
}

function stopTyping() {
  if (typingTimer !== undefined) {
    clearInterval(typingTimer);
    typingTimer = undefined;
  }
}

/** 打字机式逐段显示(ZCode 式流感);看完落盘。 */
function typeOut(full: string, rule: boolean) {
  msgs.value.push({ role: "assistant", content: "", rule });
  // 注意:必须经响应式代理更新,直接改原始对象不触发渲染
  const prox = msgs.value[msgs.value.length - 1];
  let i = 0;
  typingTimer = window.setInterval(() => {
    i = Math.min(full.length, i + 2);
    prox.content = full.slice(0, i);
    scrollBottom();
    if (i >= full.length) {
      stopTyping();
      busy.value = false;
      saveHistory();
    }
  }, 16);
}

async function send(text?: string) {
  const q = (text ?? input.value).trim();
  if (!q || busy.value) return;
  stopTyping();
  input.value = "";
  msgs.value.push({ role: "user", content: q });
  busy.value = true;
  scrollBottom();

  const history: CopilotTurn[] = msgs.value
    .slice(-12, -1)
    .filter((m) => m.content)
    .map((m) => ({ role: m.role, content: m.content }));

  try {
    const reply = await api.copilotChat(SYSTEM_PROMPT, history, q);
    providerOnline.value = true;
    typeOut(reply || "(空回复,再试一次?)", false);
  } catch (e) {
    const detail = String(e);
    const hint = /模型中心|配置/.test(detail) ? "" : `\n\n(原因:${detail.slice(0, 90)})`;
    typeOut(ruleReply(q) + hint, true);
  }
}

function useQuick(a: { label: string; text: string }) {
  if (!open.value) open.value = true;
  send(a.text);
}

function goAndNote(path: string, note: string) {
  router.push(path);
  if (!open.value) open.value = true;
  msgs.value.push({ role: "assistant", content: note });
  saveHistory();
  scrollBottom();
}

async function refreshStatus() {
  try {
    const st = await api.getStatus();
    providerOnline.value = !!st.provider_configured;
    providerLabel.value = st.provider ? `${st.provider.model}` : "未配置";
  } catch {
    providerOnline.value = false;
    providerLabel.value = "浏览器调试";
  }
}

function toggle() {
  open.value = !open.value;
  if (open.value) {
    refreshStatus();
    nextTick(() => inputEl.value?.focus());
  }
}
function onOpenEvent() {
  if (!open.value) toggle();
  else inputEl.value?.focus();
}
function onResetEvent() {
  stopTyping();
  busy.value = false;
  localStorage.removeItem(HISTORY_KEY);
  msgs.value = [];
  loadHistory();
}
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape" && open.value) {
    open.value = false;
  }
}

onMounted(() => {
  loadHistory();
  refreshStatus();
  window.addEventListener("oriera:copilot:open", onOpenEvent);
  window.addEventListener("oriera:copilot:reset", onResetEvent);
  window.addEventListener("keydown", onKey);
});
onBeforeUnmount(() => {
  stopTyping();
  window.removeEventListener("oriera:copilot:open", onOpenEvent);
  window.removeEventListener("oriera:copilot:reset", onResetEvent);
  window.removeEventListener("keydown", onKey);
});
</script>

<template>
  <!-- 悬浮吉祥物球 -->
  <div class="ori-fab" :class="{ busy }" @click="toggle" title="奥粒 · 工作台助手 (Ctrl+J)">
    <svg viewBox="0 0 64 64" width="54" height="54" aria-hidden="true">
      <defs>
        <radialGradient id="oriBody" cx="35%" cy="28%" r="80%">
          <stop offset="0%" stop-color="#9fd0ff" />
          <stop offset="60%" stop-color="#4f8ae8" />
          <stop offset="100%" stop-color="#2e5fd0" />
        </radialGradient>
      </defs>
      <!-- 轨道光环(思考时加速) -->
      <g class="ori-orbit">
        <circle cx="32" cy="34" r="27" fill="none" stroke="rgba(110,168,254,.35)" stroke-width="1.5" stroke-dasharray="4 7" />
        <circle cx="59" cy="34" r="2.4" fill="#6ea8fe" />
        <circle cx="12" cy="22" r="1.7" fill="#3ec8b0" />
      </g>
      <!-- 天线 + 星尘 -->
      <line x1="32" y1="9" x2="32" y2="15" stroke="#7eb8ff" stroke-width="2" stroke-linecap="round" />
      <circle class="ori-star" cx="32" cy="7" r="3" fill="#ffd86b" />
      <!-- 身体 -->
      <circle cx="32" cy="36" r="21" fill="url(#oriBody)" />
      <ellipse cx="26" cy="28" rx="7" ry="4.5" fill="rgba(255,255,255,.28)" />
      <!-- 眼睛(会眨) -->
      <g class="ori-eyes">
        <circle cx="24.5" cy="34" r="4.6" fill="#f4f8ff" />
        <circle cx="39.5" cy="34" r="4.6" fill="#f4f8ff" />
        <circle cx="25.5" cy="35" r="2.1" fill="#12203c" />
        <circle cx="40.5" cy="35" r="2.1" fill="#12203c" />
      </g>
      <!-- 笑 -->
      <path d="M26 44 Q32 49 38 44" stroke="#12203c" stroke-width="2.2" fill="none" stroke-linecap="round" />
    </svg>
    <span class="ori-status-dot" :class="providerOnline ? 'on' : 'off'"></span>
  </div>

  <!-- 对话抽屉 -->
  <Transition name="ori-drawer">
    <div v-if="open" class="ori-panel" role="dialog" aria-label="奥粒助手">
      <div class="ori-head">
        <svg viewBox="0 0 64 64" width="30" height="30" aria-hidden="true">
          <circle cx="32" cy="36" r="21" fill="#4f8ae8" />
          <circle cx="26.5" cy="34" r="4.2" fill="#f4f8ff" />
          <circle cx="38.5" cy="34" r="4.2" fill="#f4f8ff" />
          <circle cx="27.4" cy="35" r="1.9" fill="#12203c" />
          <circle cx="39.4" cy="35" r="1.9" fill="#12203c" />
          <path d="M27 44 Q32 48 37 44" stroke="#12203c" stroke-width="2" fill="none" stroke-linecap="round" />
          <circle cx="32" cy="8" r="2.6" fill="#ffd86b" />
        </svg>
        <div class="ori-title">
          <b>奥粒 Ori</b>
          <span class="ori-chip" :class="providerOnline ? 'on' : 'off'">
            {{ busy ? "思考中…" : providerOnline ? providerLabel + " · 对话中" : "离线小奥 · 规则应答" }}
          </span>
        </div>
        <button class="ori-close" title="收起 (Esc)" @click="open = false">✕</button>
      </div>

      <div class="ori-list" ref="listEl">
        <div v-for="(m, i) in msgs" :key="i" class="ori-msg" :class="m.role">
          <div class="ori-bubble">
            <span v-if="m.rule && m.role === 'assistant'" class="ori-ruletag">规则模式</span>
            {{ m.content }}<span v-if="busy && i === msgs.length - 1 && m.role === 'assistant'" class="ori-caret"></span>
          </div>
        </div>
        <div v-if="busy && (msgs.length === 0 || msgs[msgs.length - 1].role === 'user')" class="ori-msg assistant">
          <div class="ori-bubble ori-dots"><i></i><i></i><i></i></div>
        </div>
      </div>

      <div class="ori-quick">
        <button v-for="a in QUICK_ACTIONS" :key="a.label" @click="useQuick(a)">{{ a.label }}</button>
        <button class="ori-nav" @click="goAndNote('/new-game', '向导已打开 ✅ 五步走到黑,卡住随时叫我。')">➡️ 去新建游戏</button>
        <button class="ori-nav" @click="goAndNote('/models', '模型中心已打开 ✅ 拿不准就选免费档先跑通。')">➡️ 去模型中心</button>
      </div>

      <div class="ori-inputrow">
        <textarea
          ref="inputEl"
          v-model="input"
          rows="2"
          placeholder="问奥粒…(Enter 发送,Shift+Enter 换行)"
          @keydown.enter.exact.prevent="send()"
        ></textarea>
        <button class="ori-send" :disabled="busy || !input.trim()" @click="send()">➤</button>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.ori-fab {
  position: fixed;
  right: 22px;
  bottom: 22px;
  z-index: 90;
  width: 58px;
  height: 58px;
  cursor: pointer;
  filter: drop-shadow(0 4px 14px rgba(78, 138, 232, 0.45));
  transition: transform 0.15s ease;
}
.ori-fab > svg { display: block; animation: ori-bob 3.6s ease-in-out infinite; }
.ori-fab:hover { transform: scale(1.08); }
.ori-fab:active { transform: scale(0.96); }
@keyframes ori-bob { 0%, 100% { translate: 0 0; } 50% { translate: 0 -5px; } }

.ori-orbit { transform-origin: 32px 34px; animation: ori-spin 14s linear infinite; }
.ori-fab.busy .ori-orbit { animation-duration: 1.6s; }
@keyframes ori-spin { to { transform: rotate(360deg); } }

.ori-eyes { transform-origin: 32px 34px; animation: ori-blink 4.4s infinite; }
@keyframes ori-blink { 0%, 94%, 100% { transform: scaleY(1); } 96.5% { transform: scaleY(0.08); } }

.ori-star { animation: ori-twinkle 2.2s ease-in-out infinite; }
@keyframes ori-twinkle { 0%, 100% { opacity: 1; } 50% { opacity: 0.45; } }

.ori-status-dot {
  position: absolute; right: 5px; bottom: 7px; width: 9px; height: 9px;
  border-radius: 50%; border: 2px solid #0f1117;
}
.ori-status-dot.on { background: #3fd68f; }
.ori-status-dot.off { background: #8a90a6; }

.ori-panel {
  position: fixed; right: 0; top: 0; bottom: 0; width: 400px; max-width: 100vw;
  z-index: 95; display: flex; flex-direction: column;
  background: #141824; border-left: 1px solid #26304a;
  box-shadow: -12px 0 36px rgba(0, 0, 0, 0.45);
}
.ori-drawer-enter-active, .ori-drawer-leave-active { transition: transform 0.22s ease, opacity 0.22s ease; }
.ori-drawer-enter-from, .ori-drawer-leave-to { transform: translateX(30px); opacity: 0; }

.ori-head {
  display: flex; align-items: center; gap: 10px; padding: 12px 14px;
  border-bottom: 1px solid #232d45; background: #171c2b;
}
.ori-title { flex: 1; display: flex; align-items: center; gap: 8px; min-width: 0; }
.ori-title b { font-size: 14px; }
.ori-chip {
  font-size: 11px; padding: 1px 8px; border-radius: 9px;
  border: 1px solid #2c3959; color: #9aa2b8; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 210px;
}
.ori-chip.on { color: #3fd68f; border-color: #215437; }
.ori-chip.off { color: #c9a6ff; border-color: #4a3a66; }
.ori-close {
  background: none; border: none; color: #8a90a6; font-size: 14px; cursor: pointer; padding: 4px 8px; border-radius: 6px;
}
.ori-close:hover { color: #e6e8ef; background: #232d45; }

.ori-list { flex: 1; overflow-y: auto; padding: 14px; display: flex; flex-direction: column; gap: 10px; }
.ori-msg { display: flex; }
.ori-msg.user { justify-content: flex-end; }
.ori-bubble {
  max-width: 86%; padding: 8px 12px; border-radius: 12px; font-size: 13px; line-height: 1.65;
  white-space: pre-wrap; word-break: break-word;
}
.ori-msg.user .ori-bubble { background: #2e5fd0; color: #f0f5ff; border-bottom-right-radius: 4px; }
.ori-msg.assistant .ori-bubble { background: #1c2334; border: 1px solid #273353; border-bottom-left-radius: 4px; }
.ori-ruletag { font-size: 10px; color: #c9a6ff; border: 1px solid #4a3a66; border-radius: 7px; padding: 0 5px; margin-right: 6px; }

.ori-caret { display: inline-block; width: 7px; height: 14px; background: #6ea8fe; margin-left: 2px; animation: ori-caret 0.9s steps(2) infinite; vertical-align: -2px; }
@keyframes ori-caret { 0%, 100% { opacity: 1; } 50% { opacity: 0; } }

.ori-dots { display: flex; gap: 4px; align-items: center; padding: 12px 14px; }
.ori-dots i { width: 6px; height: 6px; border-radius: 50%; background: #6ea8fe; animation: ori-dot 1.1s infinite; }
.ori-dots i:nth-child(2) { animation-delay: 0.18s; }
.ori-dots i:nth-child(3) { animation-delay: 0.36s; }
@keyframes ori-dot { 0%, 100% { opacity: 0.25; translate: 0 0; } 45% { opacity: 1; translate: 0 -3px; } }

.ori-quick { display: flex; flex-wrap: wrap; gap: 6px; padding: 10px 14px 0; }
.ori-quick button {
  font-size: 11.5px; color: #aeb6cc; background: #1c2334; border: 1px solid #2c3959;
  border-radius: 14px; padding: 3px 10px; cursor: pointer; transition: all 0.12s ease;
}
.ori-quick button:hover { color: #6ea8fe; border-color: #3a5f9e; }
.ori-quick .ori-nav { color: #3fd68f; border-color: #215437; }

.ori-inputrow { display: flex; gap: 8px; padding: 10px 14px 14px; align-items: flex-end; }
.ori-inputrow textarea {
  flex: 1; resize: none; background: #0f1117; color: #e6e8ef; border: 1px solid #2c3959;
  border-radius: 10px; padding: 8px 10px; font-size: 13px; font-family: inherit; line-height: 1.5; outline: none;
}
.ori-inputrow textarea:focus { border-color: #3a5f9e; }
.ori-send {
  width: 38px; height: 38px; border-radius: 10px; border: 1px solid #2e5fd0;
  background: #1d3a6e; color: #9fd0ff; font-size: 15px; cursor: pointer;
}
.ori-send:disabled { opacity: 0.35; cursor: default; }
.ori-send:not(:disabled):hover { background: #2e5fd0; color: #fff; }

@media (max-width: 520px) { .ori-panel { width: 100vw; } }
</style>
