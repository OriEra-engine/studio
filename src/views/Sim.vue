<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { NButton, NInputNumber, NTag, useDialog } from "naive-ui";
import { api, type SimExportReport } from "../api";
import { refreshStatus, store } from "../store";
// 官方预设库（M2-C 三库工作台）：engine-snapshot/specs/ 由
// scripts/sync-engine-snapshot.mjs 从引擎 crates/oriera-agent/specs/ 同步。
// resolved/ 是 extends 展开版——浏览器里的 wasm 没有文件系统、读不到基 spec，
// 必须整份发给内核；展开不改身份，spec_hash 以 INDEX.json 为准（同步器的
// 「内核版本守卫」逐个核过）。构建期打进单 bundle：Tauri 内置服务器只挂
// /examples，iframe 不需要 URL 访问 specs——换装走 postMessage 整份 JSON。
import tabletopSpec from "../../engine-snapshot/specs/resolved/tabletop-4dof-pick.json";
import pandaSpec from "../../engine-snapshot/specs/resolved/panda-7dof-pick.json";
import stackSpec from "../../engine-snapshot/specs/resolved/stack-4dof.json";
import binPickSpec from "../../engine-snapshot/specs/resolved/bin-pick-4dof.json";
import drawerV2Spec from "../../engine-snapshot/specs/resolved/drawer-4dof-v2.json";
import mobileArmSpec from "../../engine-snapshot/specs/resolved/mobile-arm-4dof.json";
import pandaContactSpec from "../../engine-snapshot/specs/resolved/panda-contact-criterion.json";
import specIndexJson from "../../engine-snapshot/specs/INDEX.json";

/* 具身仿真台 · 三库工作台：iframe 内嵌引擎 example/15-embodied-sim（oriera-wasm
   物理内核，与 CLI 同一份 Rust 代码），顶部预设墙点选 → postMessage
   {cmd:"spec", json} 整份换装（内核 M1 协议，无重载）；左侧控制经 postMessage
   驱动；导出走 tauri 命令落盘 LeRobotDataset v2.1（见 src-tauri/src/sim.rs）。 */

interface SimEpisode {
  task: string;
  rows: Array<{ state: number[]; action: number[] }>;
  /** 每集回执指纹（wasm 十进制 u64 串）：v1 = 状态口径；旧内核快照可能缺省 */
  fingerprint?: string;
  /** scheme-v2 完整身份指纹（状态 ⊕ 机器人身份 ⊕ spec_hash） */
  fingerprintV2?: string;
}

interface SimEpisodesMsg {
  source: string;
  type: "episodes";
  episodes: SimEpisode[];
  obsNames: string[];
  actionNames: string[];
  task: string;
  fps: number;
  /** 机器人名（内核按解析后的 spec 透传 robot.name）——落 info.json 的 robot_type */
  robot?: string;
  /** spec 身份哈希（16 hex）——落 oriera_receipt.json */
  specHash?: string;
}

// ── 预设库 ────────────────────────────────────────────────────────
interface Placement {
  range?: { x: number[]; y: number; z: number[] };
  fixed?: number[];
}
interface SpecJson {
  meta: { name: string; difficulty?: string };
  robot: { name: string; joints: Array<{ name: string }>; gripper?: unknown };
  scene: {
    fixtures: Array<{ name: string; center: number[]; half: number[] }>;
    props: Array<{ name: string; placement: Placement }>;
  };
  task: { id: string; params: { goal: Placement; min_separation?: number } };
}
type PresetKey =
  | "tabletop-4dof-pick"
  | "panda-7dof-pick"
  | "stack-4dof"
  | "bin-pick-4dof"
  | "drawer-4dof-v2"
  | "mobile-arm-4dof"
  | "panda-contact-criterion";
