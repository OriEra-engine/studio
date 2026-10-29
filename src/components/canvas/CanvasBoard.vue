<script setup lang="ts">
// 画布表面（PLAN-canvas v2.2 §3.1）：单容器 CSS transform + 可视区剔除 +
// 派生连边 SVG + 小地图。剧情内容一律由 props 投影进来，本组件不持有剧情。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import type { AiTaskRecord, BoardNodePos, BoardNote, BoardViewport, FreeEdge } from "../../lib/board";
import { KIND_LABEL, storyEdges, type RawStory } from "../../lib/canvas/storywalk";
import { NODE_W, nodeHeight, useViewport } from "../../lib/canvas/layout";

const props = defineProps<{
  story: RawStory | null;
  nodes: BoardNodePos[];
  notes: BoardNote[];
  freeEdges: FreeEdge[];
  clips: Record<string, string>;
  vo: Record<string, string>;
  tasks: Record<string, AiTaskRecord>;
  flags: Record<string, string>;
  selected: string[];
  vp: BoardViewport;
  pro: boolean;
  serverBase: string;
  highlightNode: string | null;
}>();

const emit = defineEmits<{
  (e: "update:selected", ids: string[]): void;
  (e: "begin-structural"): void;
  (e: "commit"): void;
  (e: "open-node", ref: string): void;
  (e: "menu", payload: { kind: "node" | "note" | "bg" | "edge"; ref?: string; noteId?: string; edgeId?: string; x: number; y: number }): void;
  (e: "vp-change"): void;
  (e: "connect", payload: { from: string; to: string; x: number; y: number }): void;
  (e: "create-at", payload: { wx: number; wy: number; x: number; y: number }): void;
}>();

const { vp, screenToWorld, zoomAt, panBy, centerOn, fitAll } = useViewport(props.vp);
// props.vp 是父级 board.meta.viewport 的同一 reactive 代理：reactive(代理) 原样返回，
// 这里的平移缩放直接写穿到父级画布数据，父级 watch 防抖落盘。

const stageEl = ref<HTMLDivElement | null>(null);
const stageW = ref(800);
const stageH = ref(600);
let ro: ResizeObserver | null = null;
onMounted(() => {
  ro = new ResizeObserver((entries) => {
    const r = entries[0]?.contentRect;
    if (r) { stageW.value = r.width; stageH.value = r.height; }
  });
  if (stageEl.value) ro.observe(stageEl.value);
  window.addEventListener("keydown", onWinKey);
  window.addEventListener("keyup", onWinKey);
});
onBeforeUnmount(() => {
  ro?.disconnect();
  window.removeEventListener("keydown", onWinKey);
  window.removeEventListener("keyup", onWinKey);
});

/* ── 节点投影：位置 + 元数据合流；>200 节点做可视区剔除（v2.1 §1.1-9） ── */
const byRef = computed(() => new Map((props.story?.nodes ?? []).map((n) => [n.id, n])));
const nodesView = computed(() =>
  props.nodes
    .map((p) => {
      const n = byRef.value.get(p.ref); // computed 必须走 .value——漏写会让渲染期抛错冻结整个子树
      if (!n) return null;
      return { pos: p, node: n, w: NODE_W, h: nodeHeight(n) };
    })
    .filter((x): x is NonNullable<typeof x> => !!x)
    .sort((a, b) => a.pos.ref.localeCompare(b.pos.ref)),
);
const culled = computed(() => {
  if (nodesView.value.length <= 200) return nodesView.value;
  const x0 = -vp.x / vp.scale - 300;
  const y0 = -vp.y / vp.scale - 400;
  const x1 = (stageW.value - vp.x) / vp.scale + 300;
  const y1 = (stageH.value - vp.y) / vp.scale + 400;
  return nodesView.value.filter((n) => n.pos.x + n.w > x0 && n.pos.x < x1 && n.pos.y + n.h > y0 && n.pos.y < y1);
});

const worldStyle = computed(() => ({
  transform: `translate(${vp.x}px, ${vp.y}px) scale(${vp.scale})`,
  transformOrigin: "0 0",
}));

