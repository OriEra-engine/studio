<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { nextTick, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { useRouter } from "vue-router";
import { api } from "../api";
import type { AgentSessionFull, AgentSessionMeta, AgentStreamEvent, DiffLine } from "../api";

/**
 * Agent 工作台（v0.13.1 Codex 形态重构）：
 * 三栏全页工作台 —— 左 rail 会话列表 + 居中宽幅会话流 + 底部悬浮 composer。
 * harness 逻辑与 agent.rs 对齐：工具调用循环、内联审批（diff）、计划卡、
 * 多会话持久化、流式批刷、看门狗（审批等待暂停计时）、规则降级。
 */

const router = useRouter();

/* ---------------- 块级数据结构（与 agent.rs 落盘 turns 对齐） ---------------- */
type ToolStatus = "running" | "ok" | "error" | "denied";
type Block =
  | { type: "text"; content: string }
  | { type: "tool"; id: string; name: string; args: string; status: ToolStatus; summary: string; durationMs?: number; detail?: string; open?: boolean }
  | { type: "plan"; steps: { step: string; status: string }[] }
  | { type: "approval"; id: string; tool: string; title: string; detail: string; diff: DiffLine[]; resolved?: boolean; approved?: boolean }
  | { type: "choices"; options: { label: string; detail: string }[]; picked?: string }
  | { type: "error"; message: string }
  | { type: "notice"; content: string };
type Turn = { role: "user"; text: string } | { role: "assistant"; blocks: Block[] };

/** Codex 式工具动词短语：icon 后一句「读取 story.json · oriera-aigame」 */
function toolVerb(name: string, args: string): string {
  let a: any = {};
  try { a = JSON.parse(args || "{}"); } catch { /* 半截流式参数 */ }
  switch (name) {
    case "list_projects": return "列出项目";
    case "read_project": return a.id ? `读取项目 ${a.id}` : "读取项目";
    case "read_project_file": return `读取 ${a.file ?? "文件"}${a.id ? " · " + a.id : ""}`;
    case "write_project_file": return `写入 ${a.file ?? "文件"}${a.id ? " · " + a.id : ""}`;
    case "list_templates": return "列出玩法模板";
    case "list_assets": return "列出素材";
    case "get_workbench_status": return "查询工作台状态";
    case "update_plan": return "更新计划";
    case "propose_choices": return "给出选项";
    default: return name;
  }
}

const busy = ref(false);
const busyState = ref<"idle" | "thinking" | "streaming" | "tool" | "approval">("idle");
const input = ref("");
const providerLabel = ref("连接中…");
const providerOnline = ref(false);
const turns = ref<Turn[]>([]);
const sessions = ref<AgentSessionMeta[]>([]);
const session = ref<AgentSessionFull | null>(null);
const usage = ref<{ contextChars: number; turns: number } | null>(null);
const monthCalls = ref(0);
const listEl = ref<HTMLElement>();
const inputEl = ref<HTMLTextAreaElement>();
const elapsed = ref("");
const railCollapsed = ref(localStorage.getItem("oe.awRail") === "0");
const renamingId = ref("");
const renameDraft = ref("");

/** 本会话始终允许的工具（Codex 式「always allow」，仅内存，切会话重置） */
let allowAlways = new Set<string>();

let curTaskId = "";
let streamBuf = "";
let streamTimer: number | undefined;
let liveBlocks: Block[] | null = null;
let curTextBlock: (Block & { type: "text" }) | null = null;
let curPlanBlock: (Block & { type: "plan" }) | null = null;
const toolBlockById = new Map<string, Block>();

let elapsedTimer: number | undefined;
let busyStart = 0;
let pausedMs = 0;
let pauseStart = 0;

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

/* ---------- 规则降级应答 ---------- */
function ruleReply(q: string): string {
  const t = q.toLowerCase();
  const has = (...ks: string[]) => ks.some((k) => t.includes(k));
  if (has("你是谁", "你能做什么", "帮助", "help"))
    return "我是奥粒,住在这个工作台里的一颗元粒子 ✨ 能帮你:\n1. 带你五步做出一个能玩的游戏(新建游戏向导)\n2. 帮你选模型(云端按量 / Ollama 本地免费)\n3. 直接动手改剧情:说「把某某项目的第一拍改得更悬疑」,我改完给你看 diff\n4. 随时 Ctrl+K 唤起命令面板,键盘直达任何页面\n先从哪个开始?";
  if (has("第一个", "新手", "从零", "怎么做游戏", "做游戏", "新游戏"))
    return "三分钟出第一个游戏:\n1. 左侧「新建游戏」\n2. 选一个玩法模板(15 个任选,不确定就选「咖啡店经营」)\n3. 选叙事框架(或跳过)\n4. 点「AI 生成」剧情——没配模型就用默认剧情,一样能玩\n5. 起名 → 生成 → 「我的项目」里点运行\n做完回来告诉我,我陪你迭代!";
  if (has("模型", "api", "key", "ollama", "配置"))
    return "模型中心两条路:\n· 云端(推荐):智谱 glm-4-flash 免费档 / DeepSeek / Kimi / 豆包,注册拿 key 填进去即可\n· 本地免费:装 Ollama → 拉一个模型(模型中心一键 serve/pull)\n配好后点「测试连接」。不想配也行,所有功能都有离线兜底。去「模型中心」?";
  if (has("剧情", "节拍", "重写", "续写", "卡住", "写不下"))
    return "剧情的事交给我:\n1. 「我的项目」→ 打开项目 → 剧情编辑器\n2. 卡住的那一拍 → 「AI 重写」,可加要求\n3. 配置模型后还能直接对我说「把〈项目名〉的第一拍改得更悬疑」,我会给出 diff 让你确认\n4. 改坏了?历史备份保留 10 份";
  if (has("快捷键", "键盘", "命令"))
    return "键盘党福利:\n· Ctrl+K —— 命令面板\n· Ctrl+B —— 折叠/展开左侧栏\n· Esc —— 关掉弹层\n试试 Ctrl+K 输入「模型」?";
  if (has("导出", "备份", "存档"))
    return "项目数据安全:\n· 「我的项目」→ 项目详情里有存档导出/导入\n· 剧情编辑器自动备份最近 10 版\n· 项目目录可一键打开(reveal)自己拷贝";
  return "这个问题我在离线规则模式下答不好 😅\n两条路:\n1. 「模型中心」配一个模型(免费档即可),我立刻变成能动手的 Agent\n2. 先用快捷指令:带做游戏 / 选模型 / 剧情卡住了";
}

/* ---------- 会话 ---------- */
async function refreshSessions() {
  try {
    sessions.value = await api.agentSessionList();
  } catch {
    sessions.value = [];
  }
}
function fmtRel(ts: number) {
  if (!ts) return "";
  const diff = Date.now() / 1000 - ts;
  if (diff < 60) return "刚刚";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
  const d = new Date(ts * 1000);
  return `${d.getMonth() + 1} 月 ${d.getDate()} 日`;
}
function railToggle() {
  railCollapsed.value = !railCollapsed.value;
  localStorage.setItem("oe.awRail", railCollapsed.value ? "0" : "1");
}

async function switchSession(id: string) {
  if (busy.value) return;
  try {
    const s = await api.agentSessionLoad(id);
    session.value = s;
    allowAlways = new Set();
    turns.value = s.turns.map(t =>
      t.role === "user"
        ? { role: "user", text: String((t as any).text ?? "") }
        : { role: "assistant", blocks: mapStoredBlocks((t as any).blocks ?? []) }
    );
    usage.value = null;
    nextTick(() => { scrollBottom(); inputEl.value?.focus(); });
  } catch (e) {
    pushLocalNotice(`会话加载失败：${String(e).slice(0, 80)}`);
  }
}

function mapStoredBlocks(raw: unknown[]): Block[] {
  return (raw as any[]).map(b => {
    if (b?.type === "text") return { type: "text", content: String(b.content ?? "") };
    if (b?.type === "tool")
      return {
        type: "tool", id: `stored-${Math.random().toString(36).slice(2)}`,
        name: String(b.name ?? ""), args: prettyJson(b.args),
        status: (b.approved === false && b.ok === false ? "denied" : b.ok ? "ok" : "error") as ToolStatus,
        summary: String(b.summary ?? ""), durationMs: b.duration_ms, detail: prettyJson(b.detail), open: false,
      };
    if (b?.type === "plan") return { type: "plan", steps: Array.isArray(b.steps) ? b.steps : [] };
    if (b?.type === "choices")
      return { type: "choices", options: Array.isArray(b.options) ? b.options : [], picked: b.picked ? String(b.picked) : undefined };
    if (b?.type === "approval")
      return { type: "approval", id: `stored-${Math.random().toString(36).slice(2)}`, tool: String(b.tool ?? ""), title: String(b.title ?? ""), detail: "", diff: [], resolved: true, approved: !!b.approved };
    if (b?.type === "error") return { type: "error", message: String(b.message ?? "") };
    if (b?.type === "notice") return { type: "notice", content: String(b.content ?? "") };
    return { type: "notice", content: "（未知块）" };
  });
}
function prettyJson(v: unknown) {
  try { return typeof v === "string" ? v : JSON.stringify(v ?? {}, null, 2); } catch { return String(v); }
}

async function newSession(hint?: string) {
  if (busy.value) return;
  try {
    const s = await api.agentSessionNew(hint);
    session.value = s;
    turns.value = [];
    usage.value = null;
    allowAlways = new Set();
    await refreshSessions();
    nextTick(() => inputEl.value?.focus());
  } catch (e) {
    pushLocalNotice(`新建会话失败：${String(e).slice(0, 80)}`);
  }
}
async function deleteSession(id: string) {
  try {
    await api.agentSessionDelete(id);
    if (session.value?.id === id) {
      session.value = null;
      turns.value = [];
    }
    await refreshSessions();
  } catch { /* 静默 */ }
}
async function saveRename() {
  const id = renamingId.value;
  const title = renameDraft.value.trim();
  renamingId.value = "";
  if (!id || !title) return;
  try {
    await api.agentSessionRename(id, title);
    if (session.value?.id === id) session.value.title = title;
    await refreshSessions();
  } catch { /* 静默 */ }
}

function pushLocalNotice(content: string) {
  turns.value.push({ role: "assistant", blocks: [{ type: "notice", content }] });
  scrollBottom();
}

/* ---------- 滚动 ---------- */
async function scrollBottom() {
  await nextTick();
  const el = listEl.value;
  if (el) el.scrollTop = el.scrollHeight;
}

/* ---------- 看门狗与计时（审批等待时暂停 90s 计数） ---------- */
function stopElapsed() {
  if (elapsedTimer !== undefined) {
    clearInterval(elapsedTimer);
    elapsedTimer = undefined;
  }
  elapsed.value = "";
}
function startElapsed() {
  busyStart = Date.now();
  pausedMs = 0;
  pauseStart = 0;
  elapsed.value = "0s";
  stopElapsed();
  elapsedTimer = window.setInterval(() => {
    const s = Math.floor((Date.now() - busyStart - pausedMs) / 1000);
    if (busyState.value === "approval") {
      if (pauseStart === 0) pauseStart = Date.now();
      elapsed.value = s < 60 ? `${s}s` : `${Math.floor(s / 60)}m${s % 60}s`;
      return;
    }
    if (pauseStart > 0) {
      pausedMs += Date.now() - pauseStart;
      pauseStart = 0;
    }
    if (s >= 90) {
      watchdogTimeout();
      return;
    }
    elapsed.value = s < 60 ? `${s}s` : `${Math.floor(s / 60)}m${s % 60}s`;
  }, 500);
}
function watchdogTimeout() {
  flushStream();
  finishTurn();
  const last = lastAssistantTurn();
  if (last) last.blocks.push({ type: "notice", content: "（90 秒无响应，已自动停止——可重试）" });
  scrollBottom();
}
function lastAssistantTurn(): (Turn & { role: "assistant" }) | null {
  for (let i = turns.value.length - 1; i >= 0; i--) {
    const t = turns.value[i] as any;
    if (t.role === "assistant") return t;
  }
  return null;
}

/* ---------- 流式批刷 ---------- */
function flushStream() {
  if (streamTimer !== undefined) {
    clearInterval(streamTimer);
    streamTimer = undefined;
  }
  if (!curTextBlock || !streamBuf) { streamBuf = ""; return; }
  const el = listEl.value;
  const stick = !el || el.scrollHeight - el.scrollTop - el.clientHeight < 120;
  curTextBlock.content += streamBuf;
  streamBuf = "";
  if (stick) void scrollBottom();
}
function ensureTextBlock(): Block & { type: "text" } {
  if (!curTextBlock) {
    const b: Block = { type: "text", content: "" };
    curTextBlock = addLiveBlock(b) as Block & { type: "text" };
  }
  return curTextBlock;
}
function addLiveBlock(b: Block): Block {
  if (!liveBlocks) {
    const arr = reactive<Block[]>([]);
    turns.value.push({ role: "assistant", blocks: arr });
    liveBlocks = arr;
  }
  const arr = liveBlocks as any;
  arr.push(b);
  return arr[arr.length - 1] as Block;
}

/* ---------- Agent 事件 ---------- */
function handleEvent(ev: AgentStreamEvent) {
  if (!busy.value) return;
  switch (ev.event) {
    case "delta": {
      providerOnline.value = true;
      busyState.value = "streaming";
      streamBuf += ev.text;
      ensureTextBlock();
      if (streamTimer === undefined) streamTimer = window.setInterval(flushStream, 100);
      break;
    }
    case "tool_start": {
      flushStream();
      curTextBlock = null;
      busyState.value = "tool";
      const b: Block = { type: "tool", id: ev.id, name: ev.name, args: "", status: "running", summary: "" };
      toolBlockById.set(ev.id, addLiveBlock(b));
      break;
    }
    case "tool_args": {
      const tb = toolBlockById.get(ev.id) as any;
      if (tb) tb.args += ev.fragment;
      break;
    }
    case "tool_end": {
      const tb = toolBlockById.get(ev.id) as any;
      if (tb) {
        tb.status = ev.ok ? "ok" : "error";
        tb.summary = ev.summary;
        tb.durationMs = ev.duration_ms;
        if (!ev.ok && /denied|拒绝/.test(ev.summary)) tb.status = "denied";
      }
      busyState.value = busy.value ? "streaming" : "idle";
      break;
    }
    case "plan": {
      if (curPlanBlock) {
        curPlanBlock.steps = ev.steps;
      } else {
        const b: Block = { type: "plan", steps: ev.steps };
        curPlanBlock = addLiveBlock(b) as Block & { type: "plan" };
      }
      break;
    }
    case "approval_request": {
      if (allowAlways.has(ev.tool)) {
        void api.agentApprove(ev.id, true).catch(() => { /* 后端兜底 */ });
        break;
      }
      busyState.value = "approval";
      const b: Block = { type: "approval", id: ev.id, tool: ev.tool, title: ev.title, detail: ev.detail, diff: ev.diff ?? [] };
      addLiveBlock(b);
      const el = listEl.value;
      if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 200) void scrollBottom();
      break;
    }
    case "choices": {
      flushStream();
      curTextBlock = null;
      const b: Block = { type: "choices", options: ev.options ?? [] };
      addLiveBlock(b);
      const el = listEl.value;
      if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 160) void scrollBottom();
      break;
    }
    case "usage": {
      usage.value = { contextChars: ev.context_chars, turns: ev.turns, tokens_in: ev.tokens_in, tokens_out: ev.tokens_out };
      break;
    }
    case "tool_unsupported": {
      curTextBlock = null;
      addLiveBlock({ type: "notice", content: "当前模型不支持工具调用，已切换纯对话模式" });
      break;
    }
    case "done": {
      flushStream();
      if (ev.result?.session_id && session.value && session.value.id !== ev.result.session_id) {
        api.agentSessionLoad(ev.result.session_id).then(s => { session.value = s; }).catch(() => {});
      }
      finishTurn();
      break;
    }
    case "error": {
      flushStream();
      if (/已取消/.test(ev.message)) {
        addLiveBlock({ type: "notice", content: "已停止" });
        finishTurn();
        return;
      }
      if (/模型中心|配置/.test(ev.message)) {
        providerOnline.value = false;
        addLiveBlock({ type: "notice", content: ev.message });
        finishTurn();
        return;
      }
      addLiveBlock({ type: "error", message: ev.message });
      finishTurn();
      break;
    }
  }
}