interface Preset {
  key: PresetKey;
  /** 卡片名（人话） */
  title: string;
  /** 一句话说明（G-10） */
  pitch: string;
  /** spec 没写 meta.difficulty 时的难度（金标 tabletop 文件逐字节锁定，不带该字段） */
  difficultyFallback: string;
  /** 目标区能否单独挪：叠放的目标是底座 B、抽屉的目标是拉出行程，不单独挪 */
  goalTweak: boolean;
  spec: SpecJson;
}
const DEFAULT_KEY: PresetKey = "tabletop-4dof-pick"; // 首屏默认 = 作业基线（拍板项 3）
const PRESETS: readonly Preset[] = [
  {
    key: "tabletop-4dof-pick",
    title: "桌面抓放 · 作业基线",
    pitch: "把方块抓起来放进绿色目标圈——课程作业用的就是这一份。",
    difficultyFallback: "入门",
    goalTweak: true,
    spec: tabletopSpec as unknown as SpecJson,
  },
  {
    key: "panda-7dof-pick",
    title: "Panda 七轴抓放",
    pitch: "换成真实机械臂 Franka Panda（7 个关节）做同一个抓放。",
    difficultyFallback: "进阶",
    goalTweak: true,
    spec: pandaSpec as unknown as SpecJson,
  },
  {
    key: "stack-4dof",
    title: "叠方块",
    pitch: "把方块 A 叠到底座 B 上，松手后稳住不动才算成功。",
    difficultyFallback: "进阶",
    goalTweak: false,
    spec: stackSpec as unknown as SpecJson,
  },
  {
    key: "bin-pick-4dof",
    title: "料箱拣选",
    pitch: "从四面有墙的料箱里把方块夹出来，放到箱外的目标区。",
    difficultyFallback: "挑战",
    goalTweak: true,
    spec: binPickSpec as unknown as SpecJson,
  },
  {
    key: "drawer-4dof-v2",
    title: "拉抽屉 · 真滑轨",
    pitch: "抓住抽屉面板，沿 prismatic 滑轨拉出 8 厘米再松手（v2 真滑轨几何）。",
    difficultyFallback: "进阶",
    goalTweak: false,
    spec: drawerV2Spec as unknown as SpecJson,
  },
  {
    key: "mobile-arm-4dof",
    title: "移动底盘抓放",
    pitch: "底盘先滑到方块跟前，再伸臂抓起、放到另一侧的目标区——移动后才能抓。",
    difficultyFallback: "挑战",
    goalTweak: true,
    spec: mobileArmSpec as unknown as SpecJson,
  },
  {
    key: "panda-contact-criterion",
    title: "接触判据 · Panda 变体",
    pitch: "和 Panda 七轴同一份抓放，但「夹住」按真实接触力判定（≥1N 且不打滑），消除预接触假标签。",
    difficultyFallback: "进阶",
    goalTweak: true,
    spec: pandaContactSpec as unknown as SpecJson,
  },
  // drawer-4dof（v1，spec_hash ef18ac07）是 drawer@v2 的对照档案：可动盒体近似
  // 「抽屉把手」，已不在卡片墙下发；快照 specs/{official,resolved}/drawer-4dof.json
  // 留作 CLI 对照（--spec specs/official/drawer-4dof.json），Studio 不再引用。
];
const INDEX = (specIndexJson as unknown as { specs: Record<string, { hash: string }> }).specs;
const presetOf = (key: PresetKey): Preset => PRESETS.find((p) => p.key === key) ?? PRESETS[0];

const TASK_WORDS: Record<string, string> = {
  pick_place: "抓放",
  stack: "叠放",
  bin_pick: "料箱拣选",
  drawer: "拉抽屉",
  reach: "够取",
  push: "推",
};
function difficultyOf(p: Preset): string {
  return p.spec.meta.difficulty ?? p.difficultyFallback;
}
function difficultyType(p: Preset): "success" | "warning" | "error" {
  const d = difficultyOf(p);
  return d === "挑战" ? "error" : d === "进阶" ? "warning" : "success";
}
function taskRobotLine(p: Preset): string {
  const [family, ver] = p.spec.task.id.split("@");
  const v = ver?.startsWith("v") ? `第 ${ver.slice(1)} 版判据` : "判据";
  return `任务 ${p.spec.task.id}（${TASK_WORDS[family] ?? family} · ${v}）· 机器人 ${p.spec.robot.name}（${p.spec.robot.joints.length} 个关节）`;
}
/** 内核里的演示脚本是 4 轴解析 IK 的通用抓放节拍：只认 4 关节 + 夹爪。 */
function canScript(p: Preset): boolean {
  return p.spec.robot.joints.length === 4 && !!p.spec.robot.gripper;
}
const cliCommand = (key: PresetKey) =>
  `cargo run -p oriera-agent --example embodied_sim_baseline --features rigid3d,spec,urdf -- --spec crates/oriera-agent/specs/official/${key}.json --protocol course@v1`;

// ── 参数微调（G-3 升级）：物体 / 目标位置 → 一份新 spec（新 spec_hash = 新实验）
interface Tweak {
  obj: [number, number]; // x, z（米）
  goal: [number, number] | null;
}
const r3 = (v: number) => Math.round(v * 1000) / 1000;
const mid = (r: number[]) => (r[0] + r[1]) / 2;
function placementXZ(p: Placement): [number, number] {
  if (p.fixed) return [p.fixed[0], p.fixed[2]];
  if (p.range) return [r3(mid(p.range.x)), r3(mid(p.range.z))];
  return [0.2, 0];
}
function placementY(p: Placement, fallback: number): number {
  return p.fixed ? p.fixed[1] : p.range ? p.range.y : fallback;
}
const f2 = (v: number) => v.toFixed(2).replace("-", "−");
function placementText(p: Placement): string {
  if (p.fixed) return `固定在 x ${f2(p.fixed[0])} · z ${f2(p.fixed[2])}`;
  if (p.range) return `x ${f2(p.range.x[0])}–${f2(p.range.x[1])} · z ${f2(p.range.z[0])}–${f2(p.range.z[1])}（每集随机）`;
  return "—";
}
/** 能放的范围：桌面（fixture "table"）内缩 3 cm；没桌子的场景（移动底盘
 *  只有地面）退回第一块 fixture（地面）内缩——出生/目标带（z ±0.5）必须在
 *  界限内，硬编码默认值罩不住它们。 */
