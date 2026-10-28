<script setup lang="ts">
// 世界画布（PLAN-canvas v2.2）——「一眼全貌 · 点哪改哪 · 立刻看得见」。
// 剧情单一真源 = film/flow.json 的 story 字段：画布只投影（布局/候选/任务/批注
// 落 canvas/board.json），改文案只经统一编辑弹层整体回写。
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  NAlert, NButton, NDropdown, NInput, NModal, NSelect, NSpace, NTabPane, NTabs, NTag, useDialog, useMessage,
} from "naive-ui";
import { humanizeError } from "../utils/errors";
import { api, type FilmFlow, type ProjectMeta } from "../api";
import { FilmStory } from "../lib/filmStory";
import {
  boardFindings, defaultBoard, sanitizeBoard, uid,
  type AiTaskRecord, type BoardFile, type Candidate,
} from "../lib/board";
import { autoLayout, useViewport } from "../lib/canvas/layout";
import { storyShots, storyVoItems, tryParseStory, type RawStory } from "../lib/canvas/storywalk";
import CanvasBoard from "../components/canvas/CanvasBoard.vue";
import NodeEditDialog from "../components/canvas/NodeEditDialog.vue";
import PreviewDrawer from "../components/canvas/PreviewDrawer.vue";

const message = useMessage();
const dialog = useDialog();
const route = useRoute();
const router = useRouter();

/* ── 项目与影游流（story 单一真源所在） ── */
const projects = ref<ProjectMeta[]>([]);
const project = ref<string | null>((route.params.id as string) || null);
const flow = ref<FilmFlow | null>(null);
const storyText = ref("");
const clips = ref<Record<string, string>>({});
const voDone = ref<Record<string, string>>({});
const voices = ref<{ role: string; speaker: string }[]>([]);
const exportedZip = ref("");
const readonly = ref(false);

const DEFAULT_VOICES = [{ role: "narrator", speaker: "zh_male_shenyeboke_uranus_bigtts" }];
const parsed = computed(() => tryParseStory(storyText.value));
const story = computed<RawStory | null>(() => (parsed.value.ok ? parsed.value.story : null));

/* ── 画布数据（投影层） ── */
const board = ref<BoardFile>(defaultBoard());
const vp = useViewport();
const boardEl = ref<InstanceType<typeof CanvasBoard> | null>(null);
const selected = ref<string[]>([]);
const pro = ref(localStorage.getItem("oe.canvasPro") === "1");
const highlightNode = ref<string | null>(null);
const serverBase = ref("http://127.0.0.1:8000");

/* ── 加载 ── */
const loading = ref(true);
onMounted(load);
watch(project, load);

async function load() {
  loading.value = true;
  readonly.value = false;
  try {
    projects.value = await api.listProjects();
    if (!project.value || !projects.value.some((p) => p.id === project.value)) {
      project.value = projects.value[0]?.id ?? null;
    }
    if (!project.value) { loading.value = false; return; }
    const f = await api.filmFlowLoad(project.value);
    flow.value = f;
    storyText.value = f?.story ?? "";
    clips.value = { ...(f?.clips ?? {}) };
    voDone.value = { ...(f?.vo ?? {}) };
    voices.value = f?.voices?.length ? [...f.voices] : [...DEFAULT_VOICES];
    exportedZip.value = f?.exported_zip ?? "";
    // 端口 → 媒体绝对地址基座（预览/缩略图共用）
    try {
      const st = await api.getStatus();
      serverBase.value = `http://127.0.0.1:${st.server.port}`;
    } catch { /* 浏览器 dev 环境：保持默认 */ }
    // 画布数据：损坏 → 只读模式，绝不静默覆盖
    let b: BoardFile;
    try {
      const raw = await api.canvasLoad(project.value);
      b = sanitizeBoard(raw);
    } catch (e) {
      b = defaultBoard();
      readonly.value = true;
      message.error(`画布数据已损坏，已进入只读模式：${humanizeError(e).slice(0, 120)}`);
    }
    // 布局缝合：新节拍自动布局补位，消失节拍清位——向导与画布同源
    const st = story.value;
    const ids = new Set((st?.nodes ?? []).map((n) => n.id));
    const have = new Set(b.nodes.map((n) => n.ref));
    if (st) for (const p of autoLayout(st)) if (!have.has(p.ref)) b.nodes.push(p);
    b.nodes = b.nodes.filter((n) => ids.has(n.ref));
    const savedVp = { ...b.meta.viewport };
    b.meta.viewport = savedVp;
    board.value = b;
    vp.vp.x = savedVp.x; vp.vp.y = savedVp.y; vp.vp.scale = savedVp.scale;
    check();
  } finally {
    loading.value = false;
  }
}

/* ── 持久化：board 防抖自动保存；flow 变更即存 ── */
let saveTimer: ReturnType<typeof setTimeout> | null = null;
function scheduleSave() {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => void saveBoardNow(), 900);
}
async function saveBoardNow() {
  if (!project.value || readonly.value) return;
  // 逐字段回写：整对象替换会再次触发 deep watch，形成保存死循环
  const m = board.value.meta.viewport;
  m.x = vp.vp.x; m.y = vp.vp.y; m.scale = vp.vp.scale;
  try {
    await api.canvasSave(project.value, JSON.parse(JSON.stringify(board.value)));
  } catch (e) {
    message.error(`画布保存失败：${humanizeError(e).slice(0, 120)}`);
  }
}
watch(board, scheduleSave, { deep: true });
watch(() => ({ ...vp.vp }), scheduleSave);

async function saveFlow() {
  if (!project.value || !flow.value) return;
  const f: FilmFlow = {
    step: flow.value.step || 5,
    locks: flow.value.locks,
    story: storyText.value,
    voices: voices.value,
    clips: clips.value,
    vo: voDone.value,
    exported_zip: exportedZip.value || null,
  };
  flow.value = f;
  await api.filmFlowSave(project.value, f);
}

