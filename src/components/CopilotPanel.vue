<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { api } from "../api";
import type { CopilotTurn } from "../api";

/**
 * 奥粒 Ori —— OriEra Studio 用户辅助 agent 的壳(形象 + 对话面板)。
 * v2 对标 ZCode/Codex 的 agent 对话体验：
 *  - assistant 消息全宽 markdown 流式渲染（代码块/列表/粗体/行内代码）
 *  - 思考中显示状态行 + 计时；可随时停止（cancelAi）
 *  - 每条回答可复制；代码块独立复制
 * v1 走 Tauri `copilot_chat`(L7 provider);失败/未配置 → 确定性规则应答(引擎 RuleProvider 哲学)。
 */

const router = useRouter();

interface Msg {
  role: "user" | "assistant";
  content: string;
  /** 规则模式应答标记 */
  rule?: boolean;
  ts?: number;
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
const elapsed = ref("");
/** 当前流式任务 id（停止用） */
let curTaskId = "";
let typingTimer: number | undefined;
let elapsedTimer: number | undefined;
let busyStart = 0;

const QUICK_ACTIONS: { label: string; text: string }[] = [
  { label: "🎮 带我做第一个游戏", text: "带我从零做一个游戏,我是新手" },
  { label: "🧠 帮我选模型", text: "该怎么选模型?云端和本地有什么区别?" },
  { label: "📝 剧情卡住了", text: "我的剧情写不下去了,帮我续写一个转折" },
  { label: "💡 你能做什么", text: "你能做什么?" },
];

/* ---------- markdown-lite 渲染（零依赖，先转义再注入自有标签，模型输出不可执行） ---------- */
function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
function inline(s: string): string {
  let r = esc(s);
  r = r.replace(/`([^`]+)`/g, '<code class="md-code">$1</code>');
  r = r.replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>");
  return r;
}
function renderBlocks(text: string): string {
  const lines = text.split("\n");
  let html = "";
  let para: string[] = [];
  let list: { ordered: boolean; items: string[] } | null = null;
  const flushPara = () => { if (para.length) { html += `<p>${para.map(inline).join("<br>")}</p>`; para = []; } };
  const flushList = () => {
    if (list) {
      html += list.ordered
        ? `<ol>${list.items.map(i => `<li>${inline(i)}</li>`).join("")}</ol>`
        : `<ul>${list.items.map(i => `<li>${inline(i)}</li>`).join("")}</ul>`;
      list = null;
    }
  };
  for (const raw of lines) {
    const line = raw.trimEnd();
    if (!line.trim()) { flushPara(); flushList(); continue; }
    const h = line.match(/^(#{1,4})\s+(.*)/);
    if (h) { flushPara(); flushList(); const lv = Math.min(h[1].length + 2, 5); html += `<div class="md-h md-h${lv}">${inline(h[2])}</div>`; continue; }
    const ul = line.match(/^\s*[-*•]\s+(.*)/);
    const ol = line.match(/^\s*\d+[.、)]\s+(.*)/);
    if (ul) { flushPara(); if (!list || list.ordered) { flushList(); list = { ordered: false, items: [] }; } list.items.push(ul[1]); continue; }
    if (ol) { flushPara(); if (!list || !list.ordered) { flushList(); list = { ordered: true, items: [] }; } list.items.push(ol[1]); continue; }
    if (/^>\s?/.test(line)) { flushPara(); flushList(); html += `<blockquote>${inline(line.replace(/^>\s?/, ""))}</blockquote>`; continue; }
    para.push(line);
  }
  flushPara(); flushList();
  return html;
}
/** 流式中未闭合的 ``` 也按代码块渲染（逐字浮现） */
function renderMarkdown(src: string): string {
  const parts = src.split("```");
  let html = "";
  for (let i = 0; i < parts.length; i++) {
    if (i % 2 === 1) {
      let code = parts[i];
      let lang = "";
      const nl = code.indexOf("\n");
      if (nl >= 0) {
        const first = code.slice(0, nl).trim();
        if (/^[\w-]{0,20}$/.test(first)) { lang = first; code = code.slice(nl + 1); }
      }
      html += `<div class="md-pre"><div class="md-pre-bar"><span>${esc(lang) || "代码"}</span><span class="md-pre-copy">复制</span></div><pre>${esc(code.replace(/\n$/, ""))}</pre></div>`;
    } else {
      html += renderBlocks(parts[i]);
    }
  }
  return html;
}

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
    return "键盘党福利:\n· Ctrl+K —— 命令面板,直达任何页面/动作\n· Ctrl+J —— 召唤/收起我\n· Ctrl+B —— 折叠/展开左侧栏\n· Esc —— 关掉弹层\n试试 Ctrl+K 输入「模型」?";
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
        ts: Date.now(),
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
function stopElapsed() {
  if (elapsedTimer !== undefined) {
    clearInterval(elapsedTimer);
    elapsedTimer = undefined;
  }
  elapsed.value = "";
}
function startElapsed() {
  busyStart = Date.now();
  elapsed.value = "0s";
  stopElapsed();
  elapsedTimer = window.setInterval(() => {
    const s = Math.floor((Date.now() - busyStart) / 1000);
    elapsed.value = s < 60 ? `${s}s` : `${Math.floor(s / 60)}m${s % 60}s`;
  }, 500);
}

/** 打字机式逐段显示(规则模式);看完落盘。 */
function typeOut(full: string, rule: boolean) {
  msgs.value.push({ role: "assistant", content: "", rule, ts: Date.now() });
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
      stopElapsed();
      saveHistory();
    }
  }, 16);
}