function finishTurn() {
  streamBuf = "";
  busy.value = false;
  busyState.value = "idle";
  stopElapsed();
  curTaskId = "";
  curTextBlock = null;
  curPlanBlock = null;
  liveBlocks = null;
  toolBlockById.clear();
  const last = turns.value[turns.value.length - 1] as any;
  if (last && last.role === "assistant" && Array.isArray(last.blocks) && last.blocks.length === 0) {
    turns.value.pop();
  }
  monthCalls.value += 1;
  refreshSessions();
  scrollBottom();
}

/* ---------- 发送 ---------- */
async function send(text?: string) {
  const q = (text ?? input.value).trim();
  if (!q || busy.value) return;
  input.value = "";
  autoGrow();
  busy.value = true;
  busyState.value = "thinking";
  providerOnline.value = true;
  turns.value.push({ role: "user", text: q });
  scrollBottom();

  if (!session.value) {
    try {
      session.value = await api.agentSessionNew(q.slice(0, 24));
      await refreshSessions();
    } catch (e) {
      busy.value = false;
      busyState.value = "idle";
      typeOut(ruleReply(q) + `\n\n(原因:${String(e).slice(0, 80)})`);
      return;
    }
  }

  const blocks: Block[] = reactive<Block[]>([]);
  turns.value.push({ role: "assistant", blocks });
  liveBlocks = blocks;
  curTextBlock = null;
  curPlanBlock = null;
  startElapsed();
  scrollBottom();

  try {
    const idP = api.agentRun(session.value.id, q, handleEvent);
    idP.then(id => { if (busy.value) curTaskId = id; }).catch(() => { /* invoke 级失败走 catch */ });
    await idP;
  } catch (e) {
    const detail = String(e);
    if (/模型中心|配置/.test(detail)) {
      providerOnline.value = false;
      addLiveBlock({ type: "notice", content: "小奥离线中：先在「模型中心」配置模型（免费档即可），配好我就是能动手的 Agent" });
    } else {
      addLiveBlock({ type: "error", message: detail.slice(0, 200) });
    }
    finishTurn();
  }
}