/* ── 撤销重做：只覆盖结构性操作；生成完成/自动保存是事件不入栈 ── */
const undoStack = ref<string[]>([]);
const redoStack = ref<string[]>([]);
function boardSnapshot() {
  return JSON.stringify({ nodes: board.value.nodes, slots: board.value.slots, notes: board.value.notes, flags: board.value.flags, freeEdges: board.value.freeEdges });
}
function pushUndo() {
  undoStack.value.push(boardSnapshot());
  if (undoStack.value.length > 50) undoStack.value.shift();
  redoStack.value = [];
}
function applySnap(s: string) {
  const o = JSON.parse(s);
  board.value.nodes = o.nodes;
  board.value.slots = o.slots;
  board.value.notes = o.notes;
  board.value.flags = o.flags;
  board.value.freeEdges = o.freeEdges ?? [];
}
function undo() {
  const s = undoStack.value.pop();
  if (!s) return;
  redoStack.value.push(boardSnapshot());
  applySnap(s);
  void saveBoardNow();
}
function redo() {
  const s = redoStack.value.pop();
  if (!s) return;
  undoStack.value.push(boardSnapshot());
  applySnap(s);
  void saveBoardNow();
}

/* ── 体检（复用引擎内核镜像的同一发现集） ── */
const storyFindings = ref<any[]>([]);
const checked = ref(false);
function check() {
  try {
    storyFindings.value = FilmStory.validate(FilmStory.parse(storyText.value));
  } catch (e) {
    storyFindings.value = [{ severity: "error", node: "-", kind: "json", detail: String(e) }];
  }
  checked.value = true;
}
const storyGreen = computed(() => checked.value && storyFindings.value.length === 0);
const canvasFindings = computed(() => boardFindings(board.value, clips.value));

/** 接线指引：把"建了却没接线"的节拍翻译成画布动作——工坊体检的 dead_end/unreachable 在画布侧的因果层 */
const unconnectedBeats = computed(() => {
  const st = story.value;
  if (!st) return [];
  return st.nodes
    .filter((n) => {
      if (n.type === "scene") return !n.goto;
      if (n.type === "choice") return !(n.options && n.options.length);
      if (n.type === "hub") return !(n.routes && n.routes.length);
      return false; // 结局无需出边
    })
    .map((n) => n.id);
});

/* ── 缺口（待办语义：灰蓝，不搞红色警报） ── */
const shotsAll = computed(() => (story.value ? storyShots(story.value) : []));
const voAll = computed(() => (story.value ? storyVoItems(story.value) : []));
function slotBusy(id: string) {
  return Object.values(board.value.tasks).some((t) => t.slot === id && (t.status === "queued" || t.status === "running"));
}
const missingShots = computed(() => shotsAll.value.filter((s) => !clips.value[s.id] && !slotBusy(s.id)));
const missingVo = computed(() => voAll.value.filter((v) => !voDone.value[v.id] && !slotBusy(v.id)));

/* ── 生成队列（并发 3；失败断链防连环烧钱——沿用向导纪律） ── */
const clipTier = ref("doubao-seedance-2-0-mini-260615");
const clipRatio = ref("9:16");
const voEngine = ref<"cloud" | "local">((localStorage.getItem("oe.voEngine") as "cloud" | "local") || "cloud");
const CONC = 3;
let active = 0;
const pending: string[] = [];
const busyTasks = computed(() => Object.values(board.value.tasks).filter((t) => t.status === "queued" || t.status === "running"));

function speakerOf(role: string) {
  return voices.value.find((v) => v.role === role)?.speaker || "zh_male_shenyeboke_uranus_bigtts";
}
function voItemOf(id: string) {
  return voAll.value.find((v) => v.id === id) ?? null;
}

function enqueueTask(kind: "shot" | "vo", slot: string, prompt: string, model?: string, params?: Record<string, unknown>) {
  const t: AiTaskRecord = { id: uid("task"), kind, slot, prompt, model, params, status: "queued" };
  board.value.tasks[t.id] = t;
  pending.push(t.id);
  pump();
}
function pump() {
  while (active < CONC && pending.length) void runTask(pending.shift()!);
}
async function runTask(id: string) {
  const t = board.value.tasks[id];
  if (!t || !project.value) return;
  active += 1;
  t.status = "running";
  t.started = Date.now();
  try {
    if (t.kind === "shot") {
      const meta = await api.generateVideo(
        t.prompt, undefined,
        (t.params?.tier as string) ?? clipTier.value,
        (t.params?.ratio as string) ?? clipRatio.value,
      );
      const cand: Candidate = {
        id: uid("cand"), src: meta.file, from: "ai", prompt: t.prompt, model: t.model,
        created: Date.now(), tokens: meta.usage_tokens ?? null,
      };
      const slot = board.value.slots[t.slot] ?? (board.value.slots[t.slot] = { candidates: [], pinned: null });
      slot.candidates.push(cand);
      slot.pinned = cand.id; // 新产出自动上钉，旧版留档可随时切回
      clips.value[t.slot] = cand.src;
    } else {
      const item = voItemOf(t.slot);
      const speaker = speakerOf(item?.role ?? "narrator");
      const text = item?.text ?? t.prompt;
      const file = voEngine.value === "local"
        ? await api.ttsLocal(project.value, text, t.slot, speaker)
        : await api.ttsDoubao(project.value, speaker, text, t.slot);
      voDone.value[t.slot] = file;
    }
    t.status = "done";
    t.finished = Date.now();
    message.success(`${t.slot} 完成`);
  } catch (e) {
    t.status = "failed";
    t.error = humanizeError(e).slice(0, 160);
    t.finished = Date.now();
    message.error(`${t.slot} 失败：${t.error}`);
  } finally {
    active -= 1;
    await saveFlow();
    await saveBoardNow();
    pump();
  }
}
function retryTask(id: string) {
  const t = board.value.tasks[id];
  if (!t) return;
  enqueueTask(t.kind, t.slot, t.prompt, t.model, t.params);
}
function cancelTask(id: string) {
  const t = board.value.tasks[id];
  if (!t || t.status !== "queued") return;
  const i = pending.indexOf(id);
  if (i >= 0) pending.splice(i, 1);
  delete board.value.tasks[id];
}