function tableBounds(s: SpecJson): { x: [number, number]; z: [number, number] } {
  const t = s.scene.fixtures.find((f) => f.name === "table") ?? s.scene.fixtures[0];
  if (!t) return { x: [0, 0.6], z: [-0.3, 0.3] };
  const m = 0.03;
  return {
    x: [r3(t.center[0] - t.half[0] + m), r3(t.center[0] + t.half[0] - m)],
    z: [r3(t.center[2] - t.half[2] + m), r3(t.center[2] + t.half[2] - m)],
  };
}
/** 预设 + 微调 → 发给内核的整份 spec（深拷贝：预设常量不被改）。 */
function buildSpec(p: Preset, t: Tweak | null): SpecJson {
  const s = JSON.parse(JSON.stringify(p.spec)) as SpecJson;
  if (t) {
    const p0 = s.scene.props[0];
    p0.placement = { fixed: [t.obj[0], placementY(p0.placement, 0.55), t.obj[1]] };
    if (t.goal) s.task.params.goal = { fixed: [t.goal[0], placementY(s.task.params.goal, 0.5), t.goal[1]] };
  }
  return s;
}

// ── iframe 与内核状态 ──────────────────────────────────────────────
const frameNonce = ref(0); // >0 ⇒ 带 ?r= 强制重载（丢弃内核里的录制与状态）
const url = computed(() => {
  const port = store.status?.server?.port;
  if (!port) return "";
  const base = `http://127.0.0.1:${port}/examples/15-embodied-sim/`;
  return frameNonce.value ? `${base}?r=${frameNonce.value}` : base;
});

const frame = ref<HTMLIFrameElement>();
const loading = ref(true);
const recording = ref(false);
const episodeCount = ref(0);
const frameCount = ref(0);
const note = ref("");
const exporting = ref(false);
const report = ref<SimExportReport | null>(null);
const errMsg = ref("");
const dialog = useDialog();

/** 用户选中的预设与已应用的微调（期望态）。 */
const activeKey = ref<PresetKey>(DEFAULT_KEY);
const applied = ref<Tweak | null>(null);
/** 内核此刻真正在跑的（以内核回报为准）。内核启动自带的 preset_spec_json
 *  就是作业基线（spec_hash 与 INDEX 的 tabletop-4dof-pick 一致）。 */
const kernel = reactive({
  ready: false,
  key: DEFAULT_KEY as PresetKey,
  tweak: null as Tweak | null,
  hash: INDEX[DEFAULT_KEY]?.hash ?? "",
});
const pending = ref<{ key: PresetKey; tweak: Tweak | null } | null>(null);
/** 换装没被内核确认的预设（版本守卫：内核快照比预设库旧时会出现）。 */
const staleKeys = reactive(new Set<PresetKey>());
const switchFail = ref<{ key: PresetKey; tweaked: boolean } | null>(null);
const kernelSilent = ref(false);
let pendingTimer: ReturnType<typeof setTimeout> | undefined;
let readyTimer: ReturnType<typeof setTimeout> | undefined;
const SPEC_ACK_MS = 4000;
const SEED0 = 1000; // 换装后首集 = 协议 seed0（与 CLI 第一集同种子）

const activePreset = computed(() => presetOf(activeKey.value));
const kernelPreset = computed(() => presetOf(kernel.key));
/** props[0] 之外的场景件名（叠放底座 / 料箱墙）——内核渲染器暂时只摆 props[0]
 *  （TableTopEnv 只公开 props[0] 位姿，M1 已知缺口）；内核补齐后删掉这条提示。 */
const extraProps = computed(() => kernelPreset.value.spec.scene.props.slice(1).map((p) => p.name));
const kernelHashExpected = computed(() => (kernel.tweak ? null : INDEX[kernel.key]?.hash ?? null));
const hasPendingData = computed(() => recording.value || episodeCount.value > 0);

function send(cmd: Record<string, unknown>) {
  frame.value?.contentWindow?.postMessage({ source: "oriera-sim", ...cmd }, "*");
}

/** 把（预设, 微调）发给内核换装；内核未就绪时只记期望态，等 ready 再发。 */
function pushSpec(key: PresetKey, tweak: Tweak | null) {
  activeKey.value = key;
  applied.value = tweak;
  if (!kernel.ready) return;
  if (kernel.key === key && JSON.stringify(kernel.tweak) === JSON.stringify(tweak)) return;
  pending.value = { key, tweak };
  switchFail.value = null;
  errMsg.value = "";
  send({ cmd: "spec", json: JSON.stringify(buildSpec(presetOf(key), tweak)), seed: SEED0 });
  clearTimeout(pendingTimer);
  pendingTimer = setTimeout(onSpecTimeout, SPEC_ACK_MS);
}

function onSpecTimeout() {
  const p = pending.value;
  if (!p) return;
  pending.value = null;
  // 内核没确认 = 它还在跑旧场景：选中态回到画面里真实的那一份（不撒谎）。
  if (!p.tweak) staleKeys.add(p.key);
  switchFail.value = { key: p.key, tweaked: !!p.tweak };
  activeKey.value = kernel.key;
  applied.value = kernel.tweak;
  resetDraft();
}

