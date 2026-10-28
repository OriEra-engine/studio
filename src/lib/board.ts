// 世界画布数据模型（PLAN-canvas v2.2）——schema 唯一真源在 TS，Rust 侧 canvas.rs
// 只做透明存取。铁律：剧情内容单一真源是 film/flow.json 的 story 字段，画布
// 连边/节拍文本一律派生，本文件只存布局、版本候选组、生成任务档案与看片批注。

export interface BoardViewport {
  x: number;
  y: number;
  scale: number;
}

/** 节拍在画布上的投影：只有坐标与定稿锁，永不复制剧情文本 */
export interface BoardNodePos {
  ref: string;
  x: number;
  y: number;
  /** 定稿锁（PGC 评审）：锁定后改文案/换候选需确认 */
  locked?: boolean;
}

/** 版本候选：一次生成的留档。pinned 之外的候选随时可比对回切 */
export interface Candidate {
  id: string;
  /** 素材库相对路径（assets/xxx.mp4），与 flow.clips 同约定 */
  src: string;
  from: "ai" | "import";
  prompt?: string;
  model?: string;
  created: number;
  tokens?: number | null;
}

export interface Slot {
  candidates: Candidate[];
  pinned?: string | null;
}

export type TaskStatus = "queued" | "running" | "done" | "failed";

/** 生成任务档案（"AI 原生可审计"的原料，导出时进 MANIFEST.json） */
export interface AiTaskRecord {
  id: string;
  kind: "shot" | "vo";
  /** shotId / voId，形如 `{node}_{i}` */
  slot: string;
  prompt: string;
  model?: string;
  params?: Record<string, unknown>;
  status: TaskStatus;
  started?: number;
  finished?: number;
  error?: string;
  tokens?: number | null;
}

export interface BoardNote {
  id: string;
  text: string;
  x: number;
  y: number;
  /** 图片卡：素材库相对路径（assets/…）或内置服务器绝对 URL；有值即渲染为图片卡 */
  src?: string;
}

/** 自由连线（仅画布层，不写剧情）：用户自由发挥的"参考线" */
export interface FreeEdge {
  id: string;
  from: string;
  to: string;
  label?: string;
}

export interface BoardFile {
  version: 1;
  meta: { name: string; viewport: BoardViewport };
  nodes: BoardNodePos[];
  slots: Record<string, Slot>;
  notes: BoardNote[];
  tasks: Record<string, AiTaskRecord>;
  /** 看片批注（M2 拍板③）：storyNodeRef → 批注文本 */
  flags: Record<string, string>;
  /** 自由连线：画布自由发挥层，永不影响 story */
  freeEdges: FreeEdge[];
}

export function defaultBoard(name = "世界画布"): BoardFile {
  return {
    version: 1,
    meta: { name, viewport: { x: 80, y: 60, scale: 1 } },
    nodes: [],
    slots: {},
    notes: [],
    tasks: {},
    flags: {},
    freeEdges: [],
  };
}