/** 一键补齐（UGC 的魔法 = PGC 的批量，同一功能两种包装）。确认清单防误触烧钱 */
function askFillAll() {
  const nShot = missingShots.value.length;
  const nVo = missingVo.value.length;
  if (!nShot && !nVo) return message.info("没有缺口 —— 全部就绪");
  const est = nShot * (clipTier.value.includes("mini") ? 2.5 : 7.5);
  const d = dialog.warning({
    title: "确认生成清单",
    content: `即将生成：镜头 ${nShot} 条（估 ¥${est.toFixed(1)}）· 配音 ${nVo} 条（${voEngine.value === "local" ? "本地零费" : "按条计费"}）。并发 ${CONC}，失败自动断链。`,
    positiveText: "开始生成",
    negativeText: "取消",
    onPositiveClick: () => {
      d.destroy();
      for (const s of missingShots.value) enqueueTask("shot", s.id, s.frame + (s.motion ? `，${s.motion}` : ""), clipTier.value, { tier: clipTier.value, ratio: clipRatio.value });
      for (const v of missingVo.value) enqueueTask("vo", v.id, v.text);
      message.success(`已入队 ${nShot + nVo} 个任务（右上角任务面板可看进度）`);
    },
  });
}
function genShotOf(shotId: string) {
  const s = shotsAll.value.find((x) => x.id === shotId);
  if (!s) return;
  enqueueTask("shot", s.id, s.frame + (s.motion ? `，${s.motion}` : ""), clipTier.value, { tier: clipTier.value, ratio: clipRatio.value });
}
function genVoOf(voId: string) {
  const v = voItemOf(voId);
  if (!v) return;
  enqueueTask("vo", v.id, v.text);
}

/* ── 版本组操作 ── */
function pinCandidate(slotId: string, candId: string) {
  const slot = board.value.slots[slotId];
  const cand = slot?.candidates.find((c) => c.id === candId);
  if (!slot || !cand) return;
  const nodeRef = slotId.replace(/_\d+$/, "");
  const doPin = () => {
    pushUndo();
    slot.pinned = candId;
    clips.value[slotId] = cand.src;
    void saveFlow();
  };
  const pos = board.value.nodes.find((n) => n.ref === nodeRef);
  if (pos?.locked) {
    const d = dialog.warning({
      title: "节拍已定稿",
      content: `「${nodeRef}」已定稿锁定——确认把镜头换为 ${cand.id}？`,
      positiveText: "确认替换", negativeText: "取消",
      onPositiveClick: () => { d.destroy(); doPin(); },
    });
  } else doPin();
}
function delCandidate(slotId: string, candId: string) {
  const slot = board.value.slots[slotId];
  if (!slot) return;
  pushUndo();
  slot.candidates = slot.candidates.filter((c) => c.id !== candId);
  if (slot.pinned === candId) {
    slot.pinned = slot.candidates[slot.candidates.length - 1]?.id ?? null;
    const src = slot.candidates[slot.candidates.length - 1]?.src;
    if (src) clips.value[slotId] = src;
  }
}
function attachAsset(src: string) {
  const nodeRef = selected.value.find((s) => story.value?.nodes.some((n) => n.id === s));
  if (!nodeRef) return message.warning("先在画布选中一个节拍，再挂素材");
  const nodeShots = shotsAll.value.filter((s) => s.node === nodeRef);
  if (!nodeShots.length) return message.warning("该节拍没有镜头");
  const target = nodeShots.find((s) => !clips.value[s.id]) ?? nodeShots[0];
  const slot = board.value.slots[target.id] ?? (board.value.slots[target.id] = { candidates: [], pinned: null });
  pushUndo();
  const cand: Candidate = { id: uid("cand"), src, from: "import", created: Date.now() };
  slot.candidates.push(cand);
  slot.pinned = cand.id;
  clips.value[target.id] = src;
  void saveFlow();
  message.success(`已挂到 ${target.id}`);
}

/* ── 编辑弹层（统一改文案入口） ── */
const editShow = ref(false);
const editRef = ref<string | null>(null);
function openEdit(refId: string) {
  const pos = board.value.nodes.find((n) => n.ref === refId);
  const doOpen = () => { editRef.value = refId; editShow.value = true; };
  if (pos?.locked) {
    const d = dialog.warning({
      title: "节拍已定稿", content: `「${refId}」已定稿锁定——仍要编辑？`,
      positiveText: "仍要编辑", negativeText: "取消",
      onPositiveClick: () => { d.destroy(); doOpen(); },
    });
  } else doOpen();
}
function onEditSave(text: string) {
  storyText.value = text;
  check();
  void saveFlow();
  message.success("剧情已回写");
}
function toggleLock(refId: string) {
  const pos = board.value.nodes.find((n) => n.ref === refId);
  if (!pos) return;
  pushUndo();
  pos.locked = !pos.locked;
}
function deleteNode(refId: string) {
  const st = story.value;
  if (!st) return;
  const d = dialog.warning({
    title: "删除节拍",
    content: `从剧情中删除「${refId}」？指向它的连线会由体检标出。`,
    positiveText: "删除", negativeText: "取消",
    onPositiveClick: () => {
      d.destroy();
      st.nodes = st.nodes.filter((n) => n.id !== refId);
      storyText.value = JSON.stringify(st, null, 2);
      board.value.nodes = board.value.nodes.filter((n) => n.ref !== refId);
      delete board.value.flags[refId];
      check();
      void saveFlow();
    },
  });
}

/* ── 看片批注 ── */
function setFlag(nodeRef: string, text: string) {
  pushUndo();
  board.value.flags[nodeRef] = text;
}
function clearFlag(nodeRef: string) {
  pushUndo();
  delete board.value.flags[nodeRef];
}

/* ── 便签 ── */
const noteEdit = ref<{ show: boolean; id: string | null; text: string }>({ show: false, id: null, text: "" });
function addNote() {
  pushUndo();
  const size = boardEl.value?.stageSize?.() ?? { w: 800, h: 600 };
  const wx = (size.w / 2 - vp.vp.x) / vp.vp.scale;
  const wy = (size.h / 2 - vp.vp.y) / vp.vp.scale;
  board.value.notes.push({ id: uid("note"), text: "新便签", x: Math.round(wx), y: Math.round(wy) });
}
function editNote(id: string) {
  const n = board.value.notes.find((x) => x.id === id);
  if (!n) return;
  noteEdit.value = { show: true, id, text: n.text };
}
function saveNote() {
  const n = board.value.notes.find((x) => x.id === noteEdit.value.id);
  if (n) n.text = noteEdit.value.text;
  noteEdit.value.show = false;
}
function delNote(id: string) {
  pushUndo();
  board.value.notes = board.value.notes.filter((n) => n.id !== id);
}