function onSpecApplied(specHash: string) {
  const p = pending.value;
  if (!p) return;
  clearTimeout(pendingTimer);
  pending.value = null;
  kernel.key = p.key;
  kernel.tweak = p.tweak;
  kernel.hash = specHash;
  staleKeys.delete(p.key);
  const title = presetOf(p.key).title;
  note.value = p.tweak
    ? `已换装：${title}（改过参数 = 新实验，spec_hash ${specHash.slice(0, 8)}）`
    : `已换装：${title}（spec_hash ${specHash.slice(0, 8)}）`;
}

/** 有录制中/未导出的数据时先问：不同预设的数据不能混进同一个数据集。 */
function guardThen(action: () => void, discard: () => void) {
  if (!hasPendingData.value) return action();
  dialog.warning({
    title: "还有没导出的录制",
    content: `${recording.value ? "正在录制，" : ""}画面里还有 ${episodeCount.value} 集没导出。换预设后新旧场景的数据会混在一起（维数、任务都可能不同），所以要么先导出，要么丢弃后再换。`,
    positiveText: "丢弃并切换",
    negativeText: "先去导出",
    onPositiveClick: discard,
  });
}

function choosePreset(key: PresetKey) {
  if (pending.value || (key === activeKey.value && !applied.value)) return;
  guardThen(
    () => {
      pushSpec(key, null);
      resetDraft();
    },
    () => {
      activeKey.value = key;
      applied.value = null;
      resetDraft();
      reloadFrame(); // 重载清掉内核里的录制；ready 后按期望态换装
    },
  );
}

/** 整页重载 iframe（丢弃内核状态）；ready 后自动换回期望的预设。 */
function reloadFrame() {
  clearTimeout(pendingTimer);
  pending.value = null;
  kernel.ready = false;
  kernelSilent.value = false;
  loading.value = true;
  recording.value = false;
  episodeCount.value = 0;
  frameCount.value = 0;
  frameNonce.value += 1;
}

// 微调输入框（草稿；点「应用到画面」才生效）
const draft = reactive({ objX: 0, objZ: 0, goalX: 0, goalZ: 0 });
function resetDraft() {
  const p = activePreset.value;
  const t = applied.value;
  const [ox, oz] = t ? t.obj : placementXZ(p.spec.scene.props[0].placement);
  let [gx, gz] = t?.goal ?? placementXZ(p.spec.task.params.goal);
  // 两个出生区的中点可能比 min_separation 还近（桌面抓放只差 9.5 cm）：默认值
  // 把目标推到它出生区的远端，保证一打开就是一组能直接「应用」的数。
  const g = p.spec.task.params.goal.range;
  const sep = p.spec.task.params.min_separation ?? 0.12;
  if (!t && p.goalTweak && g) {
    if (Math.hypot(ox - gx, oz - gz) < sep) gx = g.x[1];
    if (Math.hypot(ox - gx, oz - gz) < sep) gz = g.z[1];
  }
  Object.assign(draft, { objX: ox, objZ: oz, goalX: gx, goalZ: gz });
}
resetDraft();
const bounds = computed(() => tableBounds(activePreset.value.spec));
const minSep = computed(() => activePreset.value.spec.task.params.min_separation ?? 0.12);
/** 输入框被清空时值是 null：不完整的草稿不许应用。 */
const draftValid = computed(() => {
  const vals = activePreset.value.goalTweak
    ? [draft.objX, draft.objZ, draft.goalX, draft.goalZ]
    : [draft.objX, draft.objZ];
  return vals.every((v) => typeof v === "number" && Number.isFinite(v));
});
const tooClose = computed(
  () =>
    draftValid.value &&
    activePreset.value.goalTweak &&
    Math.hypot(draft.objX - draft.goalX, draft.objZ - draft.goalZ) < minSep.value,
);
const draftTweak = computed<Tweak>(() => ({
  obj: [r3(draft.objX), r3(draft.objZ)],
  goal: activePreset.value.goalTweak ? [r3(draft.goalX), r3(draft.goalZ)] : null,
}));
function applyTweak() {
  if (!draftValid.value || tooClose.value || pending.value) return;
  const t = draftTweak.value;
  guardThen(
    () => pushSpec(activeKey.value, t),
    () => {
      applied.value = t;
      reloadFrame();
    },
  );
}
function restorePreset() {
  if (pending.value) return;
  guardThen(
    () => {
      pushSpec(activeKey.value, null);
      resetDraft();
    },
    () => {
      applied.value = null;
      resetDraft();
      reloadFrame();
    },
  );
}

async function copyText(text: string, okNote: string) {
  try {
    await navigator.clipboard.writeText(text);
    note.value = okNote;
  } catch {
    errMsg.value = "剪贴板不可用，请手动复制：" + text;
  }
}
function copySpecJson() {
  const spec = buildSpec(kernelPreset.value, kernel.tweak);
  void copyText(
    JSON.stringify(spec, null, 2) + "\n",
    "这份 spec 已复制：存成 my-spec.json，CLI 用 --spec my-spec.json 跑出同一个 spec_hash",
  );
}
function copyCli() {
  void copyText(cliCommand(kernel.key), "CLI 命令已复制（在 engine 仓根目录运行）");
}