async function send(text?: string) {
  const q = (text ?? input.value).trim();
  if (!q || busy.value) return;
  stopTyping();
  input.value = "";
  autoGrow();
  msgs.value.push({ role: "user", content: q, ts: Date.now() });
  busy.value = true;
  startElapsed();
  scrollBottom();

  const history: CopilotTurn[] = msgs.value
    .slice(-12, -1)
    .filter((m) => m.content)
    .map((m) => ({ role: m.role, content: m.content }));

  try {
    // 真流式：奥粒边想边说；失败回落规则应答
    msgs.value.push({ role: "assistant", content: "", ts: Date.now() });
    const prox = msgs.value[msgs.value.length - 1];
    const finish = () => {
      busy.value = false;
      stopElapsed();
      curTaskId = "";
      if (prox.content) saveHistory();
      scrollBottom();
    };
    curTaskId = "";
    const idP = api.copilotChatStream(SYSTEM_PROMPT, history, q, ev => {
      if (ev.event === "delta") {
        providerOnline.value = true;
        prox.content += ev.text;
        scrollBottom();
      } else if (ev.event === "done") {
        if (!prox.content) prox.content = "(空回复,再试一次?)";
        finish();
      } else {
        const detail = ev.message;
        if (/已取消/.test(detail)) {
          prox.content += "（已停止）";
          finish();
          return;
        }
        prox.rule = true;
        const hint = /模型中心|配置/.test(detail) ? "" : `\n\n(原因:${detail.slice(0, 90)})`;
        prox.content = ruleReply(q) + hint;
        finish();
      }
    });
    idP.then(id => { if (busy.value) curTaskId = id; }).catch(() => { /* invoke 级失败走 catch */ });
    await idP;
  } catch (e) {
    const detail = String(e);
    const hint = /模型中心|配置/.test(detail) ? "" : `\n\n(原因:${detail.slice(0, 90)})`;
    typeOut(ruleReply(q) + hint, true);
  }
}

/** 停止当前回答（ZCode/Codex 式控制权） */
async function stop() {
  if (!busy.value) return;
  if (curTaskId) {
    try { await api.cancelAi(curTaskId); } catch { /* 后端兜底：事件流会自然结束 */ }
  } else {
    stopTyping();
    busy.value = false;
    stopElapsed();
    const last = msgs.value[msgs.value.length - 1];
    if (last?.role === "assistant" && !last.content) {
      last.content = "（已停止）";
      saveHistory();
    }
  }
}

function useQuick(a: { label: string; text: string }) {
  if (!open.value) open.value = true;
  send(a.text);
}

function goAndNote(path: string, note: string) {
  router.push(path);
  if (!open.value) open.value = true;
  msgs.value.push({ role: "assistant", content: note, ts: Date.now() });
  saveHistory();
  scrollBottom();
}