/* ── 自由创作（画布 = 创作面，不是投影面）：双击空白新建 + 端口连线写回 ── */
const createMenu = ref<{ show: boolean; x: number; y: number; wx: number; wy: number }>({ show: false, x: 0, y: 0, wx: 0, wy: 0 });
function onCreateAt(p: { wx: number; wy: number; x: number; y: number }) {
  createMenu.value = { show: true, x: p.x, y: p.y, wx: p.wx, wy: p.wy };
}
const createOptions = [
  { label: "🎬 场景节拍（写进剧情）", key: "scene" },
  { label: "🔀 抉择（写进剧情）", key: "choice" },
  { label: "🧭 分流（写进剧情）", key: "hub" },
  { label: "🏁 结局（写进剧情）", key: "ending" },
  { type: "divider", key: "d1" },
  { label: "📝 便签（仅画布）", key: "note" },
  { label: "🖼 图片卡（仅画布）", key: "image" },
];
function uniqueBeatId(st: RawStory): string {
  let i = st.nodes.length + 1;
  while (st.nodes.some((n) => n.id === `b${i}`)) i += 1;
  return `b${i}`;
}
/** 画布自由创作不要求先有剧本：story 缺失就就地初始化，第一个节拍在画布上诞生 */
function ensureStoryText(): RawStory {
  if (parsed.value.ok) return parsed.value.story;
  const fresh: RawStory = { meta: { title: "我的影游" }, vars: {}, nodes: [] };
  storyText.value = JSON.stringify(fresh, null, 2);
  return fresh;
}
async function ensureFlowInitialized() {
  if (flow.value) return;
  flow.value = {
    step: 2, locks: { protagonist: "", supporting: "", style: "" },
    story: storyText.value, voices: [...DEFAULT_VOICES], clips: {}, vo: {}, exported_zip: null,
  };
  await saveFlow();
}
function addBeat(kind: string, wx: number, wy: number) {
  const st = ensureStoryText();
  const id = uniqueBeatId(st);
  const node: RawStory["nodes"][number] =
    kind === "scene" ? { id, type: "scene", title: "新场景", text: "", shots: [{ frame: "", motion: "", dur: 5, vo: "" }] }
    : kind === "choice" ? { id, type: "choice", title: "新抉择", options: [] }
    : kind === "hub" ? { id, type: "hub", title: "新分流", routes: [] }
    : { id, type: "ending", title: "新结局", text: "" };
  st.nodes.push(node);
  storyText.value = JSON.stringify(st, null, 2);
  pushUndo();
  board.value.nodes.push({ ref: id, x: wx, y: wy });
  void ensureFlowInitialized().then(saveFlow);
  check();
  message.success(`已创建节拍 ${id} —— 从节点右侧圆点拖线把它接进剧情`);
}
function onCreateSelect(key: string) {
  const m = createMenu.value;
  createMenu.value.show = false;
  if (key === "note") {
    pushUndo();
    board.value.notes.push({ id: uid("note"), text: "新便签", x: m.wx, y: m.wy });
  } else if (key === "image") {
    imagePick = { wx: m.wx, wy: m.wy };
    imagePickShow.value = true;
  } else {
    addBeat(key, m.wx, m.wy);
  }
}

/* 图片卡：从素材库挑一张贴上画布（仅画布层） */
let imagePick: { wx: number; wy: number } = { wx: 0, wy: 0 };
const imagePickShow = ref(false);
function pickImage(file: string) {
  pushUndo();
  board.value.notes.push({ id: uid("note"), text: "", src: file, x: imagePick.wx, y: imagePick.wy });
  imagePickShow.value = false;
  message.success("图片卡已贴上画布");
}

/* 连线意图：自由参考线 或 写回剧情（goto/选项/路由） */
const connectMenu = ref<{ show: boolean; x: number; y: number; from: string; to: string }>({ show: false, x: 0, y: 0, from: "", to: "" });
function onConnect(p: { from: string; to: string; x: number; y: number }) {
  connectMenu.value = { show: true, x: p.x, y: p.y, from: p.from, to: p.to };
}
const connectOptions = computed(() => {
  const s = story.value?.nodes.find((n) => n.id === connectMenu.value.from);
  const out: any[] = [{ label: "📌 仅参考连线（不写剧情）", key: "free" }];
  if (s?.type === "scene") out.push({ label: `➡ 设为直连（${connectMenu.value.from} → ${connectMenu.value.to}）`, key: "goto" });
  if (s?.type === "choice") out.push({ label: `➕ 加为选项（→ ${connectMenu.value.to}）`, key: "option" });
  if (s?.type === "hub") out.push({ label: `➕ 加为分流路由（→ ${connectMenu.value.to}）`, key: "route" });
  return out;
});
function applyConnect(key: string) {
  const { from, to } = connectMenu.value;
  connectMenu.value.show = false;
  const st = story.value;
  const s = st?.nodes.find((n) => n.id === from);
  if (key === "free" || !st || !s) {
    pushUndo();
    board.value.freeEdges.push({ id: uid("fe"), from, to });
    message.success("已加参考连线（不写剧情，右键可删）");
    return;
  }
  pushUndo();
  if (key === "goto" && s.type === "scene") {
    s.goto = to;
    message.success(`${from} 直连 → ${to}`);
  } else if (key === "option" && s.type === "choice") {
    if (!Array.isArray(s.options)) s.options = [];
    s.options.push({ text: `去 ${to}`, set: null, goto: to });
    message.success(`已加选项 → ${to}（双击节点可改文案）`);
  } else if (key === "route" && s.type === "hub") {
    if (!Array.isArray(s.routes)) s.routes = [];
    s.routes.push({ when: null, goto: to });
    message.success(`已加默认路由 → ${to}`);
  }
  storyText.value = JSON.stringify(st, null, 2);
  check();
  void saveFlow();
}