// ── 既有控制（录制/导出护栏不动）──────────────────────────────────
function doReset() { send({ cmd: "reset" }); }
function doPolicy() {
  if (!canScript(kernelPreset.value)) return;
  send({ cmd: "policy" });
  note.value = kernelPreset.value.spec.task.id.startsWith("pick_place")
    ? "脚本策略运行中（IK 抓放）"
    : "脚本策略运行中（通用 IK 抓放节拍；这个任务的专属基线在 CLI 里）";
}
function toggleRecord() {
  recording.value = !recording.value;
  send({ cmd: "record" });
  if (recording.value) note.value = "录制中：遥操作或跑策略，每 tick 一行 (state, action)";
}

async function copyTrainCmd() {
  const dir = report.value?.dir ?? "<导出目录>";
  const trainCmd = `python engine/scripts/jsonl_to_lerobot_v21.py "${dir}"
lerobot-train --dataset.repo_id=<user>/<ds> --policy.type=act`;
  try {
    await navigator.clipboard.writeText(trainCmd);
    note.value = "训练命令已复制到剪贴板";
  } catch {
    errMsg.value = "剪贴板不可用，请手动复制：" + trainCmd;
  }
}

// 导出收尾以「收到内核 episodes + Tauri 落盘完成」为准；内核侧无数据/
// 页面冻结时收不到消息，5 秒看门狗兜底（诚实 spinner：不再 setTimeout
// 假完成——SRE-4/5 实测发现）。
let exportWatchdog: ReturnType<typeof setTimeout> | undefined;
const EXPORT_ACK_MS = 5000;

async function doExport() {
  if (exporting.value) return;
  exporting.value = true;
  errMsg.value = "";
  // 看门狗先挂再发消息：postMessage 万一同步抛错，spinner 也有兜底自愈。
  clearTimeout(exportWatchdog);
  exportWatchdog = setTimeout(() => {
    if (!exporting.value) return;
    exporting.value = false;
    errMsg.value = `未收到画面内核的数据（${EXPORT_ACK_MS / 1000} 秒无回应）——请重试；反复失败就「重载画面」后再录一次。`;
  }, EXPORT_ACK_MS);
  send({ cmd: "export" }); // 触发 iframe 把 episodes 发回来（message 事件里落盘）
}

async function onMessage(ev: MessageEvent) {
  // 信任边界：仅接受本 iframe 的消息（source + origin 双校验——
  // 安全深检：应用内任意 iframe 可伪造 source 驱动落盘）。
  if (
    !frame.value ||
    ev.source !== frame.value.contentWindow ||
    ev.origin !== `http://127.0.0.1:${store.status?.server?.port}`
  ) {
    return;
  }
  const m = ev.data as Partial<SimEpisodesMsg> & { source?: string; type?: string; note?: string; specHash?: string };
  if (!m || m.source !== "oriera-sim") return;
  if (m.type === "progress") {
    // 所见即所录：录制行数/集数实时跳动（内核每 30 行回报累计值；停止
    // 录制时也回报一次总数）——「录制中」措辞只在真在录时说。
    frameCount.value = Number((m as { rows?: number }).rows ?? 0);
    episodeCount.value = Number((m as { episodes?: number }).episodes ?? 0);
    if (recording.value && frameCount.value > 0) {
      note.value = `录制中：第 ${episodeCount.value} 集，已 ${frameCount.value} 行`;
    }
  } else if (m.type === "episodes") {
    clearTimeout(exportWatchdog);
    const msg = m as unknown as SimEpisodesMsg;
    try {
      report.value = await api.simExportLerobot(JSON.stringify(msg));
      recording.value = false;
      // 内核交出 episodes 后已清空自己的列表：计数归零（换预设不再拦）
      episodeCount.value = 0;
      frameCount.value = 0;
      note.value = `已导出 ${report.value.episodes} 集 / ${report.value.frames} 行 → ${report.value.dir}`;
    } catch (e) {
      errMsg.value = String(e);
    } finally {
      exporting.value = false; // 落盘完成（含失败）才收 spinner
    }
  } else if (m.type === "state" && m.note === "ready") {
    // 内核启动完成（自带作业基线）；期望态不同就立刻换装。
    clearTimeout(readyTimer);
    kernelSilent.value = false;
    kernel.ready = true;
    kernel.key = DEFAULT_KEY;
    kernel.tweak = null;
    kernel.hash = INDEX[DEFAULT_KEY]?.hash ?? "";
    if (activeKey.value !== DEFAULT_KEY || applied.value) pushSpec(activeKey.value, applied.value);
  } else if (m.type === "state" && m.note === "spec-applied") {
    onSpecApplied(String(m.specHash ?? ""));
  } else if (m.type === "state" && m.note === "success") {
    note.value = "🎉 成功送达目标区！";
  }
}