function copyText(text: string, ok?: () => void) {
  try {
    navigator.clipboard?.writeText(text);
    ok?.();
  } catch { /* 剪贴板不可用时静默 */ }
}
/** 整条回答复制（按钮反馈 ✓） */
function copyMsg(e: MouseEvent, m: Msg) {
  const btn = e.target as HTMLElement;
  copyText(m.content, () => {
    if (btn) {
      btn.textContent = "✓";
      setTimeout(() => (btn.textContent = "⎘"), 1000);
    }
  });
}
/** 代码块复制（事件委托） */
function onContentClick(e: MouseEvent) {
  const t = e.target as HTMLElement;
  if (t.classList.contains("md-pre-copy")) {
    const pre = t.closest(".md-pre")?.querySelector("pre");
    if (pre) {
      copyText(pre.textContent ?? "", () => {
        t.textContent = "已复制";
        setTimeout(() => (t.textContent = "复制"), 1200);
      });
    }
  }
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
  stopElapsed();
  busy.value = false;
  curTaskId = "";
  localStorage.removeItem(HISTORY_KEY);
  msgs.value = [];
  loadHistory();
  nextTick(() => inputEl.value?.focus());
}
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape" && open.value) {
    open.value = false;
  }
}

/* 输入框自适应高度（composer 手感） */
function autoGrow() {
  const el = inputEl.value;
  if (!el) return;
  el.style.height = "auto";
  el.style.height = Math.min(el.scrollHeight, 150) + "px";
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
  stopElapsed();
  window.removeEventListener("oriera:copilot:open", onOpenEvent);
  window.removeEventListener("oriera:copilot:reset", onResetEvent);
  window.removeEventListener("keydown", onKey);
});
</script>