/* ── 右键菜单 ── */
const menu = ref<{ show: boolean; x: number; y: number; kind: "node" | "note" | "bg" | "edge"; ref?: string; noteId?: string; edgeId?: string }>({ show: false, x: 0, y: 0, kind: "bg" });
const menuOptions = computed(() => {
  if (menu.value.kind === "node") {
    const pos = board.value.nodes.find((n) => n.ref === menu.value.ref);
    return [
      { label: "✏️ 编辑节拍", key: "edit" },
      { label: pos?.locked ? "🔓 解锁定稿" : "🔒 定稿锁定", key: "lock" },
      { label: "🚩 标记待改", key: "flag" },
      { label: "🎬 生成缺的镜头", key: "genshot", disabled: !shotsAll.value.some((s) => s.node === menu.value.ref && !clips.value[s.id]) },
      { label: "🗑 删除节拍", key: "del" },
    ];
  }
  if (menu.value.kind === "note") {
    return [
      { label: "✏️ 编辑便签", key: "note-edit" },
      { label: "🗑 删除便签", key: "note-del" },
    ];
  }
  if (menu.value.kind === "edge") {
    return [{ label: "🗑 删除参考连线", key: "edge-del" }];
  }
  return [
    { label: "📝 新建便签", key: "note-add" },
    { label: "🧲 自动布局", key: "layout" },
    { label: "🔍 适应全部", key: "fit" },
  ];
});
function onMenuSelect(key: string) {
  menu.value.show = false;
  const m = menu.value;
  if (m.kind === "node" && m.ref) {
    if (key === "edit") openEdit(m.ref);
    else if (key === "lock") toggleLock(m.ref);
    else if (key === "flag") {
      const d0 = dialog.info({
        title: `标记待改 · ${m.ref}`,
        content: "批注会显示在画布节点上，导出前逐条清零。",
      });
      void d0;
      flagInputNode.value = m.ref; flagShow.value = true;
    } else if (key === "genshot") {
      const s = shotsAll.value.find((x) => x.node === m.ref && !clips.value[x.id]);
      if (s) genShotOf(s.id);
    } else if (key === "del") deleteNode(m.ref);
  } else if (m.kind === "note" && m.noteId) {
    if (key === "note-edit") editNote(m.noteId);
    else if (key === "note-del") delNote(m.noteId);
  } else if (m.kind === "edge" && m.edgeId) {
    if (key === "edge-del") {
      pushUndo();
      board.value.freeEdges = board.value.freeEdges.filter((e) => e.id !== m.edgeId);
    }
  } else {
    if (key === "note-add") addNote();
    else if (key === "layout") relayout();
    else if (key === "fit") boardEl.value?.fit();
  }
}
function relayout() {
  if (!story.value) return;
  pushUndo();
  const laid = autoLayout(story.value);
  const keep = new Map(board.value.nodes.map((n) => [n.ref, n.locked ?? false]));
  board.value.nodes = laid.map((p) => ({ ...p, locked: keep.get(p.ref) ?? false }));
}

/* ── 标记待改输入 ── */
const flagShow = ref(false);
const flagInputNode = ref<string | null>(null);
const flagText = ref("");

/* ── 预览 ── */
const previewShow = ref(false);
const previewHtml = ref("");
const previewLoading = ref(false);
async function openPreview() {
  if (!project.value || !parsed.value.ok) return message.warning("story 无效，先体检修绿");
  previewLoading.value = true;
  previewShow.value = true;
  try {
    previewHtml.value = await api.canvasPreviewHtml(project.value, storyText.value);
  } catch (e) {
    message.error(`预览渲染失败：${humanizeError(e).slice(0, 120)}`);
    previewShow.value = false;
  } finally {
    previewLoading.value = false;
  }
}
function focusFlagNode(node: string) {
  boardEl.value?.focusNode(node);
  selected.value = [node];
}

/* ── 导出（体检门禁 + MANIFEST 由后端注入） ── */
const exporting = ref(false);
async function doExport() {
  check();
  if (!storyGreen.value) return message.error("剧情体检未全绿——先修完再导出（与向导同一道门禁）");
  if (!project.value) return;
  exporting.value = true;
  try {
    await saveBoardNow();
    await saveFlow();
    const zip = await api.exportFilm(project.value, flow.value!);
    exportedZip.value = zip;
    message.success("导出完成——zip 内含 MANIFEST.json 生成档案");
  } catch (e) {
    message.error(`导出失败：${humanizeError(e).slice(0, 140)}`);
  } finally {
    exporting.value = false;
  }
}

/* ── 素材库面板 ── */
const assets = ref<{ id: string; file: string; prompt: string; created: string; kind?: string }[]>([]);
const assetFilter = ref("");
const meshPrompt = ref("");
const meshBusy = ref(false);
async function refreshAssets() {
  try { assets.value = await api.listAssets(); } catch { /* 浏览器降级 */ }
}
const assetsView = computed(() => {
  const q = assetFilter.value.trim().toLowerCase();
  return assets.value
    .filter((a) => !q || a.prompt.toLowerCase().includes(q) || a.file.toLowerCase().includes(q))
    .slice()
    .reverse();
});
async function genMesh() {
  if (!meshPrompt.value.trim()) return message.warning("先写 3D 提示词");
  meshBusy.value = true;
  try {
    await api.generateMesh(meshPrompt.value.trim());
    meshPrompt.value = "";
    await refreshAssets();
    message.success("3D 素材已生成，进素材列表");
  } catch (e) {
    message.error(`生成失败：${humanizeError(e).slice(0, 120)}`);
  } finally { meshBusy.value = false; }
}

/* ── 快捷键（专业模式可见提示） ── */
function onKey(e: KeyboardEvent) {
  const t = e.target as HTMLElement | null;
  const typing = !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z" && !e.shiftKey) { e.preventDefault(); undo(); return; }
  if ((e.ctrlKey || e.metaKey) && (e.key.toLowerCase() === "y" || (e.shiftKey && e.key.toLowerCase() === "z"))) { e.preventDefault(); redo(); return; }
  if (typing) return;
  if (e.key === "Delete" || e.key === "Backspace") {
    const noteIds = selected.value.filter((s) => board.value.notes.some((n) => n.id === s));
    if (noteIds.length) { noteIds.forEach(delNote); }
  }
  const nudge = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[e.key];
  if (nudge) {
    e.preventDefault();
    const step = e.shiftKey ? 10 : 2;
    for (const id of selected.value) {
      const p = board.value.nodes.find((n) => n.ref === id);
      if (p) { p.x += nudge[0] * step; p.y += nudge[1] * step; }
    }
  }
}

/* ── 首启导览（UGC 三步） ── */
const onboardShow = ref(!localStorage.getItem("oe.canvasOnboarded"));
function finishOnboard() {
  localStorage.setItem("oe.canvasOnboarded", "1");
  onboardShow.value = false;
}

function togglePro() {
  pro.value = !pro.value;
  localStorage.setItem("oe.canvasPro", pro.value ? "1" : "0");
}

function goFilm() {
  void router.push("/film");
}
function onPreviewHighlight(node: string | null) {
  highlightNode.value = node;
  if (node) boardEl.value?.focusNode(node);
}
/** 侧栏面板 tab（体检标记点击会跳到 health） */
const panelName = ref("gaps");