onMounted(async () => {
  // 先挂监听再等状态：内核的 ready 不能落在监听之前。
  window.addEventListener("message", onMessage);
  await refreshStatus();
});
onBeforeUnmount(() => {
  window.removeEventListener("message", onMessage);
  clearTimeout(pendingTimer);
  clearTimeout(readyTimer);
  clearTimeout(exportWatchdog);
});

function onFrameLoad() {
  loading.value = false;
  // 页面到了但内核迟迟不报到（wasm 加载失败等）：15 秒后给个重载入口。
  clearTimeout(readyTimer);
  if (!kernel.ready) readyTimer = setTimeout(() => { kernelSilent.value = !kernel.ready; }, 15000);
}
</script>

<template>
  <div class="sim-root">
    <header class="sim-toolbar">
      <div class="sim-title">
        具身仿真台 <NTag size="small" type="info" :bordered="false">真实物理 · 一键导出可训练数据</NTag>
      </div>
      <div class="sim-actions">
        <NButton size="small" @click="doReset">↺ 重置</NButton>
        <NButton
          size="small"
          type="primary"
          :disabled="!canScript(kernelPreset)"
          :title="canScript(kernelPreset) ? '' : '演示脚本是 4 轴解析 IK，7 轴 Panda 用不了——它的基线是 DLS 数值 IK，在 CLI 里跑（见右侧「在 CLI 跑同一份预设」）'"
          @click="doPolicy"
        >▶ 脚本策略</NButton>
        <NButton size="small" :type="recording ? 'error' : 'default'" @click="toggleRecord">
          {{ recording ? "■ 停止录制" : "● 开始录制" }}
        </NButton>
        <NButton size="small" :loading="exporting" :disabled="frameCount === 0 && episodeCount === 0" @click="doExport">
          ⤓ 导出数据集{{ frameCount > 0 ? `（${frameCount} 行）` : "" }}
        </NButton>
        <NButton v-if="report" size="small" quaternary @click="api.simRevealExport()">打开导出目录</NButton>
        <NButton v-if="report" size="small" quaternary @click="copyTrainCmd">复制训练命令</NButton>
      </div>
    </header>

    <section class="preset-wall" aria-label="官方预设库">
      <button
        v-for="p in PRESETS"
        :key="p.key"
        type="button"
        class="preset-card"
        :class="{ active: p.key === activeKey, pending: pending?.key === p.key, stale: staleKeys.has(p.key) }"
        :disabled="!!pending"
        :aria-pressed="p.key === activeKey"
        @click="choosePreset(p.key)"
      >
        <div class="pc-head">
          <span class="pc-title">{{ p.title }}</span>
          <NTag size="small" :type="difficultyType(p)" :bordered="false">{{ difficultyOf(p) }}</NTag>
        </div>
        <div class="pc-pitch">{{ p.pitch }}</div>
        <div class="pc-meta">{{ taskRobotLine(p) }}</div>
        <div v-if="pending?.key === p.key" class="pc-flag">换装中…</div>
        <div v-else-if="staleKeys.has(p.key)" class="pc-flag warn">画面内核待重建</div>
        <div v-else-if="p.key === DEFAULT_KEY" class="pc-flag base">作业基线（默认）</div>
      </button>
    </section>

    <div class="sim-main">
      <div class="sim-frame-wrap">
        <div v-if="!url" class="sim-hint">内置服务器未就绪——回到首页稍候再进。</div>
        <iframe
          v-else
          ref="frame"
          :src="url"
          class="sim-frame"
          @load="onFrameLoad"
        ></iframe>
        <div v-if="url && loading" class="sim-hint overlay">物理内核加载中（首次约 2 秒）…</div>
      </div>
      <aside class="sim-side">
        <h3>当前预设</h3>
        <p class="muted">
          <b class="hl">{{ kernelPreset.title }}</b>——{{ kernelPreset.pitch }}
          <template v-if="canScript(kernelPreset)">键鼠遥操作：Q/A 底座、W/S 肩、E/D 肘、R/F 腕、空格夹爪。</template>
          <template v-else>键盘只管前 4 个关节（Q/A W/S E/D R/F），其余关节用画面右侧滑条；空格夹爪。「▶ 脚本策略」在这里是灰的：演示脚本是 4 轴解析 IK，Panda 的基线是 DLS 数值 IK，在 CLI 里跑（见下方命令）。</template>
        </p>
        <p class="muted small">
          spec_hash（实验身份证号：号相同 = 同一个实验）<code>{{ kernel.hash.slice(0, 8) || "—" }}</code>
          <span v-if="kernel.tweak" class="warn-text">· 改过参数 = 新实验，作业基线请用未改动的预设</span>
          <span v-else-if="kernelHashExpected && kernel.hash === kernelHashExpected" class="ok-text">· 与 CLI 同一个实验 ✓</span>
          <span v-else-if="kernelHashExpected" class="warn-text">· 与预设登记表（INDEX.json）对不上：内核快照与引擎不同步，请重跑同步器</span>
        </p>
        <p v-if="extraProps.length" class="warn-text small">
          注意：画面目前只画出任务物。另外 {{ extraProps.length }} 个场景件（{{ extraProps.join("、") }}）在物理和判据里真实存在，
          但渲染器还读不到它们的位置，暂时画不出来（已知缺口，待内核补齐场景件位姿）；成功率以 CLI 为准。
        </p>
        <div v-if="switchFail" class="alert">
          <b>画面内核待重建</b>：内核没有确认换装「{{ presetOf(switchFail.key).title }}」（{{ SPEC_ACK_MS / 1000 }} 秒无回应），画面还是「{{ kernelPreset.title }}」。
          <template v-if="switchFail.tweaked">可能是改的数没过 spec 校验，</template>多半是这台电脑上的内核快照比预设库旧——维护者在 Studio 仓跑
          <code>node scripts/sync-engine-snapshot.mjs</code>（输出里「内核版本守卫」会列出拒载的预设）；具体原因写在画面左下「采集日志」。
          在那之前可以直接用 CLI 跑这份预设：<code class="cmd">{{ cliCommand(switchFail.key) }}</code>
          <div class="row-btns"><NButton size="tiny" @click="reloadFrame">重载画面</NButton></div>
        </div>
        <div v-if="kernelSilent" class="alert">
          内核 15 秒没有报到（wasm 没加载成功或还在加载）。<NButton size="tiny" @click="reloadFrame">重载画面</NButton>
        </div>

        <h3>参数微调 <span class="muted small">（改了 = 新实验）</span></h3>
        <p class="muted small">
          物体出生位：{{ placementText(activePreset.spec.scene.props[0].placement) }}；目标：{{ placementText(activePreset.spec.task.params.goal) }}。
          填一个位置（米，x = 离底座远近，z = 左右），点「应用到画面」就生成一份新 spec 换上（每集都放在这个点）。
        </p>
        <div class="tweak-row">
          <span class="tw-label">物体位置</span>
          x <NInputNumber v-model:value="draft.objX" size="tiny" :step="0.01" :precision="3" :min="bounds.x[0]" :max="bounds.x[1]" class="tw-num" />
          z <NInputNumber v-model:value="draft.objZ" size="tiny" :step="0.01" :precision="3" :min="bounds.z[0]" :max="bounds.z[1]" class="tw-num" />
        </div>
        <div v-if="activePreset.goalTweak" class="tweak-row">
          <span class="tw-label">目标位置</span>
          x <NInputNumber v-model:value="draft.goalX" size="tiny" :step="0.01" :precision="3" :min="bounds.x[0]" :max="bounds.x[1]" class="tw-num" />
          z <NInputNumber v-model:value="draft.goalZ" size="tiny" :step="0.01" :precision="3" :min="bounds.z[0]" :max="bounds.z[1]" class="tw-num" />
        </div>
        <p v-else class="muted small">这个任务的目标由场景决定（叠放 = 底座 B 的顶面；抽屉 = 拉出行程），不单独挪。</p>
        <p v-if="tooClose" class="warn-text small">物体和目标离得太近（小于 {{ Math.round(minSep * 100) }} 厘米），引擎会判「一出生就在目标区」——拉开一点再应用。</p>
        <p v-else-if="!draftValid" class="warn-text small">有空着的数字框，填满再应用。</p>
        <div class="row-btns">
          <NButton size="tiny" type="primary" :disabled="!draftValid || tooClose || !!pending" @click="applyTweak">应用到画面</NButton>
          <NButton size="tiny" :disabled="!applied || !!pending" @click="restorePreset">恢复预设</NButton>
          <NButton size="tiny" quaternary @click="copySpecJson">复制这份 spec JSON</NButton>
        </div>

        <h3>采集</h3>
        <p class="muted">
          录制以 60Hz 逐拍记一行（观测, 动作）——维数跟着机器人走（4 轴臂 = 21 维观测 + 5 维动作）；导出为
          <b>LeRobotDataset v2.1</b> 目录（meta 全套 + 每集回执），
          用 engine 仓脚本转 parquet 后 <code>lerobot-train</code> 直读。换预设前先导出：不同预设的数据不能混在一个数据集里。
        </p>
        <!-- 上限数字与内核 kernel.js 的 REC_CAP（200 集 / 20000 行）一致——内核侧改动时同步这里 -->
        <p v-if="episodeCount > 0 || frameCount > 0" class="muted">
          已录 <b class="hl">{{ episodeCount }}</b> 集 · <b class="hl">{{ frameCount }}</b> 行{{ recording ? "（录制中）" : "" }}——软上限 200 集 / 20000 行（先到为准），到限画面内核会自动停止录制并提示，已录数据照常可导出。
        </p>
        <h3>状态</h3>
        <p class="muted">{{ note || "进场即预载作业基线；点上方卡片换预设，重置换随机种子，脚本策略可全自动演示。" }}</p>
        <p v-if="errMsg" class="err">{{ errMsg }}</p>
        <p v-if="report" class="ok">
          最近导出：{{ report.dir }}
        </p>
        <h3>训练三步</h3>
        <ol class="muted small">
          <li>本页遥操作/跑策略 → 录制 → 导出</li>
          <li><code>python engine/scripts/jsonl_to_lerobot_v21.py &lt;导出目录&gt;</code></li>
          <li><code>lerobot-train --dataset.repo_id=&lt;user&gt;/&lt;ds&gt; --policy.type=act</code></li>
        </ol>
        <h3>在 CLI 跑同一份预设</h3>
        <p class="muted small">
          成功率 + Wilson 区间 + 任务专属闸（作业证据以 CLI 的 eval_report 为准）。在 engine 仓根目录：
          <code class="cmd">{{ cliCommand(kernel.key) }}</code>
          <template v-if="kernel.tweak">画面里这份改过参数：先「复制这份 spec JSON」存成 my-spec.json，再把命令里的 <code>--spec</code> 换成它——CLI 会算出同一个 spec_hash。</template>
        </p>
        <div class="row-btns"><NButton size="tiny" quaternary @click="copyCli">复制命令</NButton></div>
        <h3>确定性说明</h3>
        <p class="muted small">
          画面里跑的是与 CLI 同一份 Rust 物理（编译成 wasm），吃的是同一份预设文件、同一个 spec_hash；
          浏览器与 CLI 统计口径一致，逐位复现与批量采集以 CLI 为准：
          <code>cargo run -p oriera-agent --example embodied_sim_collect --features rigid3d</code>
          （双跑逐位一致 + 每集 sim_fingerprint 回执；加 <code>--export npz</code> 另出 numpy 直读的裸数组）。
        </p>
      </aside>
    </div>
  </div>