/* ── 派生连边（剧情单一真源）── */
const edges = computed(() => {
  if (!props.story) return [];
  return storyEdges(props.story)
    .map((e) => {
      const a = nodesView.value.find((n) => n.pos.ref === e.from);
      const b = nodesView.value.find((n) => n.pos.ref === e.to);
      if (!a || !b) return null;
      const x1 = a.pos.x + a.w;
      const y1 = a.pos.y + Math.min(56, a.h / 2);
      const x2 = b.pos.x;
      const y2 = b.pos.y + Math.min(56, b.h / 2);
      const dx = Math.max(46, Math.abs(x2 - x1) / 2);
      return { key: `${e.from}->${e.to}:${e.label ?? ""}`, d: `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`, label: e.label };
    })
    .filter((x): x is NonNullable<typeof x> => !!x);
});

/* ── 节点状态合成（镜头/配音/任务/批注）── */
function shotState(shotId: string, nodeId: string) {
  const clip = props.clips[shotId];
  const running = Object.values(props.tasks).some((t) => t.slot === shotId && (t.status === "running" || t.status === "queued"));
  const failed = Object.values(props.tasks).filter((t) => t.slot === shotId && t.status === "failed").length;
  return { clip, clipUrl: clip ? assetUrl(clip) : null, running, failed, voDone: !!props.vo[shotId] };
}
function nodeFailedCount(ref: string) {
  return Object.values(props.tasks).filter((t) => t.status === "failed" && t.slot.startsWith(`${ref}_`)).length;
}
function assetUrl(src: string) {
  return src.startsWith("http") ? src : `${props.serverBase}/${src.replace(/^\/+/, "")}`;
}
function clipSrcOf(shotId: string): string | null {
  const c = props.clips[shotId];
  return c ? assetUrl(c) : null;
}
function selectedSet(id: string) { return props.selected.includes(id); }

/* ── 交互：平移 / 框选 / 节点拖拽 / 便签拖拽 ── */
const spaceHeld = ref(false);
function onWinKey(e: KeyboardEvent) {
  if (e.type === "keydown" && e.code === "Space" && !isTyping(e)) { spaceHeld.value = true; }
  if (e.type === "keyup" && e.code === "Space") { spaceHeld.value = false; }
  if (e.type === "keydown" && e.key === "Escape") { emit("update:selected", []); rubber.value = null; connecting.value = null; }
}
function isTyping(e: Event) {
  const t = e.target as HTMLElement | null;
  return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
}

/* ── 自由连线：从节点端口拖出，落在目标节点上抛 connect（意图由父级决定） ── */
const connecting = ref<{ from: string; wx: number; wy: number } | null>(null);
function onPortDown(refId: string, e: PointerEvent) {
  if (e.button !== 0) return;
  e.stopPropagation();
  const p = localXY(e);
  const w = screenToWorld(p.x, p.y);
  connecting.value = { from: refId, wx: w.x, wy: w.y };
  window.addEventListener("pointermove", onConnMove);
  window.addEventListener("pointerup", onConnUp, { once: true });
}
function onConnMove(e: PointerEvent) {
  if (!connecting.value) return;
  const p = localXY(e);
  const w = screenToWorld(p.x, p.y);
  connecting.value.wx = w.x;
  connecting.value.wy = w.y;
}
function onConnUp(e: PointerEvent) {
  window.removeEventListener("pointermove", onConnMove);
  const conn = connecting.value;
  connecting.value = null;
  if (!conn) return;
  const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
  const hit = el?.closest("[data-ref]") as HTMLElement | null;
  const to = hit?.getAttribute("data-ref") ?? null;
  if (to && to !== conn.from) emit("connect", { from: conn.from, to, x: e.clientX, y: e.clientY });
}

/* ── 双击空白：就地新建（坐标换算成世界坐标交父级落点） ── */
function onStageDblclick(e: MouseEvent) {
  const t = e.target as HTMLElement | null;
  if (t && (t.closest(".cb-node") || t.closest(".cb-note") || t.closest(".cb-port") || t.closest(".cb-minimap"))) return;
  const p = localXY(e);
  const w = screenToWorld(p.x, p.y);
  emit("create-at", { wx: Math.round(w.x), wy: Math.round(w.y), x: e.clientX, y: e.clientY });
}