/** 规则模式打字机 */
function typeOut(full: string) {
  const blocks: Block[] = reactive<Block[]>([]);
  turns.value.push({ role: "assistant", blocks });
  liveBlocks = blocks;
  const b = addLiveBlock({ type: "text", content: "" }) as Block & { type: "text" };
  curTextBlock = b;
  let i = 0;
  const timer = window.setInterval(() => {
    i = Math.min(full.length, i + 2);
    b.content = full.slice(0, i);
    scrollBottom();
    if (i >= full.length) {
      clearInterval(timer);
      curTextBlock = null;
      liveBlocks = null;
      monthCalls.value += 1;
    }
  }, 16);
}

function retryLast() {
  for (let i = turns.value.length - 1; i >= 0; i--) {
    const t = turns.value[i];
    if (t.role === "user") {
      const last = turns.value[turns.value.length - 1] as any;
      if (last?.role === "assistant") {
        const errIdx = last.blocks.findIndex((b: Block) => b.type === "error");
        if (errIdx >= 0) last.blocks.splice(errIdx, 1);
        if (last.blocks.length === 0) turns.value.pop();
      }
      void send(t.text);
      return;
    }
  }
}

/* ---------- 审批 ---------- */
function approve(id: string, mode: "once" | "always" | "deny", tool: string) {
  const card = findApproval(id);
  if (card) {
    card.resolved = true;
    card.approved = mode !== "deny";
  }
  if (pauseStart > 0) {
    pausedMs += Date.now() - pauseStart;
    pauseStart = 0;
  }
  busyState.value = busy.value ? "streaming" : "idle";
  void api.agentApprove(id, mode !== "deny").catch(() => { /* 超时/已取消 */ });
  if (mode === "always") allowAlways.add(tool);
}
/** 点选选项卡：标记已选并作为新消息发送（A2UI choices） */
function pickChoice(b: Block & { type: "choices" }, label: string) {
  if (b.picked) return;
  b.picked = label;
  void send(label);
}
function findApproval(id: string): (Block & { type: "approval" }) | null {
  for (const t of turns.value) {
    if (t.role !== "assistant") continue;
    for (const b of t.blocks as any[]) {
      if (b.type === "approval" && b.id === id) return b;
    }
  }
  return null;
}