</template>

<style scoped>
.sim-root { display: flex; flex-direction: column; height: 100vh; }
.sim-toolbar {
  display: flex; justify-content: space-between; align-items: center;
  padding: 10px 16px; border-bottom: 1px solid var(--n-border-color, #222633); gap: 12px;
}
.sim-title { font-weight: 600; display: flex; align-items: center; gap: 8px; }
.sim-actions { display: flex; gap: 8px; flex-wrap: wrap; }
.preset-wall {
  display: grid; grid-template-columns: repeat(auto-fill, minmax(190px, 1fr)); gap: 8px;
  padding: 10px 16px; border-bottom: 1px solid #222633;
}
.preset-card {
  text-align: left; background: #11151f; color: inherit; font: inherit;
  border: 1px solid #222633; border-radius: 8px; padding: 8px 10px; cursor: pointer;
  display: flex; flex-direction: column; gap: 4px; min-width: 0;
  transition: border-color 0.15s, box-shadow 0.15s;
}
.preset-card:hover { border-color: #3a4870; }
.preset-card.active { border-color: #5b8def; box-shadow: 0 0 0 1px #5b8def inset; background: #141b2c; }
.preset-card.stale { border-style: dashed; border-color: #a06a30; }
.preset-card:disabled { cursor: progress; opacity: 0.75; }
.pc-head { display: flex; justify-content: space-between; align-items: center; gap: 6px; }
.pc-title { font-weight: 600; font-size: 13px; color: #e6e8ee; }
.pc-pitch { font-size: 12px; color: #b8bfd3; line-height: 1.5; }
.pc-meta { font-size: 10.5px; color: #6f7690; line-height: 1.4; word-break: break-all; }
.pc-flag { font-size: 10.5px; color: #9fb6e8; }
.pc-flag.base { color: #7be0a0; }
.pc-flag.warn { color: #ffb86b; }
.sim-main { flex: 1; display: flex; min-height: 0; }
.sim-frame-wrap { flex: 1; position: relative; min-width: 0; }
.sim-frame { width: 100%; height: 100%; border: 0; background: #0b0d12; }
.sim-hint { padding: 24px; color: #8a90a6; font-size: 13px; }
.sim-hint.overlay {
  position: absolute; inset: 0; display: flex; align-items: center; justify-content: center;
  background: rgba(11, 13, 18, 0.75);
}
.sim-side {
  width: 300px; border-left: 1px solid #222633; padding: 14px;
  overflow: auto; font-size: 12px; line-height: 1.7;
}
.sim-side h3 { font-size: 12px; margin: 14px 0 6px; color: #9fb6e8; }
.sim-side h3:first-child { margin-top: 0; }
.muted { color: #8a90a6; }
.small { font-size: 11px; }
.hl { color: #e6e8ee; }
.cube { color: #ffd479; }
.goal { color: #7be0a0; }
.err { color: #ff8098; }
.ok { color: #7be0a0; word-break: break-all; }
.ok-text { color: #7be0a0; }
.warn-text { color: #ffb86b; }
.alert {
  border: 1px solid #5a4020; background: #1d1710; color: #d9c3a5; border-radius: 6px;
  padding: 8px 10px; margin: 8px 0; font-size: 11px; line-height: 1.6;
}
.tweak-row { display: flex; align-items: center; gap: 4px; margin: 4px 0; color: #8a90a6; }
.tw-label { width: 56px; color: #b8bfd3; }
.tw-num { width: 86px; }
.row-btns { display: flex; gap: 6px; flex-wrap: wrap; margin: 6px 0; }
code { background: #1a2030; border-radius: 4px; padding: 1px 5px; }
code.cmd { display: block; margin-top: 4px; white-space: pre-wrap; word-break: break-all; font-size: 10.5px; }
</style>