/* ── 自由连线渲染：虚线橙色，命中区加宽供右键删除 ── */
const freeEdgePaths = computed(() => {
  return props.freeEdges
    .map((fe) => {
      const a = nodesView.value.find((x) => x.pos.ref === fe.from);
      const b = nodesView.value.find((x) => x.pos.ref === fe.to);
      if (!a || !b) return null;
      const x1 = a.pos.x + a.w;
      const y1 = a.pos.y + Math.min(56, a.h / 2);
      const x2 = b.pos.x;
      const y2 = b.pos.y + Math.min(56, b.h / 2);
      const dx = Math.max(46, Math.abs(x2 - x1) / 2);
      const d = `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
      return { id: fe.id, d, label: fe.label ?? "" };
    })
    .filter((x): x is NonNullable<typeof x> => !!x);
});
function onEdgeMenu(edgeId: string, e: MouseEvent) {
  emit("menu", { kind: "edge", edgeId, x: e.clientX, y: e.clientY });
}
const connFromX = computed(() => {
  const a = nodesView.value.find((x) => x.pos.ref === connecting.value?.from);
  return a ? a.pos.x + a.w : 0;
});
const connFromY = computed(() => {
  const a = nodesView.value.find((x) => x.pos.ref === connecting.value?.from);
  return a ? a.pos.y + Math.min(56, a.h / 2) : 0;
});

type DragMode =
  | { kind: "pan"; sx: number; sy: number; ox: number; oy: number }
  | { kind: "rubber"; sx: number; sy: number }
  | { kind: "move"; sx: number; sy: number; orig: Map<string, { x: number; y: number }>; moved: boolean; what: "node" | "note" };
let drag: DragMode | null = null;
const rubber = ref<{ x: number; y: number; w: number; h: number } | null>(null);

function localXY(e: PointerEvent | MouseEvent) {
  const r = stageEl.value!.getBoundingClientRect();
  return { x: e.clientX - r.left, y: e.clientY - r.top };
}

function onWheel(e: WheelEvent) {
  e.preventDefault();
  const p = localXY(e);
  zoomAt(p.x, p.y, e.deltaY > 0 ? 0.9 : 1.1);
  emit("vp-change");
}

function onBgDown(e: PointerEvent) {
  if (e.button === 1 || (e.button === 0 && spaceHeld.value)) {
    drag = { kind: "pan", sx: e.clientX, sy: e.clientY, ox: vp.x, oy: vp.y };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp, { once: true });
    return;
  }
  if (e.button !== 0) return;
  const p = localXY(e);
  drag = { kind: "rubber", sx: p.x, sy: p.y };
  rubber.value = { x: p.x, y: p.y, w: 0, h: 0 };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp, { once: true });
}

function onNodeDown(p: { pos: BoardNodePos }, e: PointerEvent, what: "node" | "note") {
  if (e.button !== 0 || spaceHeld.value) return;
  e.stopPropagation();
  const id = what === "node" ? p.pos.ref : (p as unknown as BoardNote).id;
  let sel = props.selected;
  if (e.shiftKey || e.ctrlKey || e.metaKey) {
    sel = sel.includes(id) ? sel.filter((s) => s !== id) : [...sel, id];
    emit("update:selected", sel);
  } else if (!sel.includes(id)) {
    sel = [id];
    emit("update:selected", sel);
  }
  const ids = what === "node" ? sel.filter((s) => byRef.value.has(s)) : sel.filter((s) => props.notes.some((n) => n.id === s));
  const orig = new Map<string, { x: number; y: number }>();
  const pool: { key: string; x: number; y: number }[] =
    what === "node"
      ? props.nodes.map((n) => ({ key: n.ref, x: n.x, y: n.y }))
      : props.notes.map((n) => ({ key: n.id, x: n.x, y: n.y }));
  for (const item of pool) if (ids.includes(item.key)) orig.set(item.key, { x: item.x, y: item.y });
  drag = { kind: "move", sx: e.clientX, sy: e.clientY, orig, moved: false, what };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp, { once: true });
}

function onMove(e: PointerEvent) {
  if (!drag) return;
  if (drag.kind === "pan") {
    vp.x = drag.ox + (e.clientX - drag.sx);
    vp.y = drag.oy + (e.clientY - drag.sy);
    return;
  }
  if (drag.kind === "rubber") {
    const p = localXY(e);
    rubber.value = {
      x: Math.min(drag.sx, p.x), y: Math.min(drag.sy, p.y),
      w: Math.abs(p.x - drag.sx), h: Math.abs(p.y - drag.sy),
    };
    return;
  }
  const dx = (e.clientX - drag.sx) / vp.scale;
  const dy = (e.clientY - drag.sy) / vp.scale;
  if (!drag.moved && Math.abs(dx) + Math.abs(dy) > 2) {
    drag.moved = true;
    emit("begin-structural"); // 首次真实移动时入撤销栈，避免空拖占用步数
  }
  for (const [key, o] of drag.orig) {
    if (drag.what === "node") {
      const hit = props.nodes.find((n) => n.ref === key);
      if (hit) { hit.x = Math.round(o.x + dx); hit.y = Math.round(o.y + dy); }
    } else {
      const hit = props.notes.find((n) => n.id === key);
      if (hit) { hit.x = Math.round(o.x + dx); hit.y = Math.round(o.y + dy); }
    }
  }
}

function onUp() {
  window.removeEventListener("pointermove", onMove);
  if (!drag) return;
  if (drag.kind === "pan") {
    emit("vp-change");
  } else if (drag.kind === "rubber") {
    const r = rubber.value;
    if (r && r.w + r.h > 8) {
      const hits = nodesView.value
        .filter((n) => {
          const sx = n.pos.x * vp.scale + vp.x;
          const sy = n.pos.y * vp.scale + vp.y;
          return sx < r.x + r.w && sx + n.w * vp.scale > r.x && sy < r.y + r.h && sy + n.h * vp.scale > r.y;
        })
        .map((n) => n.pos.ref);
      emit("update:selected", hits);
    } else {
      emit("update:selected", []);
    }
    rubber.value = null;
  } else if (drag.kind === "move") {
    if (drag.moved) emit("commit");
  }
  drag = null;
}

function onBgMenu(e: MouseEvent) {
  emit("menu", { kind: "bg", x: e.clientX, y: e.clientY });
}
function onNodeMenu(ref: string, e: MouseEvent) {
  emit("menu", { kind: "node", ref, x: e.clientX, y: e.clientY });
}
function onNoteMenu(noteId: string, e: MouseEvent) {
  emit("menu", { kind: "note", noteId, x: e.clientX, y: e.clientY });
}

/* ── 小地图：世界 bbox → 176×116 视窗，点击跳转 ── */
const MM_W = 176;
const MM_H = 116;
const mmBox = computed(() => {
  const xs = nodesView.value.map((n) => n.pos.x);
  const ys = nodesView.value.map((n) => n.pos.y);
  if (!xs.length) return { minX: 0, minY: 0, w: 1, h: 1, k: 0.2 };
  const minX = Math.min(...xs) - 60;
  const minY = Math.min(...ys) - 60;
  const w = Math.max(1, Math.max(...xs.map((x, i) => x + nodesView.value[i].w)) + 60 - minX);
  const h = Math.max(1, Math.max(...ys.map((y, i) => y + nodesView.value[i].h)) + 60 - minY);
  return { minX, minY, w, h, k: Math.min(MM_W / w, MM_H / h) };
});
const mmNodes = computed(() =>
  nodesView.value.map((n) => ({
    ref: n.pos.ref,
    kind: n.node.type,
    x: (n.pos.x - mmBox.value.minX) * mmBox.value.k,
    y: (n.pos.y - mmBox.value.minY) * mmBox.value.k,
    w: Math.max(3, n.w * mmBox.value.k),
    h: Math.max(2, n.h * mmBox.value.k),
  })),
);
const mmView = computed(() => ({
  x: (-vp.x / vp.scale - mmBox.value.minX) * mmBox.value.k,
  y: (-vp.y / vp.scale - mmBox.value.minY) * mmBox.value.k,
  w: (stageW.value / vp.scale) * mmBox.value.k,
  h: (stageH.value / vp.scale) * mmBox.value.k,
}));
function mmClick(e: MouseEvent) {
  const r = (e.currentTarget as SVGElement).getBoundingClientRect();
  const wx = (e.clientX - r.left) / mmBox.value.k + mmBox.value.minX;
  const wy = (e.clientY - r.top) / mmBox.value.k + mmBox.value.minY;
  centerOn(wx, wy, stageW.value, stageH.value);
  emit("vp-change");
}

/* ── 对外动作 ── */
function focusNode(ref: string) {
  const n = nodesView.value.find((x) => x.pos.ref === ref);
  if (n) centerOn(n.pos.x + n.w / 2, n.pos.y + n.h / 2, stageW.value, stageH.value);
}
function fit() {
  fitAll(nodesView.value.map((n) => ({ x: n.pos.x, y: n.pos.y, w: n.w, h: n.h })), stageW.value, stageH.value);
}
defineExpose({ focusNode, fit, stageSize: () => ({ w: stageW.value, h: stageH.value }) });

const KIND_COLOR: Record<string, string> = {
  scene: "#4f8ae8", choice: "#ff9a3c", hub: "#9a7ff0", ending: "#4fc38a",
};
</script>

<template>
  <div
    ref="stageEl" class="cb-stage"
    :class="{ panning: spaceHeld }"
    @wheel="onWheel" @pointerdown="onBgDown" @contextmenu.prevent="onBgMenu"
    @dblclick="onStageDblclick"
  >
    <div class="cb-world" :style="worldStyle">
      <svg class="cb-edges" :width="1" :height="1">
        <defs>
          <marker id="cb-arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
            <path d="M0 0 L8 4 L0 8 z" fill="#5b6474" />
          </marker>
          <marker id="cb-arrow-free" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
            <path d="M0 0 L8 4 L0 8 z" fill="#ff9a3c" />
          </marker>
        </defs>
        <path v-for="e in edges" :key="e.key" :d="e.d" fill="none" stroke="#5b6474" stroke-width="1.6"
          marker-end="url(#cb-arrow)" :title="e.label" />
        <!-- 自由连线（仅画布层）：虚线橙，可右键删除 -->
        <g v-for="fe in freeEdgePaths" :key="fe.id">
          <path :d="fe.d" fill="none" stroke="transparent" stroke-width="14" style="cursor:pointer"
            @contextmenu.prevent.stop="onEdgeMenu(fe.id, $event)" />
          <path :d="fe.d" fill="none" stroke="#ff9a3c" stroke-width="1.6" stroke-dasharray="6 5"
            marker-end="url(#cb-arrow-free)" :title="`参考连线${fe.label ? ' · ' + fe.label : ''}（右键删除，不写剧情）`" />
        </g>
        <!-- 拖线中的临时线 -->
        <path v-if="connecting" :d="`M ${connFromX} ${connFromY} L ${connecting.wx} ${connecting.wy}`"
          fill="none" stroke="#ff9a3c" stroke-width="2" stroke-dasharray="4 4" />
      </svg>

      <div
        v-for="n in culled" :key="n.pos.ref"
        class="cb-node"
        :data-ref="n.pos.ref"
        :class="[`cb-${n.node.type}`, {
          sel: selectedSet(n.pos.ref),
          hl: highlightNode === n.pos.ref,
          locked: n.pos.locked,
        }]"
        :style="{ left: n.pos.x + 'px', top: n.pos.y + 'px', width: n.w + 'px', height: n.h + 'px' }"
        @pointerdown="onNodeDown(n, $event, 'node')"
        @dblclick.stop="emit('open-node', n.pos.ref)"
        @contextmenu.prevent.stop="onNodeMenu(n.pos.ref, $event)"
      >
        <span class="cb-port" title="拖到目标节点连线" @pointerdown.stop.prevent="onPortDown(n.pos.ref, $event)"></span>
        <div class="cb-node-head">
          <span class="cb-kind" :style="{ color: KIND_COLOR[n.node.type] }">{{ KIND_LABEL[n.node.type] || n.node.type }}</span>
          <span class="cb-title">{{ n.node.title || n.pos.ref }}</span>
          <span v-if="pro" class="cb-id mono">{{ n.pos.ref }}</span>
          <span v-if="n.pos.locked" title="已定稿锁定">🔒</span>
          <span v-if="flags[n.pos.ref]" class="cb-flag" :title="flags[n.pos.ref]">🚩</span>
          <span v-if="nodeFailedCount(n.pos.ref)" class="cb-fail" :title="`${nodeFailedCount(n.pos.ref)} 个任务失败`">●</span>
        </div>
        <div v-if="n.node.type === 'scene'" class="cb-body">
          <p v-if="n.node.text" class="cb-text">{{ n.node.text }}</p>
          <div v-for="(s, i) in n.node.shots ?? []" :key="i" class="cb-shot">
            <video v-if="clipSrcOf(`${n.pos.ref}_${i}`)" class="cb-thumb" :src="clipSrcOf(`${n.pos.ref}_${i}`)!"
              muted preload="metadata" @pointerdown.stop @dblclick.stop />
            <span v-else class="cb-thumb cb-thumb-missing">{{ shotState(`${n.pos.ref}_${i}`, n.pos.ref).running ? "⏳" : "空" }}</span>
            <span class="cb-shot-meta">
              <code class="mono">{{ n.pos.ref }}_{{ i }}</code>
              <em>{{ s.dur ?? 5 }}s</em>
              <span v-if="s.vo" class="cb-vo" :class="{ done: shotState(`${n.pos.ref}_${i}`, n.pos.ref).voDone }">🎙</span>
              <span v-if="shotState(`${n.pos.ref}_${i}`, n.pos.ref).failed" class="cb-fail">●</span>
            </span>
          </div>
        </div>
        <div v-else-if="n.node.type === 'choice'" class="cb-body">
          <div v-for="(o, i) in n.node.options ?? []" :key="i" class="cb-option">
            <span class="cb-arrow">→</span>{{ o.text }} <code class="mono dim">{{ o.goto }}</code>
          </div>
        </div>
        <div v-else-if="n.node.type === 'hub'" class="cb-body">
          <div v-for="(r, i) in n.node.routes ?? []" :key="i" class="cb-option">
            <code class="mono dim">{{ r.when ?? "默认" }}</code> → <code class="mono dim">{{ r.goto }}</code>
          </div>
        </div>
        <div v-else class="cb-body">
          <p v-if="n.node.text" class="cb-text">{{ n.node.text }}</p>
          <span class="cb-ending-chip">结局 · 按变量结算</span>
        </div>
      </div>

      <div
        v-for="nt in notes" :key="nt.id"
        class="cb-note"
        :class="{ sel: selectedSet(nt.id), img: !!nt.src }"
        :style="{ left: nt.x + 'px', top: nt.y + 'px' }"
        @pointerdown="onNodeDown(nt as any, $event, 'note')"
        @contextmenu.prevent.stop="onNoteMenu(nt.id, $event)"
      >
        <img v-if="nt.src" class="cb-note-img" :src="nt.src.startsWith('http') ? nt.src : serverBase + '/' + nt.src.replace(/^\/+/, '')" draggable="false" @pointerdown.stop />
        <span>{{ nt.text }}</span>
      </div>
    </div>

    <div v-if="rubber" class="cb-rubber" :style="{ left: rubber.x + 'px', top: rubber.y + 'px', width: rubber.w + 'px', height: rubber.h + 'px' }" />

    <svg class="cb-minimap" :width="MM_W" :height="MM_H" @pointerdown.stop.prevent="mmClick" @wheel.prevent.stop>
      <rect :x="mmView.x" :y="mmView.y" :width="mmView.w" :height="mmView.h" fill="rgba(79,138,232,.12)" stroke="#4f8ae8" stroke-width="1" />
      <rect v-for="m in mmNodes" :key="m.ref" :x="m.x" :y="m.y" :width="m.w" :height="m.h" rx="1"
        :fill="KIND_COLOR[m.kind] || '#888'" opacity="0.75" />
    </svg>
  </div>
</template>

<style scoped>
.cb-stage {
  position: relative; flex: 1; overflow: hidden; min-height: 0; cursor: default;
  background-color: #0b0b0e;
  background-image: radial-gradient(circle, #1c1c22 1px, transparent 1px);
  background-size: 26px 26px;
  user-select: none;
}
.cb-stage.panning { cursor: grab; }
.cb-world { position: absolute; left: 0; top: 0; will-change: transform; }
.cb-edges { position: absolute; overflow: visible; left: 0; top: 0; }
.cb-node {
  position: absolute; border-radius: 10px; background: #16161b;
  border: 1px solid #30303a; padding: 10px 12px; overflow: hidden;
  box-shadow: 0 4px 14px rgba(0,0,0,.35); cursor: grab;
}
.cb-node.sel { border-color: #4f8ae8; box-shadow: 0 0 0 2px rgba(79,138,232,.25), 0 4px 14px rgba(0,0,0,.35); }
.cb-node.hl { border-color: #ff9a3c; box-shadow: 0 0 0 2px rgba(255,154,60,.35); }
.cb-node.locked { border-style: solid; border-color: #4fc38a66; }
.cb-port {
  position: absolute; right: -7px; top: 50%; margin-top: -7px;
  width: 14px; height: 14px; border-radius: 50%;
  background: #16161b; border: 2px solid #ff9a3c;
  opacity: 0; transition: opacity .12s; cursor: crosshair; z-index: 3;
}
.cb-node:hover .cb-port { opacity: 1; }
.cb-port:hover { background: #ff9a3c; }
.cb-node-head { display: flex; align-items: center; gap: 6px; margin-bottom: 6px; min-width: 0; }
.cb-kind { font-size: 11px; flex: none; }
.cb-title { font-size: 13px; font-weight: 600; color: #dce8f2; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.cb-id { font-size: 10px; color: #5b6474; flex: none; }
.cb-flag { flex: none; }
.cb-fail { color: #ff5d5d; flex: none; }
.cb-body { font-size: 12px; color: #93a7b4; overflow: hidden; }
.cb-text { margin: 0 0 6px; color: #b9c7d4; line-height: 1.5; display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
.cb-shot { display: flex; align-items: center; gap: 8px; padding: 3px 0; }
.cb-thumb { width: 72px; height: 40px; object-fit: cover; border-radius: 4px; background: #0a0a0c; flex: none; border: 1px solid #23232b; }
.cb-thumb-missing { display: flex; align-items: center; justify-content: center; font-size: 11px; color: #5b6474; }
.cb-shot-meta { display: flex; align-items: center; gap: 6px; min-width: 0; }
.cb-shot-meta code { font-size: 10px; color: #71717c; }
.cb-shot-meta em { font-style: normal; font-size: 10px; color: #5b6474; }
.cb-vo { opacity: .4; }
.cb-vo.done { opacity: 1; }
.cb-option { padding: 2px 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.cb-arrow { color: #ff9a3c; margin-right: 4px; }
.cb-ending-chip { display: inline-block; font-size: 10px; color: #4fc38a; border: 1px solid #4fc38a44; border-radius: 99px; padding: 1px 8px; }
.cb-note {
  position: absolute; width: 180px; min-height: 60px; background: #2a2417;
  border: 1px solid #5a4c28; color: #e8d9ad; border-radius: 8px; padding: 8px 10px;
  font-size: 12px; line-height: 1.5; cursor: grab; box-shadow: 0 4px 14px rgba(0,0,0,.35);
}
.cb-note.img { width: 220px; padding: 6px; }
.cb-note-img { display: block; width: 100%; border-radius: 6px; margin-bottom: 6px; pointer-events: none; }
.cb-note.sel { border-color: #ff9a3c; }
.cb-rubber { position: absolute; border: 1px solid #4f8ae8; background: rgba(79,138,232,.12); pointer-events: none; }
.cb-minimap { position: absolute; right: 14px; bottom: 14px; background: rgba(11,11,14,.85); border: 1px solid #30303a; border-radius: 8px; cursor: crosshair; }
.mono { font-family: Consolas, monospace; }
.dim { color: #71717c; }
</style>
