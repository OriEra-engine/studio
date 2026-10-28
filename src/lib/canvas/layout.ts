// 画布布局与视口（PLAN-canvas v2.2 §3.1）：分层流向自动布局 + 光标锚点缩放。
// 纯函数 + 一个 reactive 视口，无第三方依赖（v2.1 拍死引库）。

import { reactive } from "vue";
import type { BoardNodePos, BoardViewport } from "../board";
import type { RawNode, RawStory } from "./storywalk";

/* ── 节点尺寸：布局与渲染共用同一估算，杜绝两处漂移 ── */
export const NODE_W = 252;
export function nodeHeight(n: RawNode): number {
  const shots = n.shots?.length ?? 0;
  switch (n.type) {
    case "scene": {
      const base = 96 + Math.min((n.text ?? "").length, 90) / 3;
      return Math.round(Math.min(300, base + shots * 46 + 14));
    }
    case "choice":
      return Math.round(74 + (n.options?.length ?? 0) * 30);
    case "hub":
      return Math.round(58 + (n.routes?.length ?? 0) * 24);
    case "ending":
      return 132;
    default:
      return 120;
  }
}

/** 分层流向布局：从入口 BFS 分层（深度=列），同列纵向堆叠。约 100 行内的手写算法 */
export function autoLayout(story: RawStory): BoardNodePos[] {
  const nodes = story.nodes ?? [];
  if (!nodes.length) return [];
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const outs = (n: RawNode): string[] => {
    const t = [n.goto ?? null, ...(n.options ?? []).map((o) => o.goto), ...(n.routes ?? []).map((r) => r.goto)];
    return t.filter((x): x is string => !!x && byId.has(x));
  };
  // BFS 分层（同一节点取最先到达的层，接近"主干深度"）
  const depth = new Map<string, number>();
  const entry = nodes[0].id;
  depth.set(entry, 0);
  const q = [entry];
  while (q.length) {
    const id = q.shift()!;
    const d = depth.get(id)!;
    for (const t of outs(byId.get(id)!)) {
      if (!depth.has(t)) {
        depth.set(t, d + 1);
        q.push(t);
      }
    }
  }
  // 不可达节点（体检会报）排到最后几列，不让它们堆在原点
  const maxD = Math.max(0, ...[...depth.values()]);
  for (const n of nodes) if (!depth.has(n.id)) depth.set(n.id, maxD + 1);

  const cols = new Map<number, RawNode[]>();
  for (const n of nodes) {
    const d = depth.get(n.id)!;
    if (!cols.has(d)) cols.set(d, []);
    cols.get(d)!.push(n);
  }
  const out: BoardNodePos[] = [];
  for (const [d, col] of [...cols.entries()].sort((a, b) => a[0] - b[0])) {
    let y = 70;
    for (const n of col) {
      out.push({ ref: n.id, x: 70 + d * (NODE_W + 88), y });
      y += nodeHeight(n) + 42;
    }
  }
  return out;
}

/* ── 视口：世界坐标 ⇄ 屏幕坐标，滚轮以光标为锚缩放 ── */
export const VP_MIN = 0.15;
export const VP_MAX = 3;

export function useViewport(initial?: BoardViewport) {
  const vp = reactive<BoardViewport>(
    initial ? { ...initial } : { x: 80, y: 60, scale: 1 },
  );
  function clampScale(s: number) {
    return Math.min(VP_MAX, Math.max(VP_MIN, s));
  }
  function screenToWorld(sx: number, sy: number) {
    return { x: (sx - vp.x) / vp.scale, y: (sy - vp.y) / vp.scale };
  }
  /** factor<1 放大（滚轮向下），>1 缩小 */
  function zoomAt(sx: number, sy: number, factor: number) {
    const ns = clampScale(vp.scale * factor);
    const w = screenToWorld(sx, sy);
    vp.scale = ns;
    vp.x = sx - w.x * ns;
    vp.y = sy - w.y * ns;
  }
  function panBy(dx: number, dy: number) {
    vp.x += dx;
    vp.y += dy;
  }
  function centerOn(wx: number, wy: number, viewW: number, viewH: number) {
    vp.x = viewW / 2 - wx * vp.scale;
    vp.y = viewH / 2 - wy * vp.scale;
  }
  function fitAll(
    rects: { x: number; y: number; w: number; h: number }[],
    viewW: number,
    viewH: number,
  ) {
    if (!rects.length || viewW <= 0 || viewH <= 0) return;
    const minX = Math.min(...rects.map((r) => r.x));
    const minY = Math.min(...rects.map((r) => r.y));
    const maxX = Math.max(...rects.map((r) => r.x + r.w));
    const maxY = Math.max(...rects.map((r) => r.y + r.h));
    const pad = 60;
    const scale = clampScale(
      Math.min((viewW - pad * 2) / Math.max(1, maxX - minX), (viewH - pad * 2) / Math.max(1, maxY - minY), 1.2),
    );
    vp.scale = scale;
    vp.x = viewW / 2 - ((minX + maxX) / 2) * scale;
    vp.y = viewH / 2 - ((minY + maxY) / 2) * scale;
  }
  return { vp, screenToWorld, zoomAt, panBy, centerOn, fitAll, clampScale };
}