/* ---------- 停止 ---------- */
async function stop() {
  if (!busy.value) return;
  if (curTaskId) {
    try { await api.cancelAi(curTaskId); } catch { /* 后端兜底 */ }
  } else {
    busy.value = false;
    busyState.value = "idle";
    stopElapsed();
  }
}

/* ---------- 快捷指令 ---------- */
const QUICK_ACTIONS: { label: string; text: string }[] = [
  { label: "🎮 带我做第一个游戏", text: "带我从零做一个游戏,我是新手" },
  { label: "📂 看看我的项目", text: "列出我的所有项目,顺便说说哪个最值得继续打磨" },
  { label: "📝 改剧情试试", text: "帮我把第一个项目的第一拍改得更悬疑一点" },
  { label: "💡 你能做什么", text: "你能做什么?会改我的文件吗?" },
];
function useQuick(a: { label: string; text: string }) {
  void send(a.text);
}
function goAndNote(path: string, note: string) {
  router.push(path);
  pushLocalNotice(note);
}

/* ---------- 复制 ---------- */
function copyText(text: string, ok?: () => void) {
  try {
    navigator.clipboard?.writeText(text);
    ok?.();
  } catch { /* 静默 */ }
}
function copyBlock(e: MouseEvent, b: Block & { type: "text" }) {
  const btn = e.target as HTMLElement;
  copyText(b.content, () => {
    if (btn) {
      btn.textContent = "✓";
      setTimeout(() => (btn.textContent = "⎘"), 1000);
    }
  });
}
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

function usageText() {
  if (!usage.value) return "";
  const k = usage.value.contextChars / 1000;
  const u = usage.value as { contextChars: number; turns: number; tokens_in?: number; tokens_out?: number };
  const tok = u.tokens_in != null || u.tokens_out != null
    ? ` · ↑${fmtTok(u.tokens_in)}↓${fmtTok(u.tokens_out)} tok`
    : "";
  return `上下文 ≈${k < 10 ? k.toFixed(1) : Math.round(k)}k 字 · ${usage.value.turns} 轮${tok}`;
}
function fmtTok(n?: number) {
  if (n == null) return "?";
  return n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n);
}

function autoGrow() {
  const el = inputEl.value;
  if (!el) return;
  el.style.height = "auto";
  el.style.height = Math.min(el.scrollHeight, 200) + "px";
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
  try {
    const led = await api.chatLedgerMonth();
    monthCalls.value = Object.values(led).reduce((a, b) => a + (Number(b) || 0), 0);
  } catch { /* 静默 */ }
}

function onNewEvent() {
  void newSession();
}

onMounted(async () => {
  refreshStatus();
  refreshSessions();
  window.addEventListener("oriera:agent:new", onNewEvent);
  nextTick(() => inputEl.value?.focus());
});
onBeforeUnmount(() => {
  stopElapsed();
  window.removeEventListener("oriera:agent:new", onNewEvent);
});
</script>

