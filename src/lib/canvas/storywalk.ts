// story 派生层：画布对 film/flow.json story 字段的只读投影。
// story JSON 是白名单外的富格式（shots/vo/scene…），FilmStory.parse 会丢字段，
// 这里保留原始对象（画布只读不改写结构；编辑弹层改完整体回写字符串）。

export interface RawShot {
  frame?: string;
  motion?: string;
  dur?: number;
  vo?: string;
  [k: string]: unknown;
}

export interface RawNode {
  id: string;
  type: string; // scene | choice | hub | ending
  title?: string;
  scene?: string;
  text?: string;
  shots?: RawShot[];
  goto?: string | null;
  options?: { text: string; set?: Record<string, string> | null; goto: string }[];
  routes?: { when?: string | null; goto: string }[];
  timeout?: number | null;
  [k: string]: unknown;
}

export interface RawStory {
  meta?: Record<string, unknown> | null;
  vars?: Record<string, unknown> | null;
  nodes: RawNode[];
}

export function parseStory(text: string): RawStory {
  const d = JSON.parse(text);
  if (!d || !Array.isArray(d.nodes)) throw new Error("story 缺少 nodes 数组");
  return d as RawStory;
}

export function tryParseStory(text: string): { ok: true; story: RawStory } | { ok: false; error: string } {
  try {
    return { ok: true, story: parseStory(text) };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}

export interface ShotRef {
  id: string; // `{node}_{i}`
  node: string;
  index: number;
  frame: string;
  motion: string;
  dur: number;
  vo: string;
}

export function storyShots(story: RawStory): ShotRef[] {
  const out: ShotRef[] = [];
  for (const n of story.nodes ?? []) {
    (n.shots ?? []).forEach((s, i) => {
      out.push({
        id: `${n.id}_${i}`,
        node: n.id,
        index: i,
        frame: String(s.frame ?? ""),
        motion: String(s.motion ?? ""),
        dur: Number(s.dur) || 5,
        vo: String(s.vo ?? ""),
      });
    });
  }
  return out;
}

export interface VoRef {
  id: string;
  node: string;
  role: string;
  text: string;
}

/** 配音清单：shots.vo（"role: 台词"）+ 无 vo 的 ending 旁白——与影游工坊同扫法 */
export function storyVoItems(story: RawStory): VoRef[] {
  const out: VoRef[] = [];
  for (const n of story.nodes ?? []) {
    (n.shots ?? []).forEach((s, i) => {
      if (!s.vo) return;
      const m = String(s.vo).match(/^(\w+)\s*[:：]\s*(.+)$/);
      out.push({ id: `${n.id}_${i}`, node: n.id, role: m ? m[1] : "narrator", text: m ? m[2] : String(s.vo) });
    });
    if (n.type === "ending" && !(n.shots ?? []).some((s) => s.vo)) {
      out.push({ id: `${n.id}_0`, node: n.id, role: "narrator", text: String(n.text ?? "") });
    }
  }
  return out.filter((v) => v.text.trim());
}

export interface EdgeRef {
  from: string;
  to: string;
  /** 选项/路由的说明（画布 hover 提示） */
  label?: string;
}

/** 连边派生：goto + options + routes——剧情单一真源，画布不存边 */
export function storyEdges(story: RawStory): EdgeRef[] {
  const ids = new Set((story.nodes ?? []).map((n) => n.id));
  const out: EdgeRef[] = [];
  for (const n of story.nodes ?? []) {
    if (n.goto && ids.has(n.goto)) out.push({ from: n.id, to: n.goto });
    for (const o of n.options ?? []) if (ids.has(o.goto)) out.push({ from: n.id, to: o.goto, label: o.text });
    for (const r of n.routes ?? []) if (ids.has(r.goto)) out.push({ from: n.id, to: r.goto, label: r.when ?? "默认" });
  }
  return out;
}

export const KIND_LABEL: Record<string, string> = {
  scene: "🎬 场景",
  choice: "🔀 抉择",
  hub: "🧭 分流",
  ending: "🏁 结局",
};

/** 连播巡检顺序（M2）：按剧本文档顺序过每一拍，不做分支演绎 */
export function walkSequence(story: RawStory): { node: RawNode; shots: ShotRef[] }[] {
  return (story.nodes ?? [])
    .map((node) => ({ node, shots: storyShots(story).filter((s) => s.node === node.id) }))
    .filter((x) => x.shots.length > 0);
}
