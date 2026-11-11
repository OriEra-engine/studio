<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { api } from "../api";
import type { BenchmarkCase, BenchmarkOut, EvalEvent, ScoreRecord, SnapshotMeta } from "../api";
import type { DiffLine } from "../api";

/**
 * 进化实验室（v0.14.0，学 PenguinHarness 的 Default RSI Toolkit）：
 * 冻结 Benchmark 打分 → 优化器提可证伪假设改 prompt（一处有边界的修改）→
 * 重新评估 → 分数严格提升才保留（候选不落盘，落选即天然回滚）。
 * 信息隔离：评估员见 rubric；被测只做题干；优化器只见分数+理由。
 */

const loading = ref(true);
const bench = ref<BenchmarkOut | null>(null);
const board = ref<ScoreRecord[]>([]);
const running = ref(false);
const rounds = ref(1);
const runningKind = ref<"eval" | "optimize" | "">("");
const events = ref<LabEvent[]>([]);
const doneFlash = ref("");
const openDiffCase = ref(-1);
const showRubric = ref(-1);

type LabEvent =
  | { t: "round_start"; kind: string; total: number }
  | { t: "case"; index: number; total: number; case_id: string; score: number; duration_ms: number; note: string }
  | { t: "proposal"; hypothesis: string; change_summary: string; diff: DiffLine[] }
  | { t: "round_done"; kind: string; avg_score: number; prev_avg: number | null; adopted: boolean | null; state_version: number }
  | { t: "error"; message: string };

const stateVersion = ref(1);
const snapshots = ref<SnapshotMeta[]>([]);

const baseline = computed(() => board.value[0]);
const last = computed(() => board.value[board.value.length - 1]);

function fmtTime(ts: number) {
  if (!ts) return "";
  const d = new Date(ts * 1000);
  return `${d.getMonth() + 1}/${d.getDate()} ${d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })}`;
}
function kindLabel(kind: string, adopted: boolean | null) {
  if (kind === "baseline") return "基线";
  if (kind === "candidate") return adopted ? "候选·已采纳" : "候选·已丢弃";
  return "评估";
}
function caseTitle(id: string) {
  const c = bench.value?.cases.find(x => x.id === id);
  return c ? `${c.statement.beat.title}（${id}）` : id;
}

async function load() {
  loading.value = true;
  try {
    const [b, sb, st] = await Promise.all([
      api.benchmarkGet(),
      api.labScoreboardGet(),
      api.agentStateGet().catch(() => null),
    ]);
    bench.value = b;
    board.value = sb;
    if (st) {
      stateVersion.value = st.config.version;
      snapshots.value = st.snapshots;
    }
  } catch (e) {
    events.value.push({ t: "error", message: String(e).slice(0, 160) });
  }
  loading.value = false;
}

function handle(ev: EvalEvent) {
  switch (ev.event) {
    case "round_start":
      events.value.push({ t: "round_start", kind: ev.kind, total: ev.total_cases });
      break;
    case "case_done":
      events.value.push({ t: "case", index: ev.index, total: ev.total, case_id: ev.case_id, score: ev.score, duration_ms: ev.duration_ms, note: ev.note });
      break;
    case "proposal":
      events.value.push({ t: "proposal", hypothesis: ev.hypothesis, change_summary: ev.change_summary, diff: ev.diff ?? [] });
      break;
    case "round_done":
      events.value.push({ t: "round_done", kind: ev.kind, avg_score: ev.avg_score, prev_avg: ev.prev_avg, adopted: ev.adopted, state_version: ev.state_version });
      break;
    case "done":
      doneFlash.value = ev.message;
      break;
    case "error":
      events.value.push({ t: "error", message: ev.message });
      break;
  }
}

/** 跑完一轮（invoke 在拿 task id 时即返回，必须等 done/error 事件才算结束） */
function runLab(fn: (onEvent: (e: EvalEvent) => void) => Promise<string>): Promise<void> {
  return new Promise(resolve => {
    const wrapped = (ev: EvalEvent) => {
      handle(ev);
      if (ev.event === "done" || ev.event === "error") resolve();
    };
    fn(wrapped).catch(e => {
      events.value.push({ t: "error", message: String(e).slice(0, 160) });
      resolve();
    });
  });
}

async function runEval() {
  if (running.value) return;
  running.value = true;
  runningKind.value = "eval";
  events.value = [];
  doneFlash.value = "";
  await runLab(api.labEvalRun);
  running.value = false;
  runningKind.value = "";
  await load();
}