onMounted(() => {
  window.addEventListener("keydown", onKey);
  void refreshAssets();
});
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <div class="cv-root">
    <NAlert v-if="!project && !loading" type="warning" style="margin-bottom: 12px">
      还没有项目——先去「新建游戏」或「影游工坊」走一遍向导，画布会以它的 story.json 为单一真源。
    </NAlert>

    <template v-else>
      <!-- 工具栏 -->
      <div class="cv-toolbar">
        <NSelect v-model:value="project" size="small" style="width: 190px"
          :options="projects.map(p => ({ label: p.name, value: p.id }))" />
        <NTag size="small" :type="storyGreen ? 'success' : checked ? 'error' : 'default'" round style="cursor: pointer"
          @click="check(); panelName = 'health'">
          {{ storyGreen ? "🩺 全绿" : checked ? `🩺 ${storyFindings.length} 项待修` : "🩺 体检" }}
        </NTag>
        <NButton size="small" type="primary" :disabled="!missingShots.length && !missingVo.length" @click="askFillAll">
          ⚡ {{ pro ? `批量生成（缺 ${missingShots.length + missingVo.length}）` : `一键补齐（缺 ${missingShots.length + missingVo.length}）` }}
        </NButton>
        <NButton size="small" :loading="previewLoading" @click="openPreview">▶ 预览</NButton>
        <NButton size="small" type="warning" :loading="exporting" :disabled="!storyGreen" @click="doExport">📦 导出</NButton>
        <NSelect v-if="pro" v-model:value="clipTier" size="small" style="width: 170px" :options="[
          { label: 'mini 草稿档', value: 'doubao-seedance-2-0-mini-260615' },
          { label: '2.5 高光档', value: 'doubao-seedance-2-5-260628' }]" />
        <NSelect v-if="pro" v-model:value="clipRatio" size="small" style="width: 96px" :options="[
          { label: '9:16', value: '9:16' }, { label: '16:9', value: '16:9' }]" />
        <span class="spacer"></span>
        <NButton size="small" quaternary title="撤销 (Ctrl+Z)" @click="undo">↶</NButton>
        <NButton size="small" quaternary title="重做 (Ctrl+Y)" @click="redo">↷</NButton>
        <NButton size="small" quaternary title="自动布局" @click="relayout">🧲</NButton>
        <NButton size="small" quaternary title="适应全部" @click="boardEl?.fit()">🔍</NButton>
        <NButton size="small" :type="pro ? 'primary' : 'default'" @click="togglePro">{{ pro ? "📡 专业模式" : "📟 简洁模式" }}</NButton>
      </div>

      <NAlert v-if="readonly" type="error" :show-icon="false" style="margin-bottom: 8px">
        画布数据已损坏，当前只读——可在项目目录 canvas/ 删除 board.json 后重建。
      </NAlert>
      <NAlert v-if="!storyText" type="warning" :show-icon="false" style="margin-bottom: 8px">
        自由画布模式 —— <b>双击任意空白处</b>即可创建第一个节拍（自动初始化剧本），节点右侧圆点可拖线连接；
        也可以去 <a style="color: #4f8ae8; cursor: pointer" @click="goFilm()">影游工坊</a> 走向导后再回来精修。
      </NAlert>

      <!-- 画布 + 侧栏 -->
      <div class="cv-main">
        <CanvasBoard
          ref="boardEl"
          :story="story"
          :nodes="board.nodes"
          :notes="board.notes"
          :free-edges="board.freeEdges"
          :clips="clips"
          :vo="voDone"
          :tasks="board.tasks"
          :flags="board.flags"
          :selected="selected"
          :vp="vp.vp"
          :pro="pro"
          :server-base="serverBase"
          :highlight-node="highlightNode"
          @update:selected="selected = $event"
          @begin-structural="pushUndo"
          @commit="() => void saveBoardNow()"
          @open-node="openEdit"
          @menu="(p) => { menu = { show: true, ...p } }"
          @vp-change="scheduleSave"
          @connect="onConnect"
          @create-at="onCreateAt"
        />

        <aside class="cv-side">
          <NTabs v-model:value="panelName" type="line" size="small" style="height: 100%">
            <!-- 缺口（待办） -->
            <NTabPane name="gaps" :tab="`缺口 ${missingShots.length + missingVo.length}`">
              <div class="cv-pane">
                <NButton size="tiny" type="primary" block :disabled="!missingShots.length && !missingVo.length" @click="askFillAll">
                  ⚡ {{ pro ? "批量生成全部" : "一键补齐" }}
                </NButton>
                <div class="cv-sec-title">缺镜头（{{ missingShots.length }}）</div>
                <div v-for="s in missingShots" :key="s.id" class="cv-row">
                  <code class="mono" style="cursor:pointer" @click="focusFlagNode(s.node)">{{ s.id }}</code>
                  <span class="cv-row-text">{{ s.frame.slice(0, 30) }}</span>
                  <NButton size="tiny" @click="genShotOf(s.id)">生成</NButton>
                </div>
                <div class="cv-sec-title">缺配音（{{ missingVo.length }}）</div>
                <div class="cv-row" style="margin-bottom: 6px">
                  <NSelect v-model:value="voEngine" size="tiny" :options="[
                    { label: '☁ 豆包（计费）', value: 'cloud' },
                    { label: '🏠 本地 piper（零费）', value: 'local' }]" @update:value="(v: any) => localStorage.setItem('oe.voEngine', v)" />
                </div>
                <div v-for="v in missingVo" :key="v.id" class="cv-row">
                  <code class="mono" style="cursor:pointer" @click="focusFlagNode(v.node)">{{ v.id }}</code>
                  <span class="cv-row-text">{{ v.text.slice(0, 22) }}</span>
                  <NButton size="tiny" @click="genVoOf(v.id)">合成</NButton>
                </div>
                <p v-if="!missingShots.length && !missingVo.length" class="cv-hint">全部就绪——按 ▶ 预览过片，或直接导出。</p>
              </div>
            </NTabPane>

            <!-- 任务队列 -->
            <NTabPane name="tasks" :tab="`任务 ${busyTasks.length}`">
              <div class="cv-pane">
                <div v-for="(t, id) in board.tasks" :key="id" class="cv-row" style="align-items: flex-start">
                  <NTag size="tiny" round :type="t.status === 'done' ? 'success' : t.status === 'failed' ? 'error' : t.status === 'running' ? 'info' : 'default'">
                    {{ { queued: "排队", running: "生成中", done: "完成", failed: "失败" }[t.status] }}
                  </NTag>
                  <div class="cv-task-body">
                    <code class="mono">{{ t.slot }}</code>
                    <span v-if="pro && t.tokens" class="cv-hint"> · {{ t.tokens }} tok</span>
                    <div v-if="t.error" class="cv-err">{{ t.error }}</div>
                  </div>
                  <NButton v-if="t.status === 'failed'" size="tiny" @click="retryTask(id as string)">重跑</NButton>
                  <NButton v-else-if="t.status === 'queued'" size="tiny" quaternary @click="cancelTask(id as string)">取消</NButton>
                </div>
                <p v-if="!Object.keys(board.tasks).length" class="cv-hint">还没有生成任务。缺口面板或右键节拍可发起。</p>
              </div>
            </NTabPane>

            <!-- 版本组（选中节拍） -->
            <NTabPane name="versions" tab="版本">
              <div class="cv-pane">
                <template v-for="refId in selected.filter(s => story?.nodes.some(n => n.id === s))" :key="refId">
                  <div class="cv-sec-title">{{ refId }}</div>
                  <template v-for="(s, i) in shotsAll.filter(x => x.node === refId)" :key="s.id">
                    <div class="cv-sec-title" style="opacity:.7; font-weight: 400">镜头 {{ s.id }}</div>
                    <div v-for="c in board.slots[s.id]?.candidates ?? []" :key="c.id" class="cv-row">
                      <video v-if="c.src.endsWith('.mp4')" class="cv-thumb" :src="`${serverBase}/${c.src}`" muted preload="metadata" />
                      <span v-else class="cv-thumb cv-thumb-missing">素材</span>
                      <div class="cv-task-body">
                        <div>
                          <NTag v-if="board.slots[s.id]?.pinned === c.id" size="tiny" type="success" :bordered="false">采纳</NTag>
                          <span v-if="pro && c.model" class="cv-hint"> {{ c.model }}</span>
                          <span v-if="pro && c.tokens" class="cv-hint"> · {{ c.tokens }} tok</span>
                        </div>
                        <div class="cv-hint cv-clip">{{ c.prompt?.slice(0, 40) }}</div>
                      </div>
                      <NButton size="tiny" :disabled="board.slots[s.id]?.pinned === c.id" @click="pinCandidate(s.id, c.id)">采纳</NButton>
                      <NButton size="tiny" quaternary type="error" @click="delCandidate(s.id, c.id)">删</NButton>
                    </div>
                    <div v-if="!board.slots[s.id]?.candidates?.length" class="cv-hint" style="padding-left: 4px">
                      还没有候选——缺口面板生成，或素材面板挂载。
                    </div>
                  </template>
                </template>
                <p v-if="!selected.length" class="cv-hint">选中一个节拍，这里对比它的镜头版本并采纳。</p>
              </div>
            </NTabPane>

            <!-- 素材库 -->
            <NTabPane name="assets" tab="素材">
              <div class="cv-pane">
                <NInput v-model:value="meshPrompt" size="small" placeholder="文生 3D（Rodin）提示词" @keydown.enter="genMesh" />
                <NButton size="tiny" style="margin: 6px 0" block :loading="meshBusy" @click="genMesh">生成 3D 素材</NButton>
                <NInput v-model:value="assetFilter" size="small" placeholder="过滤素材…" clearable style="margin-bottom: 6px" />
                <div v-for="a in assetsView" :key="a.id" class="cv-row">
                  <span class="cv-kind-chip">{{ { video: "🎬", mesh: "🧊", image: "🖼" }[a.kind ?? "image"] ?? "🖼" }}</span>
                  <span class="cv-row-text" :title="a.prompt">{{ a.prompt.slice(0, 22) || a.file }}</span>
                  <NButton size="tiny" @click="attachAsset(a.file)">挂镜头</NButton>
                </div>
                <p class="cv-hint" style="margin-top: 6px">「挂镜头」会挂到选中节拍的第一个缺片镜头（无缺片则替换首个）。</p>
              </div>
            </NTabPane>

            <!-- 体检 + 批注 -->
            <NTabPane name="health" :tab="`体检 ${storyFindings.length + canvasFindings.length}`">
              <div class="cv-pane">
                <NButton size="tiny" block @click="check">🩺 重新体检</NButton>
                <template v-if="unconnectedBeats.length">
                  <div class="cv-sec-title">接线指引（{{ unconnectedBeats.length }}）</div>
                  <div v-for="id in unconnectedBeats" :key="id" class="cv-row">
                    <code class="mono" style="cursor:pointer" @click="focusFlagNode(id)">{{ id }}</code>
                    <span class="cv-row-text">尚未接入剧情流——从节点右侧圆点拖线到目标</span>
                    <NButton size="tiny" @click="focusFlagNode(id)">去接</NButton>
                  </div>
                </template>
                <div v-for="(f, i) in storyFindings" :key="'s' + i" class="cv-row">
                  <NTag size="tiny" :type="f.severity === 'error' ? 'error' : 'warning'">{{ f.severity }}</NTag>
                  <code class="mono">{{ f.node }}</code>
                  <span class="cv-row-text">{{ f.detail || f.kind }}</span>
                </div>
                <div v-for="(f, i) in canvasFindings" :key="'c' + i" class="cv-row">
                  <NTag size="tiny" type="warning">{{ f.severity }}</NTag>
                  <span class="cv-row-text">{{ f.detail }}</span>
                </div>
                <div class="cv-sec-title">看片批注（{{ Object.keys(board.flags).length }}）</div>
                <div v-for="(f, node) in board.flags" :key="node" class="cv-row">
                  <code class="mono" style="cursor:pointer" @click="focusFlagNode(node as string)">{{ node }}</code>
                  <span class="cv-row-text">{{ f }}</span>
                  <NButton size="tiny" quaternary @click="clearFlag(node as string)">清</NButton>
                </div>
                <p v-if="storyGreen && !canvasFindings.length" class="cv-hint">全绿——可以导出了。</p>
                <p v-else-if="exportedZip" class="cv-hint">最近导出：<span class="mono" style="font-size:10px">{{ exportedZip }}</span></p>
              </div>
            </NTabPane>
          </NTabs>
        </aside>
      </div>

      <!-- 右键菜单 -->
      <NDropdown trigger="manual" :show="menu.show" :x="menu.x" :y="menu.y" placement="bottom-start"
        :options="menuOptions" @select="onMenuSelect" @clickoutside="menu.show = false" />
      <!-- 双击空白：新建菜单（自由创作入口） -->
      <NDropdown trigger="manual" :show="createMenu.show" :x="createMenu.x" :y="createMenu.y" placement="bottom-start"
        :options="createOptions" @select="onCreateSelect" @clickoutside="createMenu.show = false" />
      <!-- 拖线落点：连线意图菜单 -->
      <NDropdown trigger="manual" :show="connectMenu.show" :x="connectMenu.x" :y="connectMenu.y" placement="bottom-start"
        :options="connectOptions" @select="applyConnect" @clickoutside="connectMenu.show = false" />

      <!-- 图片卡选素材 -->
      <NModal :show="imagePickShow" preset="card" title="🖼 选一张素材贴上画布" style="max-width: 600px" @close="imagePickShow = false">
        <div class="cv-pick-grid">
          <div v-for="a in assetsView.filter(x => x.file.endsWith('.mp4') || (x.kind ?? '') === 'image' || x.file.match(/\.(png|jpe?g|webp|gif)$/))" :key="a.id"
            class="cv-pick-item" @click="pickImage(a.file)">
            <video v-if="a.file.endsWith('.mp4')" class="cv-pick-thumb" :src="`${serverBase}/${a.file}`" muted preload="metadata" />
            <img v-else class="cv-pick-thumb" :src="`${serverBase}/${a.file}`" />
            <span class="cv-hint">{{ a.prompt.slice(0, 16) || a.file.slice(-14) }}</span>
          </div>
        </div>
        <p v-if="!assetsView.length" class="cv-hint">素材中心还没有素材——先去「AI 素材中心」生成图片/视频，再回来贴。</p>
      </NModal>

      <!-- 编辑弹层 -->
      <NodeEditDialog v-model:show="editShow" :story="story" :node-ref="editRef" :pro="pro" @save="onEditSave" />

      <!-- 批注输入 -->
      <NModal :show="flagShow" preset="dialog" title="标记待改" style="max-width: 460px" @close="flagShow = false">
        <NInput v-model:value="flagText" type="textarea" :rows="2" placeholder="哪里不行？写一句（如：这句台词太生硬）" />
        <template #action>
          <NButton size="small" @click="flagShow = false">取消</NButton>
          <NButton size="small" type="primary" @click="() => { if (flagInputNode && flagText.trim()) setFlag(flagInputNode, flagText.trim()); flagShow = false; flagText = ''; }">
            落旗
          </NButton>
        </template>
      </NModal>

      <!-- 便签编辑 -->
      <NModal :show="noteEdit.show" preset="dialog" title="便签" style="max-width: 420px" @close="noteEdit.show = false">
        <NInput v-model:value="noteEdit.text" type="textarea" :rows="3" />
        <template #action>
          <NButton size="small" @click="noteEdit.show = false">取消</NButton>
          <NButton size="small" type="primary" @click="saveNote">保存</NButton>
        </template>
      </NModal>

      <!-- 预览抽屉 -->
      <PreviewDrawer v-model:show="previewShow" :project-id="project ?? ''" :preview-html="previewHtml"
        :story="story" :clips="clips" :vo="voDone" :server-base="serverBase" :flags="board.flags"
        @highlight="onPreviewHighlight" @flag="setFlag" @unflag="clearFlag" />

      <!-- 首启导览 -->
      <NModal :show="onboardShow" preset="card" title="🎨 世界画布 · 30 秒上手" style="max-width: 460px">
        <div class="cv-onboard">
          <p><b>1 · 全貌</b>：剧情自动排成流向图，缺镜头/缺配音的节拍直接标出，缺口清单就是待办。</p>
          <p><b>2 · 点哪改哪</b>：双击节拍改文案；右键生成/定稿/批注；生成完自动挂上，版本可对比切换。</p>
          <p><b>3 · 立刻看得见</b>：▶ 预览逐镜过片（可标记待改），🎮 试玩就是导出同款播放器，体检全绿一键导出。</p>
          <p class="cv-hint">双击空白即建节点 · 节点右侧圆点拖线连接（可选写进剧情或仅作参考线）· 滚轮缩放 · 空格拖拽平移 · 右键更多操作。向导照常能用，两边数据同源。</p>
        </div>
        <template #footer>
          <NButton type="primary" block @click="finishOnboard">开始用</NButton>
        </template>
      </NModal>
    </template>
  </div>