/** 脏数据兜底：任意来路 JSON 规整成合法 BoardFile（缺省补默认，类型错丢弃） */
export function sanitizeBoard(v: unknown): BoardFile {
  const b = defaultBoard();
  if (!v || typeof v !== "object") return b;
  const o = v as Record<string, any>;
  if (o.meta && typeof o.meta === "object") {
    b.meta.name = typeof o.meta.name === "string" ? o.meta.name : b.meta.name;
    const vp = o.meta.viewport;
    if (vp && Number.isFinite(vp.x) && Number.isFinite(vp.y) && Number.isFinite(vp.scale)) {
      b.meta.viewport = { x: vp.x, y: vp.y, scale: Math.min(3, Math.max(0.15, vp.scale)) };
    }
  }
  if (Array.isArray(o.nodes)) {
    b.nodes = o.nodes
      .filter((n: any) => n && typeof n.ref === "string" && Number.isFinite(n.x) && Number.isFinite(n.y))
      .map((n: any) => ({ ref: n.ref, x: n.x, y: n.y, locked: !!n.locked }));
  }
  if (o.slots && typeof o.slots === "object") {
    for (const [k, s] of Object.entries<any>(o.slots)) {
      if (!s || !Array.isArray(s.candidates)) continue;
      b.slots[k] = {
        candidates: s.candidates
          .filter((c: any) => c && typeof c.id === "string" && typeof c.src === "string")
          .map((c: any) => ({
            id: c.id, src: c.src, from: c.from === "import" ? ("import" as const) : ("ai" as const),
            prompt: typeof c.prompt === "string" ? c.prompt : undefined,
            model: typeof c.model === "string" ? c.model : undefined,
            created: Number(c.created) || 0,
            tokens: typeof c.tokens === "number" ? c.tokens : null,
          })),
        pinned: typeof s.pinned === "string" ? s.pinned : null,
      };
    }
  }
  if (Array.isArray(o.notes)) {
    b.notes = o.notes.filter((n: any) => n && typeof n.id === "string").map((n: any) => ({
      id: n.id, text: String(n.text ?? ""), x: Number(n.x) || 0, y: Number(n.y) || 0,
      src: typeof n.src === "string" && n.src ? n.src : undefined,
    }));
  }
  if (Array.isArray(o.freeEdges)) {
    b.freeEdges = o.freeEdges.filter((e: any) => e && typeof e.id === "string" && typeof e.from === "string" && typeof e.to === "string")
      .map((e: any) => ({ id: e.id, from: e.from, to: e.to, label: typeof e.label === "string" ? e.label : undefined }));
  }
  if (o.tasks && typeof o.tasks === "object") {
    for (const [k, t] of Object.entries<any>(o.tasks)) {
      if (!t || typeof t.id !== "string") continue;
      if (t.status === "running" || t.status === "queued") {
        // 应用重启的残留任务：标记中断，用户可一键重跑
        b.tasks[k] = { ...t, status: "failed", error: "应用重启，任务中断——可重跑" };
      } else {
        b.tasks[k] = t;
      }
    }
  }
  if (o.flags && typeof o.flags === "object") {
    for (const [k, f] of Object.entries<any>(o.flags)) {
      if (typeof f === "string" && f) b.flags[k] = f;
    }
  }
  return b;
}

let seq = 0;
export function uid(prefix: string): string {
  seq += 1;
  return `${prefix}_${Date.now().toString(36)}_${seq.toString(36)}`;
}

export function pinnedOf(slot: Slot | undefined): Candidate | undefined {
  if (!slot) return undefined;
  return slot.candidates.find((c) => c.id === slot.pinned) ?? slot.candidates[slot.candidates.length - 1];
}

export interface BoardFinding {
  severity: "error" | "warning";
  kind: string;
  node?: string;
  detail: string;
}

/** 画布层体检：只查画布自己引入的问题；剧情体检走 FilmStory.validate，不建第二套政权 */
export function boardFindings(
  board: BoardFile,
  clips: Record<string, string>,
): BoardFinding[] {
  const out: BoardFinding[] = [];
  const running = Object.values(board.tasks).filter((t) => t.status === "running" || t.status === "queued");
  if (running.length) {
    out.push({ severity: "warning", kind: "task_running", detail: `还有 ${running.length} 个生成任务未完成` });
  }
  const failed = Object.values(board.tasks).filter((t) => t.status === "failed");
  if (failed.length) {
    out.push({ severity: "warning", kind: "task_failed", detail: `${failed.length} 个任务失败待处理` });
  }
  for (const [slot, s] of Object.entries(board.slots)) {
    if (s.pinned && !s.candidates.some((c) => c.id === s.pinned)) {
      out.push({ severity: "warning", kind: "dangling_pin", node: slot, detail: "采纳的候选已被删除" });
    }
  }
  for (const [slot, file] of Object.entries(clips)) {
    if (!file) out.push({ severity: "warning", kind: "empty_clip", node: slot, detail: "镜头素材路径为空" });
  }
  return out;
}