async function runOptimize() {
  if (running.value) return;
  running.value = true;
  runningKind.value = "optimize";
  events.value = [];
  doneFlash.value = "";
  await runLab((onEvent) => api.labOptimizeRun(rounds.value, onEvent));
  running.value = false;
  runningKind.value = "";
  await load();
}

onMounted(load);
</script>

<template>
  <div class="lb-root">
    <header class="lb-head">
      <div class="lb-title">
        <h2>进化实验室</h2>
        <span v-if="bench" class="lb-chip">{{ bench.manifest?.title }} · {{ bench.manifest?.version }} · {{ bench.cases.length }} 题</span>
        <span class="lb-chip mono">当前奥粒 v{{ stateVersion }}</span>
      </div>
      <span style="flex: 1"></span>
      <span class="lb-cost">每轮评估 ≈ 12 次 LLM 调用（6 题 × 改写+评分）</span>
      <button class="lb-btn" :disabled="running" @click="runEval">
        {{ running && runningKind === "eval" ? "评估中…" : "▶ 跑一轮评估" }}
      </button>
      <span v-if="!running" class="lb-rounds">
        <button v-for="n in [1, 2, 3]" :key="n" class="lb-round" :class="{ on: rounds === n }" @click="rounds = n">{{ n }} 轮</button>
      </span>
      <button class="lb-btn primary" :disabled="running || !board.length" :title="board.length ? '' : '先跑一轮评估建立基线'" @click="runOptimize">
        {{ running && runningKind === "optimize" ? "优化中…" : `⚡ 优化 ${rounds} 轮` }}
      </button>
    </header>

    <div v-if="doneFlash" class="lb-flash">{{ doneFlash }}</div>
    <div v-if="loading" class="lb-loading">读取中…</div>

    <template v-else>
      <!-- 轮次进行中：事件流（Codex 工具行样式） -->
      <div v-if="events.length" class="lb-live">
        <div v-for="(ev, i) in events" :key="i" class="lb-ev">
          <template v-if="ev.t === 'round_start'">
            <div class="lb-ev-row"><span class="lb-spin"></span><span class="lb-ev-verb">开始{{ ev.kind === "eval" ? "评估" : "优化" }} · {{ ev.total }} 题</span></div>
          </template>
          <template v-else-if="ev.t === 'case'">
            <div class="lb-ev-row" :class="{ done: true }">
              <span class="lb-ev-ico ok">✓</span>
              <span class="lb-ev-verb">{{ caseTitle(ev.case_id) }}</span>
              <span class="lb-ev-score">{{ ev.score.toFixed(0) }} 分</span>
              <span class="lb-ev-dur">{{ (ev.duration_ms / 1000).toFixed(1) }}s</span>
            </div>
            <div v-if="ev.note" class="lb-ev-note">{{ ev.note }}</div>
          </template>
          <template v-else-if="ev.t === 'proposal'">
            <div class="lb-proposal">
              <div class="lb-ap-title">💡 假设：{{ ev.hypothesis }}</div>
              <div class="lb-ap-sub">{{ ev.change_summary }}</div>
              <div v-if="ev.diff.length" class="lb-diff">
                <div v-for="(l, li) in ev.diff" :key="li" class="lb-diff-line" :class="l.kind">
                  <span class="lb-diff-sign">{{ l.kind === "add" ? "+" : l.kind === "del" ? "−" : " " }}</span>
                  <span>{{ l.text }}</span>
                </div>
              </div>
            </div>
          </template>
          <template v-else-if="ev.t === 'round_done'">
            <div class="lb-ev-row">
              <span class="lb-ev-ico" :class="ev.adopted === false ? 'warn' : 'ok'">{{ ev.adopted === false ? "↩" : "✓" }}</span>
              <span class="lb-ev-verb">
                {{ ev.adopted === null ? "评估完成" : ev.adopted ? "假设成立，已采纳" : "假设不成立，候选已丢弃" }}
                — 平均 {{ ev.avg_score.toFixed(1) }}
                <template v-if="ev.prev_avg != null">（参照 {{ ev.prev_avg.toFixed(1) }}）</template>
              </span>
            </div>
          </template>
          <template v-else>
            <div class="lb-ev-row"><span class="lb-ev-ico err">✗</span><span class="lb-ev-verb err">{{ ev.message }}</span></div>
          </template>
        </div>
      </div>

      <!-- 记分板（Penguin scoreboard：每个分数可追溯） -->
      <div class="lb-board">
        <div class="lb-label">记分板 <span v-if="!board.length" class="lb-hint">还没有评估——点「跑一轮评估」建立基线</span></div>
        <table v-if="board.length" class="lb-table">
          <thead>
            <tr><th>版本</th><th>类型</th><th>平均分</th><th>模型</th><th>均时</th><th>调用</th><th>时间</th><th>结论</th></tr>
          </thead>
          <tbody>
            <tr v-for="(r, i) in [...board].reverse()" :key="i">
              <td class="mono">v{{ r.state_version }}</td>
              <td><span class="lb-kind" :class="r.kind + (r.adopted === false ? '-drop' : '')">{{ kindLabel(r.kind, r.adopted) }}</span></td>
              <td class="mono lb-score">{{ r.avg_score.toFixed(1) }}</td>
              <td class="mono">{{ r.model }}</td>
              <td class="mono">{{ (r.avg_duration_ms / 1000).toFixed(1) }}s</td>
              <td class="mono">{{ r.calls }}</td>
              <td class="mono">{{ fmtTime(r.ts) }}</td>
              <td class="lb-summary" :title="r.summary">{{ r.summary_title }}</td>
            </tr>
          </tbody>
        </table>

        <!-- 最近一轮逐题明细（可追溯） -->
        <template v-if="last && last.cases.length">
          <div class="lb-label" style="margin-top: 18px">最近评估逐题明细（{{ last.summary_title }}）
            <button class="lb-mini" @click="showRubric = showRubric === -1 ? 0 : -1">{{ showRubric === -1 ? "显示评分细则" : "隐藏评分细则" }}</button>
          </div>
          <div v-for="(c, i) in last.cases" :key="c.id" class="lb-case">
            <div class="lb-ev-row" @click="openDiffCase = openDiffCase === i ? -1 : i">
              <span class="lb-ev-ico ok">✓</span>
              <span class="lb-ev-verb">{{ caseTitle(c.id) }}</span>
              <span class="lb-ev-score">{{ c.score.toFixed(0) }} 分</span>
              <span class="lb-ev-dur">{{ (c.duration_ms / 1000).toFixed(1) }}s</span>
              <span class="lb-chev">{{ openDiffCase === i ? "▾" : "▸" }}</span>
            </div>
            <div v-if="openDiffCase === i" class="lb-case-detail">
              <div class="lb-k">评估员</div>
              <div class="lb-note">{{ c.note || "（无理由）" }}</div>
              <template v-if="showRubric !== -1">
                <div class="lb-k" style="margin-top: 8px">评分细则（私有，被测不可见）</div>
                <div v-for="r in bench?.cases.find(x => x.id === c.id)?.rubric ?? []" :key="r.name" class="lb-rubric">
                  <b>{{ r.name }}（{{ r.weight }} 分）</b> — {{ r.guide }}
                </div>
              </template>
            </div>
          </div>
        </template>
      </div>
    </template>
  </div>