<template>
  <div class="aw-root">
    <!-- ============ 左 rail：会话列表（Codex 式常驻） ============ -->
    <aside class="aw-rail" :class="{ collapsed: railCollapsed }">
      <div class="aw-rail-head">
        <svg viewBox="0 0 64 64" width="20" height="20" aria-hidden="true">
          <circle cx="32" cy="36" r="21" fill="#4f8ae8" />
          <circle cx="26.5" cy="34" r="4.2" fill="#f4f8ff" />
          <circle cx="38.5" cy="34" r="4.2" fill="#f4f8ff" />
          <circle cx="27.4" cy="35" r="1.9" fill="#12203c" />
          <circle cx="39.4" cy="35" r="1.9" fill="#12203c" />
          <path d="M27 44 Q32 48 37 44" stroke="#12203c" stroke-width="2" fill="none" stroke-linecap="round" />
          <circle cx="32" cy="8" r="2.6" fill="#ffd86b" />
        </svg>
        <span v-if="!railCollapsed" class="aw-rail-name">奥粒 Agent</span>
        <span style="flex:1"></span>
        <button class="aw-rail-btn" :title="railCollapsed ? '展开列表' : '收起列表'" @click="railToggle">
          {{ railCollapsed ? "»" : "«" }}
        </button>
      </div>

      <button v-if="!railCollapsed" class="aw-new-task" @click="newSession()">＋ 新任务</button>

      <div v-if="!railCollapsed" class="aw-rail-foot">
        <button @click="router.push('/agent/state')">⚙ 奥粒状态</button>
        <button @click="router.push('/agent/lab')">🧪 进化实验室</button>
      </div>
      <div v-if="!railCollapsed" class="aw-rail-list">
        <div v-if="!sessions.length" class="aw-rail-empty">还没有历史任务</div>
        <div
          v-for="s in sessions" :key="s.id"
          class="aw-task" :class="{ cur: session?.id === s.id }"
          @click="switchSession(s.id)"
        >
          <template v-if="renamingId === s.id">
            <input v-model="renameDraft" class="aw-rename" @keydown.enter.prevent="saveRename" @keydown.esc.prevent="renamingId = ''" @blur="saveRename" />
          </template>
          <template v-else>
            <span class="aw-task-name">{{ s.title }}</span>
            <span class="aw-task-meta">{{ s.turns }} 轮 · {{ fmtRel(s.updated) }}</span>
          </template>
          <span class="aw-task-act" @click.stop>
            <button title="重命名" @click.stop="renamingId = s.id; renameDraft = s.title">✎</button>
            <button title="删除" @click.stop="deleteSession(s.id)">🗑</button>
          </span>
        </div>
      </div>
    </aside>

    <!-- ============ 主区：居中会话流 + 悬浮 composer ============ -->
    <main class="aw-main">
      <div class="aw-scroll" ref="listEl" role="log" aria-live="polite" aria-label="Agent 任务记录" @click="onContentClick">
        <div class="aw-thread">
          <!-- 空状态：Codex 式居中问候 + 建议 -->
          <template v-if="turns.length === 0">
            <div class="aw-greet">
              <div class="aw-greet-title">今天想做什么？</div>
              <div class="aw-greet-sub">派活给奥粒——查项目、改剧情、出计划；写文件前会先给你看 diff。</div>
            </div>
          </template>

          <template v-for="(t, ti) in turns" :key="ti">
            <!-- 用户：左对齐浅色圆角块（Codex 式，非气泡） -->
            <div v-if="t.role === 'user'" class="aw-user">{{ t.text }}</div>

            <!-- 奥粒：无头平铺，块级内容 -->
            <div v-else class="aw-asst">
              <template v-for="(b, bi) in t.blocks" :key="bi">
                <div v-if="b.type === 'text'" class="aw-text-wrap">
                  <div class="aw-md" v-html="renderMarkdown(b.content)"></div>
                  <button
                    v-if="b.content && !(busy && ti === turns.length - 1 && bi === t.blocks.length - 1)"
                    class="aw-copy" title="复制" @click="copyBlock($event, b)"
                  >⎘</button>
                  <span v-if="busy && ti === turns.length - 1 && bi === t.blocks.length - 1" class="aw-caret"></span>
                </div>

                <!-- 工具行：安静单行，无框，点击展开 -->
                <div v-else-if="b.type === 'tool'" class="aw-tool" :class="b.status" @click="b.open = !b.open">
                  <div class="aw-tool-row">
                    <span v-if="b.status === 'running'" class="aw-tool-spin"></span>
                    <span v-else class="aw-tool-ico">{{ b.status === "ok" ? "✓" : b.status === "denied" ? "⊘" : "✗" }}</span>
                    <span class="aw-tool-verb">{{ toolVerb(b.name, b.args) }}</span>
                    <span v-if="b.status === 'error'" class="aw-tool-sum">{{ b.summary }}</span>
                    <span v-if="b.durationMs != null && b.durationMs >= 300" class="aw-tool-dur">{{ (b.durationMs / 1000).toFixed(1) }}s</span>
                    <span class="aw-tool-chev">{{ b.open ? "▾" : "▸" }}</span>
                  </div>
                  <template v-if="b.open">
                    <div v-if="b.args" class="aw-tool-kv"><span class="aw-k">参数</span><pre class="aw-pre">{{ b.args }}</pre></div>
                    <div v-if="b.detail" class="aw-tool-kv"><span class="aw-k">结果</span><pre class="aw-pre">{{ b.detail }}</pre></div>
                  </template>
                </div>

                <!-- 计划：单色清单 -->
                <div v-else-if="b.type === 'plan'" class="aw-plan">
                  <div v-for="(s, si) in b.steps" :key="si" class="aw-plan-step">
                    <span class="aw-plan-dot" :class="s.status"></span>
                    <span :class="{ done: s.status === 'done' }">{{ s.step }}</span>
                  </div>
                </div>

                <!-- 选项卡（A2UI）：点击即发送 -->
              <div v-else-if="b.type === 'choices'" class="aw-choices">
                <button
                  v-for="opt in b.options" :key="opt.label"
                  class="aw-choice" :class="{ picked: b.picked === opt.label, dim: !!b.picked && b.picked !== opt.label }"
                  :disabled="!!b.picked || busy" @click.stop="pickChoice(b, opt.label)"
                >
                  <b>{{ opt.label }}</b>
                  <span v-if="opt.detail" class="aw-choice-detail">{{ opt.detail }}</span>
                </button>
              </div>

              <!-- 审批卡（Codex 里唯一带边框的卡） -->
                <div v-else-if="b.type === 'approval'" class="aw-ap" :class="{ resolved: b.resolved }">
                  <div class="aw-ap-title">✋ 允许{{ b.title }}？</div>
                  <div class="aw-ap-sub">写入前自动备份旧版（保留 10 份），写入后热重载运行窗口。</div>
                  <div v-if="b.diff.length" class="aw-diff">
                    <div v-for="(l, li) in b.diff" :key="li" class="aw-diff-line" :class="l.kind">
                      <span class="aw-diff-sign">{{ l.kind === "add" ? "+" : l.kind === "del" ? "−" : " " }}</span>
                      <span class="aw-diff-text">{{ l.text }}</span>
                    </div>
                  </div>
                  <div v-if="!b.resolved" class="aw-ap-btns">
                    <button class="ap-allow" @click.stop="approve(b.id, 'once', b.tool)">允许</button>
                    <button class="ap-always" @click.stop="approve(b.id, 'always', b.tool)">本任务始终允许</button>
                    <button class="ap-deny" @click.stop="approve(b.id, 'deny', b.tool)">拒绝</button>
                  </div>
                  <div v-else class="aw-ap-state" :class="b.approved ? 'ok' : 'no'">
                    {{ b.approved ? "✓ 已允许" : "⊘ 已拒绝" }}
                  </div>
                </div>

                <!-- 错误 + 重试 -->
                <div v-else-if="b.type === 'error'" class="aw-err">
                  <span class="aw-err-text">{{ b.message }}</span>
                  <button class="aw-retry" @click="retryLast">重试</button>
                </div>

                <div v-else-if="b.type === 'notice'" class="aw-notice">{{ b.content }}</div>
              </template>

              <!-- 工作中状态行 -->
              <div v-if="busy && ti === turns.length - 1 && t.blocks.length === 0" class="aw-working">
                <span class="aw-spin"></span>
                <span>{{ busyState === "approval" ? "等待你审批…" : busyState === "tool" ? "正在执行工具" : "工作中" }}</span>
                <span v-if="elapsed" class="aw-working-elapsed">{{ elapsed }}</span>
              </div>
            </div>
          </template>
        </div>
      </div>

      <!-- 快捷指令（空会话时出现在 composer 上方） -->
      <div class="aw-chips" v-if="turns.length === 0">
        <button v-for="a in QUICK_ACTIONS" :key="a.label" @click="useQuick(a)">{{ a.label }}</button>
        <button class="aw-chip-nav" @click="goAndNote('/new-game', '向导已打开 ✅ 五步走到黑,卡住随时叫我。')">➡️ 去新建游戏</button>
        <button class="aw-chip-nav" @click="goAndNote('/models', '模型中心已打开 ✅ 拿不准就选免费档先跑通。')">➡️ 去模型中心</button>
      </div>

      <!-- 悬浮 composer（Codex 式大圆角卡） -->
      <div class="aw-composer-wrap">
        <div class="aw-composer" :class="{ focus: !!input }">
          <textarea
            ref="inputEl"
            v-model="input"
            rows="1"
            placeholder="给奥粒派活…比如「把第一个项目的第一拍改得更悬疑」"
            @input="autoGrow"
            @keydown.enter.exact.prevent="send()"
          ></textarea>
          <div class="aw-composer-bar">
            <span class="aw-pill" :class="providerOnline ? 'on' : 'off'">
              <span class="status-dot" :class="providerOnline ? 'on' : ''"></span>
              {{ providerOnline ? providerLabel : "规则模式" }}
            </span>
            <span v-if="usageText()" class="aw-usage">{{ usageText() }}</span>
            <span v-if="busy" class="aw-elapsed">{{ elapsed }}</span>
            <span style="flex:1"></span>
            <button v-if="busy" class="aw-stop" aria-label="停止" title="停止" @click="stop">
              <span class="aw-stop-sq"></span>
            </button>
            <button v-else class="aw-send" :disabled="!input.trim()" aria-label="发送" title="发送 (Enter)" @click="send()">↑</button>
          </div>
        </div>
        <div class="aw-hint">Enter 发送 · Shift+Enter 换行 · 本月已调用 {{ monthCalls }} 次</div>
      </div>
    </main>
  </div>