<template>
  <!-- 悬浮吉祥物球（面板打开时隐藏，避免双层入口） -->
  <div
    v-if="!open"
    class="ori-fab" :class="{ busy }"
    role="button" tabindex="0" :aria-expanded="open" aria-label="奥粒,工作台助手(Ctrl+J 打开或收起)"
    @click="toggle" @keydown.enter.prevent="toggle" @keydown.space.prevent="toggle"
  >
    <svg viewBox="0 0 64 64" width="54" height="54" aria-hidden="true">
      <defs>
        <radialGradient id="oriBody" cx="35%" cy="28%" r="80%">
          <stop offset="0%" stop-color="#9fd0ff" />
          <stop offset="60%" stop-color="#4f8ae8" />
          <stop offset="100%" stop-color="#2e5fd0" />
        </radialGradient>
      </defs>
      <g class="ori-orbit">
        <circle cx="32" cy="34" r="27" fill="none" stroke="rgba(110,168,254,.35)" stroke-width="1.5" stroke-dasharray="4 7" />
        <circle cx="59" cy="34" r="2.4" fill="#6ea8fe" />
        <circle cx="12" cy="22" r="1.7" fill="#3ec8b0" />
      </g>
      <line x1="32" y1="9" x2="32" y2="15" stroke="#7eb8ff" stroke-width="2" stroke-linecap="round" />
      <circle class="ori-star" cx="32" cy="7" r="3" fill="#ffd86b" />
      <circle cx="32" cy="36" r="21" fill="url(#oriBody)" />
      <ellipse cx="26" cy="28" rx="7" ry="4.5" fill="rgba(255,255,255,.28)" />
      <g class="ori-eyes">
        <circle cx="24.5" cy="34" r="4.6" fill="#f4f8ff" />
        <circle cx="39.5" cy="34" r="4.6" fill="#f4f8ff" />
        <circle cx="25.5" cy="35" r="2.1" fill="#12203c" />
        <circle cx="40.5" cy="35" r="2.1" fill="#12203c" />
      </g>
      <path d="M26 44 Q32 49 38 44" stroke="#12203c" stroke-width="2.2" fill="none" stroke-linecap="round" />
    </svg>
    <span class="ori-status-dot" :class="providerOnline ? 'on' : 'off'"></span>
  </div>

  <!-- 对话抽屉 -->
  <Transition name="ori-drawer">
    <div v-if="open" class="ori-panel" role="dialog" aria-label="奥粒助手">
      <div class="ori-head">
        <svg viewBox="0 0 64 64" width="28" height="28" aria-hidden="true">
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
            {{ busy ? "回答中…" : providerOnline ? providerLabel + " · 对话中" : "离线小奥 · 规则应答" }}
          </span>
        </div>
        <button class="ori-head-btn" title="新对话（清空记录）" aria-label="开始新对话" @click="onResetEvent">＋</button>
        <button class="ori-head-btn" title="收起 (Esc)" aria-label="收起奥粒面板" @click="open = false">✕</button>
      </div>

      <div class="ori-list" ref="listEl" role="log" aria-live="polite" aria-label="奥粒对话记录" @click="onContentClick">
        <template v-for="(m, i) in msgs" :key="i">
          <!-- 用户：右侧蓝泡 -->
          <div v-if="m.role === 'user'" class="ori-msg user">
            <div class="ori-bubble">{{ m.content }}</div>
          </div>
          <!-- 奥粒：全宽 markdown 流（ZCode 式 transcript） -->
          <div v-else class="ori-msg assistant">
            <div class="ori-msg-head">
              <svg viewBox="0 0 64 64" width="16" height="16" aria-hidden="true">
                <circle cx="32" cy="36" r="24" fill="#4f8ae8" />
                <circle cx="25" cy="34" r="5" fill="#f4f8ff" />
                <circle cx="39" cy="34" r="5" fill="#f4f8ff" />
                <circle cx="26" cy="35" r="2.2" fill="#12203c" />
                <circle cx="40" cy="35" r="2.2" fill="#12203c" />
              </svg>
              <span class="ori-name">奥粒</span>
              <span v-if="m.rule" class="ori-ruletag">规则模式</span>
              <span class="ori-msg-time">{{ m.ts ? new Date(m.ts).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" }) : "" }}</span>
              <span class="ori-msg-act">
                <button v-if="m.content && !(busy && i === msgs.length - 1)" class="ori-copy" title="复制全文" @click="copyMsg($event, m)">⎘</button>
              </span>
            </div>
            <!-- 思考中（尚无内容）：状态行 + 计时 -->
            <div v-if="!m.content" class="ori-thinking">
              <span class="ori-spinner" aria-hidden="true"></span>
              <span>思考中{{ elapsed ? " · " + elapsed : "" }}</span>
            </div>
            <div v-else class="ori-md" v-html="renderMarkdown(m.content)"></div>
            <span v-if="busy && i === msgs.length - 1 && m.content" class="ori-caret"></span>
          </div>
        </template>
      </div>

      <div class="ori-quick" v-if="msgs.length <= 2">
        <button v-for="a in QUICK_ACTIONS" :key="a.label" @click="useQuick(a)">{{ a.label }}</button>
        <button class="ori-nav" @click="goAndNote('/new-game', '向导已打开 ✅ 五步走到黑,卡住随时叫我。')">➡️ 去新建游戏</button>
        <button class="ori-nav" @click="goAndNote('/models', '模型中心已打开 ✅ 拿不准就选免费档先跑通。')">➡️ 去模型中心</button>
      </div>

      <div class="ori-composer">
        <textarea
          ref="inputEl"
          v-model="input"
          rows="1"
          placeholder="问奥粒…(Enter 发送,Shift+Enter 换行)"
          @input="autoGrow"
          @keydown.enter.exact.prevent="send()"
        ></textarea>
        <div class="ori-composer-bar">
          <span class="ori-chip small" :class="providerOnline ? 'on' : 'off'">
            <span class="status-dot" :class="providerOnline ? 'on' : ''"></span>
            {{ providerOnline ? providerLabel : "规则模式" }}
          </span>
          <span v-if="busy" class="ori-elapsed mono">{{ elapsed }}</span>
          <span style="flex: 1"></span>
          <button v-if="busy" class="ori-stop" aria-label="停止生成" title="停止" @click="stop">
            <span class="ori-stop-sq"></span>停止
          </button>
          <button v-else class="ori-send" :disabled="!input.trim()" aria-label="发送给奥粒" @click="send()">➤</button>
        </div>
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
.ori-fab:focus-visible { outline: 2px solid #7eb8ff; outline-offset: 3px; border-radius: 50%; }
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
  border-radius: 50%; border: 2px solid #0b0b0e;
}
.ori-status-dot.on { background: #3fd68f; }
.ori-status-dot.off { background: #71717c; }

.ori-panel {
  position: fixed; right: 0; top: 0; bottom: 0; width: 420px; max-width: 100vw;
  z-index: 95; display: flex; flex-direction: column;
  background: #0e0e12; border-left: 1px solid #232329;
  box-shadow: -12px 0 36px rgba(0, 0, 0, 0.45);
}
.ori-drawer-enter-active, .ori-drawer-leave-active { transition: transform 0.22s ease, opacity 0.22s ease; }
.ori-drawer-enter-from, .ori-drawer-leave-to { transform: translateX(30px); opacity: 0; }

.ori-head {
  display: flex; align-items: center; gap: 10px; padding: 11px 14px;
  border-bottom: 1px solid #202027; background: #141419;
}
.ori-title { flex: 1; display: flex; align-items: center; gap: 8px; min-width: 0; }
.ori-title b { font-size: 14px; }
.ori-chip {
  font-size: 11px; padding: 1px 8px; border-radius: 9px;
  border: 1px solid #2c2c36; color: #a0a0ab; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 190px;
  display: inline-flex; align-items: center; gap: 5px;
}
.ori-chip.small { max-width: 170px; padding: 1px 7px; }
.ori-chip.on { color: #3fd68f; border-color: #215437; }
.ori-chip.off { color: #c9a6ff; border-color: #4a3a66; }
.ori-head-btn {
  background: none; border: none; color: #71717c; font-size: 14px; cursor: pointer;
  padding: 4px 8px; border-radius: 6px; line-height: 1;
}
.ori-head-btn:hover { color: #e9e9ee; background: #20202a; }

.ori-list { flex: 1; overflow-y: auto; padding: 16px 16px 10px; display: flex; flex-direction: column; gap: 14px; }

/* 用户消息：右侧蓝泡 */
.ori-msg.user { display: flex; justify-content: flex-end; }
.ori-bubble {
  max-width: 84%; padding: 8px 12px; border-radius: 12px; font-size: 13px; line-height: 1.65;
  white-space: pre-wrap; word-break: break-word;
  background: #2e5fd0; color: #f0f5ff; border-bottom-right-radius: 4px;
}

/* 奥粒消息：全宽 transcript（ZCode 式） */
.ori-msg.assistant { display: flex; flex-direction: column; }
.ori-msg-head { display: flex; align-items: center; gap: 6px; margin-bottom: 5px; color: #74747f; }
.ori-name { font-size: 12px; font-weight: 600; color: #9fd0ff; }
.ori-msg-time { font-size: 10.5px; color: #5b6379; font-family: var(--mono, monospace); }
.ori-msg-act { margin-left: auto; opacity: 0; transition: opacity .12s ease; }
.ori-msg.assistant:hover .ori-msg-act { opacity: 1; }
.ori-copy {
  background: none; border: none; color: #71717c; cursor: pointer; font-size: 13px; padding: 2px 6px; border-radius: 5px;
}
.ori-copy:hover { color: #e9e9ee; background: #20202a; }
.ori-ruletag { font-size: 10px; color: #c9a6ff; border: 1px solid #4a3a66; border-radius: 7px; padding: 0 5px; }

/* markdown 渲染区 */
.ori-md { font-size: 13px; line-height: 1.7; color: #d4d4dc; word-break: break-word; }
.ori-md :deep(p) { margin: 0 0 8px; }
.ori-md :deep(p:last-child) { margin-bottom: 0; }
.ori-md :deep(.md-h) { font-weight: 700; color: #e8eaf2; margin: 10px 0 6px; font-size: 13.5px; }
.ori-md :deep(.md-h3) { font-size: 14.5px; }
.ori-md :deep(ul), .ori-md :deep(ol) { margin: 2px 0 8px; padding-left: 20px; display: flex; flex-direction: column; gap: 3px; }
.ori-md :deep(blockquote) {
  margin: 4px 0 8px; padding: 6px 10px; border-left: 3px solid #3a3a44;
  background: rgba(255, 255, 255, .04); border-radius: 0 6px 6px 0; color: #b2bacf;
}
.ori-md :deep(.md-code) {
  font-family: var(--mono, monospace); font-size: 12px; background: #09090b;
  border: 1px solid #232329; border-radius: 5px; padding: 1px 5px; color: #9fd0ff;
}
.ori-md :deep(.md-pre) {
  margin: 6px 0 10px; border: 1px solid #232329; border-radius: 9px; overflow: hidden; background: #09090b;
}
.ori-md :deep(.md-pre-bar) {
  display: flex; align-items: center; justify-content: space-between;
  padding: 4px 10px; background: #141419; border-bottom: 1px solid #232329;
  font-size: 11px; color: #74747f; font-family: var(--mono, monospace);
}
.ori-md :deep(.md-pre-copy) { cursor: pointer; border-radius: 4px; padding: 1px 7px; }
.ori-md :deep(.md-pre-copy:hover) { color: #9fd0ff; background: #20202a; }
.ori-md :deep(pre) {
  margin: 0; padding: 10px 12px; overflow-x: auto;
  font-family: var(--mono, monospace); font-size: 12px; line-height: 1.6; color: #c9c9d2;
  white-space: pre;
}

/* 思考中状态行 */
.ori-thinking { display: flex; align-items: center; gap: 8px; color: #74747f; font-size: 12.5px; padding: 4px 0; }
.ori-spinner {
  width: 13px; height: 13px; border-radius: 50%; flex: none;
  border: 2px solid #2c2c36; border-top-color: #4f8ae8;
  animation: ori-spin .8s linear infinite;
}
.ori-caret { display: inline-block; width: 7px; height: 14px; background: #6ea8fe; margin-top: 4px; animation: ori-caret 0.9s steps(2) infinite; align-self: flex-start; }
@keyframes ori-caret { 0%, 100% { opacity: 1; } 50% { opacity: 0; } }

.ori-quick { display: flex; flex-wrap: wrap; gap: 6px; padding: 10px 14px 0; }
.ori-quick button {
  font-size: 11.5px; color: #b2bacf; background: #17171c; border: 1px solid #2c2c36;
  border-radius: 14px; padding: 3px 10px; cursor: pointer; transition: all 0.12s ease;
}
.ori-quick button:hover { color: #7eb8ff; border-color: #3a3a46; }
.ori-quick .ori-nav { color: #3fd68f; border-color: #215437; }

/* composer：输入 + 状态条 + 发送/停止 */
.ori-composer { padding: 10px 14px 12px; }
.ori-composer textarea {
  width: 100%; resize: none; background: #09090b; color: #e8eaf2; border: 1px solid #2c2c36;
  border-radius: 11px; padding: 9px 12px; font-size: 13px; font-family: inherit; line-height: 1.55;
  outline: none; max-height: 150px; transition: border-color .12s ease;
}
.ori-composer textarea:focus { border-color: #4f8ae8; }
.ori-composer-bar { display: flex; align-items: center; gap: 8px; margin-top: 8px; }
.ori-elapsed { font-size: 11.5px; color: #74747f; }
.ori-send {
  height: 30px; min-width: 52px; border-radius: 8px; border: 1px solid #4f8ae8;
  background: #4f8ae8; color: #fff; font-size: 14px; cursor: pointer; transition: background .12s ease;
}
.ori-send:disabled { opacity: .35; cursor: default; }
.ori-send:not(:disabled):hover { background: #6ba1f2; }
.ori-stop {
  display: inline-flex; align-items: center; gap: 7px; height: 30px; padding: 0 12px;
  border-radius: 8px; border: 1px solid #a34d4d; background: #2a1418; color: #ff9aa8;
  font-size: 12.5px; cursor: pointer; transition: background .12s ease;
}
.ori-stop:hover { background: #3a1a20; }
.ori-stop-sq { width: 9px; height: 9px; border-radius: 2px; background: #ff9aa8; }

@media (max-width: 520px) { .ori-panel { width: 100vw; } }
</style>