</template>

<style scoped>
.lb-root { max-width: 1080px; margin: 0 auto; padding: 24px 28px 96px; }
.lb-head { display: flex; align-items: center; gap: 10px; margin-bottom: 16px; flex-wrap: wrap; }
.lb-title { display: flex; align-items: center; gap: 10px; }
.lb-title h2 { margin: 0; font-size: 17px; font-weight: 700; }
.lb-chip { font-size: 11px; color: var(--text-2); border: 1px solid var(--line-strong); border-radius: 999px; padding: 2px 10px; }
.lb-cost { color: var(--text-3); font-size: 11px; }
.lb-btn {
  background: none; border: 1px solid var(--line-strong); color: var(--text);
  border-radius: 8px; font-size: 12.5px; font-weight: 600; padding: 6px 14px; cursor: pointer;
  transition: all .12s ease;
}
.lb-btn:hover:not(:disabled) { background: var(--bg-hover); border-color: #3a3a46; }
.lb-btn.primary { background: #e8e8f0; color: #101014; border-color: transparent; }
.lb-btn.primary:hover:not(:disabled) { background: #fff; }
.lb-btn:disabled { opacity: .4; cursor: default; }
.lb-rounds { display: inline-flex; gap: 4px; }
.lb-round {
  background: none; border: 1px solid var(--line); border-radius: 999px;
  color: var(--text-3); font-size: 11px; padding: 3px 10px; cursor: pointer;
}
.lb-round.on { color: var(--text); border-color: var(--line-strong); background: var(--bg-hover); }
.lb-flash {
  color: #cfe8d8; font-size: 12.5px; border: 1px solid #215437; border-radius: 8px;
  padding: 8px 12px; margin-bottom: 14px; white-space: pre-wrap;
}
.lb-loading { color: var(--text-3); font-size: 13px; padding: 40px 0; }

.lb-live { border: 1px solid #22222a; border-radius: 12px; padding: 10px 14px; margin-bottom: 18px; }
.lb-ev { padding: 2px 0; }
.lb-ev-row { display: flex; align-items: center; gap: 9px; padding: 4px 6px; border-radius: 7px; cursor: default; }
.lb-ev-ico { flex: none; font-size: 11px; color: var(--text-3); }
.lb-ev-ico.ok { color: var(--good); }
.lb-ev-ico.err { color: var(--danger); }
.lb-ev-ico.warn { color: #c9a6ff; }
.lb-ev-verb { font-size: 12.5px; color: var(--text-2); }
.lb-ev-verb.err { color: #ffb3b3; }
.lb-ev-score { font-size: 11.5px; color: #e2e4ec; font-family: var(--mono, monospace); }
.lb-ev-dur { font-size: 10.5px; color: var(--text-3); font-family: var(--mono, monospace); }
.lb-chev { margin-left: auto; font-size: 9px; color: var(--text-3); }
.lb-ev-note { color: var(--text-3); font-size: 11.5px; padding: 0 6px 4px 26px; }
.lb-spin {
  width: 11px; height: 11px; border-radius: 50%; flex: none;
  border: 2px solid #2a2a34; border-top-color: var(--text-2);
  animation: lb-spin .8s linear infinite;
}
@keyframes lb-spin { to { transform: rotate(360deg); } }

.lb-proposal { border: 1px solid #2e2e38; border-radius: 10px; padding: 10px 14px; margin: 6px 0; }
.lb-ap-title { font-size: 12.5px; font-weight: 600; color: #e8eaf2; }
.lb-ap-sub { font-size: 11.5px; color: var(--text-3); margin-top: 3px; }
.lb-diff {
  border: 1px solid #20202a; border-radius: 8px; background: #0b0b0f;
  margin-top: 8px; max-height: 220px; overflow-y: auto;
  font-family: var(--mono, monospace); font-size: 11px; line-height: 1.6;
}
.lb-diff-line { display: flex; gap: 4px; padding: 0 10px; white-space: pre-wrap; word-break: break-all; }
.lb-diff-line.add { background: rgba(53, 208, 127, .09); color: #86cfa4; }
.lb-diff-line.del { background: rgba(255, 143, 143, .09); color: #ff9f9f; text-decoration: line-through; }
.lb-diff-line.same { color: #667087; }
.lb-diff-line.note { color: #8a8a96; font-style: italic; }
.lb-diff-sign { width: 12px; flex: none; user-select: none; }

.lb-board { margin-top: 4px; }
.lb-label { font-size: 12px; color: var(--text-2); font-weight: 600; margin-bottom: 8px; display: flex; align-items: center; gap: 10px; }
.lb-hint { color: var(--text-3); font-weight: 400; }
.lb-table { width: 100%; border-collapse: collapse; font-size: 12px; }
.lb-table th {
  text-align: left; color: var(--text-3); font-weight: 500; font-size: 11px;
  border-bottom: 1px solid var(--line); padding: 6px 10px;
}
.lb-table td { border-bottom: 1px solid #191920; padding: 7px 10px; color: var(--text-2); }
.lb-table td.mono { font-family: var(--mono, monospace); font-size: 11.5px; }
.lb-score { color: #e2e4ec; font-weight: 600; }
.lb-kind {
  font-size: 10.5px; border: 1px solid #26262e; border-radius: 999px; padding: 1px 8px;
  color: var(--text-2); white-space: nowrap;
}
.lb-kind.baseline { color: var(--accent); border-color: #233a52; }
.lb-kind.candidate { color: var(--good); border-color: #215437; }
.lb-kind-candidate-drop, .lb-kind[class*="drop"] { color: #c9a6ff; border-color: #4a3a66; }
.lb-summary { max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-3); }

.lb-case { border-top: 1px solid #191920; }
.lb-case .lb-ev-row { cursor: pointer; }
.lb-case .lb-ev-row:hover { background: #141419; }
.lb-case-detail { padding: 4px 10px 12px 26px; }
.lb-k { font-size: 10.5px; color: var(--text-3); margin-bottom: 3px; }
.lb-note { color: var(--text-2); font-size: 12px; }
.lb-rubric { color: var(--text-3); font-size: 11.5px; padding: 2px 0; }
.lb-rubric b { color: var(--text-2); font-weight: 600; }
.lb-mini {
  background: none; border: 1px solid var(--line-strong); border-radius: 6px;
  color: var(--text-3); font-size: 10.5px; padding: 1px 8px; cursor: pointer;
}
.lb-mini:hover { color: var(--text-2); }
</style>