</template>

<style scoped>
.cv-root { display: flex; flex-direction: column; height: 100%; min-height: 0; }
.cv-toolbar { display: flex; align-items: center; gap: 8px; padding-bottom: 10px; flex-wrap: wrap; }
.cv-toolbar .spacer { flex: 1; }
.cv-main { flex: 1; display: flex; gap: 12px; min-height: 0; }
.cv-side { width: 320px; flex: none; border: 1px solid #23232b; border-radius: 10px; background: #101014; overflow: hidden; }
.cv-pane { padding: 8px 10px; overflow-y: auto; max-height: calc(100vh - 210px); }
.cv-sec-title { font-size: 12px; color: #93a7b4; margin: 10px 0 4px; font-weight: 600; }
.cv-row { display: flex; align-items: center; gap: 6px; padding: 3px 0; font-size: 12px; }
.cv-row-text { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #93a7b4; }
.cv-task-body { flex: 1; min-width: 0; }
.cv-err { font-size: 11px; color: #ff5d5d; margin-top: 2px; }
.cv-hint { font-size: 11px; color: #5b6474; line-height: 1.6; }
.cv-clip { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cv-thumb { width: 64px; height: 36px; object-fit: cover; border-radius: 4px; flex: none; background: #0a0a0c; }
.cv-thumb-missing { display: flex; align-items: center; justify-content: center; font-size: 11px; color: #5b6474; }
.cv-kind-chip { width: 20px; text-align: center; flex: none; }
.mono { font-family: Consolas, monospace; color: #71717c; }
.cv-onboard p { font-size: 13px; line-height: 1.8; color: #b9c7d4; margin: 6px 0; }
.cv-pick-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; max-height: 340px; overflow-y: auto; }
.cv-pick-item { display: flex; flex-direction: column; gap: 4px; cursor: pointer; padding: 6px; border: 1px solid #23232b; border-radius: 8px; }
.cv-pick-item:hover { border-color: #4f8ae8; }
.cv-pick-thumb { width: 100%; height: 72px; object-fit: cover; border-radius: 6px; background: #0a0a0c; pointer-events: none; }
</style>