</template>

<style scoped>
.aw-root { display: flex; height: 100%; min-height: 0; }

/* ============ 左 rail ============ */
.aw-rail {
  width: 264px; flex: none; display: flex; flex-direction: column;
  background: var(--bg-deep); border-right: 1px solid var(--line);
  padding: 10px; gap: 8px; min-height: 0;
}
.aw-rail.collapsed { width: 52px; padding: 10px 8px; }
.aw-rail-head { display: flex; align-items: center; gap: 8px; padding: 4px 6px 8px; }
.aw-rail-name { font-size: 13px; font-weight: 700; letter-spacing: .2px; }
.aw-rail-btn {
  background: none; border: none; color: var(--text-3); cursor: pointer;
  font-size: 13px; padding: 3px 6px; border-radius: 6px; line-height: 1;
}
.aw-rail-btn:hover { color: var(--text); background: var(--bg-hover); }
.aw-new-task {
  width: 100%; display: flex; align-items: center; justify-content: center; gap: 6px;
  background: none; border: 1px solid var(--line-strong); color: var(--text);
  font-size: 12.5px; font-weight: 600; border-radius: 10px; padding: 7px 0;
  cursor: pointer; transition: background .12s ease, border-color .12s ease;
}
.aw-new-task:hover { background: var(--bg-hover); border-color: #3a3a46; }
.aw-rail-list { flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 2px; margin-top: 2px; }
.aw-rail-empty { color: var(--text-3); font-size: 12px; padding: 10px 8px; }
.aw-task {
  position: relative; display: flex; flex-direction: column; gap: 2px;
  padding: 8px 10px; border-radius: 9px; cursor: pointer; transition: background .12s ease;
}
.aw-task:hover { background: var(--bg-hover); }
.aw-task.cur { background: #1a1a21; }
.aw-task-name {
  font-size: 12.5px; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding-right: 40px;
}
.aw-task-meta { font-size: 10.5px; color: var(--text-3); }
.aw-task-act { position: absolute; right: 8px; top: 8px; display: none; gap: 2px; }
.aw-task:hover .aw-task-act { display: inline-flex; }
.aw-task-act button {
  background: none; border: none; color: var(--text-3); cursor: pointer; font-size: 11px;
  padding: 2px 4px; border-radius: 5px;
}
.aw-task-act button:hover { color: var(--text); background: #26262e; }
.aw-rename {
  width: 100%; background: #0a0a0c; border: 1px solid var(--accent-strong); border-radius: 6px;
  color: var(--text); font-size: 12px; padding: 3px 8px; outline: none;
}

/* ============ 主区 ============ */
.aw-main { flex: 1; min-width: 0; display: flex; flex-direction: column; min-height: 0; position: relative; }
.aw-scroll { flex: 1; overflow-y: auto; min-height: 0; }
.aw-thread {
  max-width: 780px; margin: 0 auto; padding: 28px 24px 12px;
  display: flex; flex-direction: column; gap: 26px;
}

/* 空状态问候（Codex 式居中） */
.aw-greet { text-align: center; padding: 120px 0 12px; }
.aw-greet-title { font-size: 22px; font-weight: 700; letter-spacing: .2px; }
.aw-greet-sub { color: var(--text-3); font-size: 13px; margin-top: 8px; }

/* 用户消息：左对齐浅色圆角块 */
.aw-user {
  background: #17171d; border: 1px solid #1f1f27; border-radius: 14px;
  padding: 12px 16px; font-size: 13.5px; line-height: 1.65; color: #e6e6ee;
  white-space: pre-wrap; word-break: break-word;
}

/* 奥粒块 */
.aw-asst { display: flex; flex-direction: column; gap: 8px; }
.aw-text-wrap { position: relative; }
.aw-copy {
  position: absolute; right: -2px; top: -20px;
  background: none; border: none; color: var(--text-3); cursor: pointer; font-size: 13px;
  padding: 2px 6px; border-radius: 5px; opacity: 0; transition: opacity .12s ease;
}
.aw-text-wrap:hover .aw-copy { opacity: 1; }
.aw-copy:hover { color: var(--text); background: var(--bg-hover); }

.aw-md { font-size: 13.5px; line-height: 1.75; color: #d6d6de; word-break: break-word; }
.aw-md :deep(p) { margin: 0 0 10px; }
.aw-md :deep(p:last-child) { margin-bottom: 0; }
.aw-md :deep(.md-h) { font-weight: 700; color: #eceef4; margin: 14px 0 8px; font-size: 14px; }
.aw-md :deep(ul), .aw-md :deep(ol) { margin: 2px 0 10px; padding-left: 22px; display: flex; flex-direction: column; gap: 4px; }
.aw-md :deep(blockquote) {
  margin: 4px 0 10px; padding: 8px 12px; border-left: 3px solid #2e2e38;
  background: rgba(255, 255, 255, .03); border-radius: 0 8px 8px 0; color: #b0b6c6;
}
.aw-md :deep(.md-code) {
  font-family: var(--mono, monospace); font-size: 12px; background: #0a0a0d;
  border: 1px solid #22222a; border-radius: 5px; padding: 1px 5px; color: #9fc2f5;
}
.aw-md :deep(.md-pre) {
  margin: 8px 0 12px; border: 1px solid #22222a; border-radius: 10px; overflow: hidden; background: #0a0a0d;
}
.aw-md :deep(.md-pre-bar) {
  display: flex; align-items: center; justify-content: space-between;
  padding: 5px 12px; background: #121218; border-bottom: 1px solid #22222a;
  font-size: 11px; color: var(--text-3); font-family: var(--mono, monospace);
}
.aw-md :deep(.md-pre-copy) { cursor: pointer; border-radius: 4px; padding: 1px 7px; }
.aw-md :deep(.md-pre-copy:hover) { color: #9fc2f5; background: #1e1e26; }
.aw-md :deep(pre) {
  margin: 0; padding: 12px 14px; overflow-x: auto;
  font-family: var(--mono, monospace); font-size: 12.5px; line-height: 1.6; color: #c9c9d4;
  white-space: pre;
}

/* 工具行：安静、无框（Codex 式） */
.aw-tool { padding-left: 4px; }
.aw-tool-row {
  display: flex; align-items: center; gap: 8px; padding: 5px 8px; border-radius: 8px;
  cursor: pointer; transition: background .12s ease; user-select: none;
}
.aw-tool-row:hover { background: #141419; }
.aw-tool-ico { flex: none; font-size: 11px; color: var(--text-3); }
.aw-tool.ok .aw-tool-ico { color: var(--good); }
.aw-tool.error .aw-tool-ico { color: var(--danger); }
.aw-tool.denied .aw-tool-ico { color: #c9a6ff; }
.aw-tool-verb { font-size: 12.5px; color: var(--text-2); }
.aw-tool-sum { font-size: 11.5px; color: var(--danger); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.aw-tool-dur { font-size: 10.5px; color: var(--text-3); font-family: var(--mono, monospace); }
.aw-tool-chev { margin-left: auto; font-size: 9px; color: var(--text-3); opacity: .7; }
.aw-tool-spin {
  width: 11px; height: 11px; border-radius: 50%; flex: none;
  border: 2px solid #2a2a34; border-top-color: var(--text-2);
  animation: aw-spin .8s linear infinite;
}
@keyframes aw-spin { to { transform: rotate(360deg); } }
.aw-tool-kv { padding: 4px 8px 8px 26px; }
.aw-k { font-size: 10.5px; color: var(--text-3); display: block; margin-bottom: 3px; }
.aw-pre {
  margin: 0; padding: 8px 10px; background: #0a0a0d; border: 1px solid #1d1d25; border-radius: 8px;
  font-family: var(--mono, monospace); font-size: 11.5px; line-height: 1.55; color: #8d95a8;
  max-height: 260px; overflow: auto; white-space: pre-wrap; word-break: break-all;
}

/* 选项卡（A2UI choices） */
.aw-choices { display: flex; flex-direction: column; gap: 6px; padding-left: 4px; }
.aw-choice {
  display: flex; flex-direction: column; gap: 2px; text-align: left;
  background: #121218; border: 1px solid #26262e; border-radius: 10px;
  padding: 8px 14px; cursor: pointer; transition: all .12s ease; font-family: inherit;
}
.aw-choice:hover:not(:disabled) { border-color: #3c3c48; background: #17171d; }
.aw-choice b { font-size: 12.5px; color: #e2e4ec; font-weight: 600; }
.aw-choice-detail { font-size: 11.5px; color: var(--text-3); }
.aw-choice.picked { border-color: #2e4a38; background: #121a14; cursor: default; }
.aw-choice.picked b::after { content: " ✓"; color: var(--good); }
.aw-choice.dim { opacity: .45; }
.aw-choice:disabled { cursor: default; }

/* rail 底部入口 */
.aw-rail-foot { display: flex; gap: 6px; padding: 2px 0 4px; }
.aw-rail-foot button {
  flex: 1; background: none; border: 1px solid transparent; border-radius: 8px;
  color: var(--text-3); font-size: 11.5px; padding: 5px 4px; cursor: pointer;
  transition: all .12s ease; font-family: inherit; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
}
.aw-rail-foot button:hover { color: var(--text-2); background: var(--bg-hover); border-color: #22222a; }

/* 计划：单色清单 */
.aw-plan { display: flex; flex-direction: column; gap: 4px; padding: 2px 0 2px 6px; }
.aw-plan-step { display: flex; align-items: center; gap: 9px; font-size: 13px; color: #c3c8d4; padding: 2px 0; }
.aw-plan-step.done { color: var(--text-3); text-decoration: line-through; }
.aw-plan-dot { width: 7px; height: 7px; border-radius: 50%; flex: none; background: #33333e; }
.aw-plan-dot.done { background: var(--good); }
.aw-plan-dot.running { background: var(--warn); animation: aw-twinkle 1.2s ease-in-out infinite; }
@keyframes aw-twinkle { 0%, 100% { opacity: 1; } 50% { opacity: .45; } }

/* 审批卡：唯一带边框的卡（Codex 同款强调） */
.aw-ap {
  border: 1px solid #2e2e38; border-radius: 12px; background: #131317;
  padding: 14px 16px;
}
.aw-ap.resolved { opacity: .72; }
.aw-ap-title { font-size: 13.5px; font-weight: 600; color: #e8eaf2; }
.aw-ap-sub { font-size: 12px; color: var(--text-3); margin-top: 4px; }
.aw-diff {
  border: 1px solid #20202a; border-radius: 8px; overflow-y: auto; margin: 10px 0 12px;
  max-height: 280px;
  font-family: var(--mono, monospace); font-size: 11.5px; line-height: 1.65;
  background: #0b0b0f;
}
.aw-diff-line { display: flex; gap: 4px; padding: 0 10px; white-space: pre-wrap; word-break: break-all; }
.aw-diff-line.add { background: rgba(53, 208, 127, .09); color: #86cfa4; }
.aw-diff-line.del { background: rgba(255, 143, 143, .09); color: #ff9f9f; text-decoration: line-through; }
.aw-diff-line.same { color: #667087; }
.aw-diff-line.note { color: #8a8a96; font-style: italic; }
.aw-diff-sign { width: 12px; flex: none; user-select: none; }
.aw-ap-btns { display: flex; gap: 8px; }
.aw-ap-btns button { border-radius: 8px; font-size: 12.5px; padding: 5px 14px; cursor: pointer; border: 1px solid transparent; font-weight: 600; }
.ap-allow { background: #e8e8f0; color: #101014; }
.ap-allow:hover { background: #ffffff; }
.ap-always { background: none; border-color: var(--line-strong); color: var(--text-2); font-weight: 500; }
.ap-always:hover { color: var(--text); border-color: #3a3a46; }
.ap-deny { background: none; border-color: #4a2226; color: #ff9aa8; font-weight: 500; }
.ap-deny:hover { background: #2a1418; }
.aw-ap-state { font-size: 12px; }
.aw-ap-state.ok { color: var(--good); }
.aw-ap-state.no { color: #c9a6ff; }

/* 错误 + 重试 */
.aw-err { display: flex; align-items: center; gap: 12px; }
.aw-err-text { flex: 1; color: #ffb3b3; font-size: 13px; word-break: break-word; }
.aw-retry {
  flex: none; background: none; border: 1px solid #4a2226; color: #ff9aa8;
  border-radius: 8px; font-size: 12px; padding: 4px 14px; cursor: pointer;
}
.aw-retry:hover { background: #2a1418; }

.aw-notice { color: #7d8699; font-size: 12.5px; padding: 1px 0; }
.aw-notice::before { content: "› "; color: var(--text-3); }

/* 工作中状态行 */
.aw-working { display: flex; align-items: center; gap: 9px; color: var(--text-3); font-size: 12.5px; padding: 2px 0; }
.aw-spin {
  width: 12px; height: 12px; border-radius: 50%; flex: none;
  border: 2px solid #2a2a34; border-top-color: var(--text-2);
  animation: aw-spin .8s linear infinite;
}
.aw-working-elapsed { font-family: var(--mono, monospace); font-size: 11px; color: #5b6379; }
.aw-caret {
  display: inline-block; width: 7px; height: 15px; background: #8ab4ec;
  margin-top: 6px; animation: aw-caret 0.9s steps(2) infinite;
}
@keyframes aw-caret { 0%, 100% { opacity: 1; } 50% { opacity: 0; } }

/* ============ 快捷指令 chips ============ */
.aw-chips {
  display: flex; flex-wrap: wrap; gap: 8px; justify-content: center;
  max-width: 780px; width: 100%; margin: 0 auto; padding: 0 24px 4px;
}
.aw-chips button {
  font-size: 12px; color: #b6bccb; background: #121218; border: 1px solid #26262e;
  border-radius: 999px; padding: 6px 14px; cursor: pointer; transition: all 0.12s ease;
}
.aw-chips button:hover { color: #d8dce6; border-color: #34343e; background: #17171d; }
.aw-chip-nav { color: #7fa86f !important; border-color: #233a26 !important; }

/* ============ 悬浮 composer ============ */
.aw-composer-wrap {
  flex: none; padding: 10px 24px 14px;
  background: linear-gradient(180deg, rgba(11, 11, 14, 0) 0%, var(--bg) 26%);
}
.aw-composer {
  max-width: 780px; margin: 0 auto;
  background: #131318; border: 1px solid #2a2a34; border-radius: 18px;
  padding: 12px 14px 10px;
  box-shadow: 0 10px 40px rgba(0, 0, 0, .45);
  transition: border-color .12s ease;
}
.aw-composer:focus-within { border-color: #3c3c48; }
.aw-composer textarea {
  width: 100%; resize: none; background: transparent; border: none; outline: none;
  color: #e8eaf2; font-size: 13.5px; font-family: inherit; line-height: 1.6;
  min-height: 24px; max-height: 200px; padding: 2px 4px;
}
.aw-composer textarea::placeholder { color: #5c5c68; }
.aw-composer-bar { display: flex; align-items: center; gap: 10px; margin-top: 8px; padding: 0 2px; }
.aw-pill {
  display: inline-flex; align-items: center; gap: 6px;
  font-size: 11.5px; color: var(--text-2);
  border: 1px solid #26262e; border-radius: 999px; padding: 3px 10px;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 220px;
}
.aw-pill.on .status-dot { background: var(--good); }
.aw-pill.off { color: #c9a6ff; }
.aw-usage { font-size: 11px; color: var(--text-3); font-family: var(--mono, monospace); white-space: nowrap; }
.aw-elapsed { font-size: 11px; color: var(--text-3); font-family: var(--mono, monospace); }
.aw-send {
  width: 30px; height: 30px; border-radius: 50%; border: none;
  background: #e8e8f0; color: #101014; font-size: 15px; font-weight: 700;
  cursor: pointer; transition: background .12s ease; line-height: 1;
}
.aw-send:hover:not(:disabled) { background: #ffffff; }
.aw-send:disabled { opacity: .25; cursor: default; }
.aw-stop {
  width: 30px; height: 30px; border-radius: 50%; border: 1px solid #4a2226;
  background: #2a1418; cursor: pointer; display: inline-flex; align-items: center; justify-content: center;
}
.aw-stop:hover { background: #3a1a20; }
.aw-stop-sq { width: 10px; height: 10px; border-radius: 2px; background: #ff9aa8; }
.aw-hint {
  max-width: 780px; margin: 6px auto 0; text-align: center;
  font-size: 10.5px; color: #4d4d58;
}
</style>
