// XLite × OriEra 引擎全模型矩阵测试
// 用真实 kernel.js HttpChatProvider（与游戏运行时同一代码路径）逐模型调用，
// 通过包装 globalThis.fetch 捕获每请求的 usage 与延迟。
import { pathToFileURL, fileURLToPath } from "node:url";
import { performance } from "node:perf_hooks";

// Key 由使用者本地注入，不写入仓库：
//   PowerShell:  $env:XLITE_API_KEY = "你的key"
//   Git Bash:    export XLITE_API_KEY=你的key
const KEY = process.env.XLITE_API_KEY ?? "";
if (!KEY) {
  console.error("缺少 XLite API Key：请先在本地设置环境变量 XLITE_API_KEY 再运行。");
  process.exit(1);
}
// 引擎内核默认取本仓库内快照，可用 XLITE_KERNEL 覆盖。
const KERNEL = process.env.XLITE_KERNEL
  ?? fileURLToPath(new URL("../engine-snapshot/examples/11-ai-cafe/engine/kernel.js", import.meta.url));
const BASE = "https://xlite.dezai.com/v1";

// ---- 捕获层：记录每个请求的裸数据 ----
const wire = [];
const origFetch = globalThis.fetch;
globalThis.fetch = async (url, init) => {
  const t0 = performance.now();
  try {
    const res = await origFetch(url, init);
    const ms = Math.round(performance.now() - t0);
    let body = null;
    try { body = await res.clone().json(); } catch {}
    let model = null;
    try { model = JSON.parse(init.body).model; } catch {}
    wire.push({
      url: String(url), model, status: res.status, ms,
      usage: body?.usage ? {
        prompt: body.usage.prompt_tokens ?? body.usage.input_tokens,
        completion: body.usage.completion_tokens ?? body.usage.output_tokens,
        total: body.usage.total_tokens,
      } : null,
      errMsg: body?.error?.message?.slice(0, 120) ?? null,
    });
    return res;
  } catch (e) {
    wire.push({ url: String(url), model: null, status: 0, ms: Math.round(performance.now() - t0), usage: null, errMsg: String(e).slice(0, 120) });
    throw e;
  }
};

// ---- 引擎装配（与 index.html 相同：createProvider(cfg)） ----
const { HttpChatProvider, createProvider } = await import(pathToFileURL(KERNEL).href);

// 代表性文本模型矩阵（全家桶各家族）
const MODELS = [
  // GPT 家族
  "gpt-4o-mini", "gpt-4o", "gpt-5-mini", "gpt-5.2",
  // Claude 家族
  "claude-haiku-4-5-20251001", "claude-sonnet-4-6", "claude-sonnet-5-5", "claude-haiku-5-5",
  // Gemini 家族
  "gemini-2.5-flash", "gemini-2.5-pro", "gemini-3.6-flash", "gemini-3.7-flash",
  // Qwen 家族
  "qwen-plus", "qwen3.8-max", "qwen3-coder-plus",
  // DeepSeek 家族
  "deepseek-v4-flash", "DeepSeek-V4-Pro", "deepseek-v4.1-flash",
  // GLM 家族
  "glm-5.3", "glm-5.3-flash", "GLM-5-Turbo",
  // Kimi 家族
  "Kimi-K2.6", "kimi-k2.7-code",
  // 豆包家族
  "Doubao-Seed-1.6", "doubao-seed-2-0-pro-260215",
  // 其他
  "MiniMax-M3", "grok-4.6",
];

// 咖啡馆 NPC 风格提示词（与 CognitionSystem 实际请求形状一致）
const NPC_MESSAGES = [
  { role: "system", content: "你是隅角咖啡馆的常客阿哲，一名迷茫的大学生。用一句不超过 40 字的中文台词回应，符合角色身份，可对他人说话。" },
  { role: "user", content: "（当前场景：傍晚，店内只剩你和老周，咖啡机嘶嘶作响）轮到你说话了。" },
];

const results = [];
const startWire = 0;

async function callModel(model, opts = { max: 80, temp: 0.85 }) {
  const provider = createProvider({ kind: "xlite", key: KEY, model, base: BASE });
  const mark = wire.length;
  const t0 = performance.now();
  try {
    const text = await provider.complete(NPC_MESSAGES, opts);
    const rec = wire.slice(mark).find(w => w.model === model) || wire[wire.length - 1];
    return { model, ok: true, ms: Math.round(performance.now() - t0), apiMs: rec?.ms ?? null,
      status: rec?.status, usage: rec?.usage, text: (text || "").slice(0, 60), retries: wire.slice(mark).length - 1 };
  } catch (e) {
    const rec = wire.slice(mark).find(w => w.model === model);
    return { model, ok: false, ms: Math.round(performance.now() - t0), status: rec?.status ?? 0,
      error: (rec?.errMsg || String(e)).slice(0, 150), attempts: wire.slice(mark).length };
  }
}

// ---- 阶段 1：全模型矩阵（每模型 2 轮） ----
console.log(`\n===== 阶段 1：全模型矩阵（${MODELS.length} 个模型 × 2 轮） =====`);
for (const m of MODELS) {
  for (let round = 1; round <= 2; round++) {
    const r = await callModel(m);
    r.round = round;
    results.push(r);
    const tag = r.ok ? `✓ ${r.ms}ms ${r.usage ? `tok ${r.usage.prompt}+${r.usage.completion}` : "no-usage"}` : `✗ ${r.status} ${r.error?.slice(0, 60)}`;
    console.log(`${m} #${round}: ${tag}${r.ok && r.text ? " | " + r.text.slice(0, 30) : ""}`);
  }
  await new Promise(z => setTimeout(z, 300));
}

// ---- 阶段 2：突发稳定性（3 个模型 × 6 连发，测降级与重试行为） ----
console.log(`\n===== 阶段 2：突发稳定性（3 模型 × 6 连发） =====`);
const BURST = ["gpt-4o-mini", "glm-5.3-flash", "deepseek-v4.1-flash"];
for (const m of BURST) {
  for (let i = 1; i <= 6; i++) {
    const r = await callModel(m, { max: 40 });
    r.phase = "burst";
    results.push(r);
    console.log(`${m} burst${i}: ${r.ok ? `✓ ${r.ms}ms` : `✗ ${r.status}`}`);
  }
}

// ---- 汇总 ----
const byModel = {};
for (const r of results) {
  byModel[r.model] ??= { total: 0, ok: 0, msSum: 0, msMin: 1e9, msMax: 0, prompt: 0, completion: 0, errors: [] };
  const b = byModel[r.model];
  b.total++;
  if (r.ok) { b.ok++; b.msSum += r.ms; b.msMin = Math.min(b.msMin, r.ms); b.msMax = Math.max(b.msMax, r.ms);
    if (r.usage) { b.prompt += r.usage.prompt; b.completion += r.usage.completion; } }
  else b.errors.push(`${r.status}:${(r.error || "").slice(0, 50)}`);
}
const summary = Object.entries(byModel).map(([m, b]) => ({
  model: m, success: `${b.ok}/${b.total}`,
  avgMs: b.ok ? Math.round(b.msSum / b.ok) : null,
  minMs: b.ok && b.msMin < 1e9 ? b.msMin : null, maxMs: b.msMax || null,
  promptTok: b.prompt, completionTok: b.completion,
  errors: b.errors.length ? b.errors.slice(0, 2) : undefined,
}));

const totalCalls = results.length;
const okCalls = results.filter(r => r.ok).length;
const totalPrompt = results.reduce((a, r) => a + (r.usage?.prompt || 0), 0);
const totalCompletion = results.reduce((a, r) => a + (r.usage?.completion || 0), 0);

const report = {
  testedAt: new Date().toISOString(),
  enginePath: "kernel.js HttpChatProvider (createProvider cfg kind=xlite)",
  totals: { models: MODELS.length, calls: totalCalls, ok: okCalls, fail: totalCalls - okCalls,
    successRate: (okCalls / totalCalls * 100).toFixed(1) + "%",
    promptTokens: totalPrompt, completionTokens: totalCompletion, totalTokens: totalPrompt + totalCompletion },
  perModel: summary,
};

console.log(`\n===== 汇总 =====`);
console.log(JSON.stringify(report.totals, null, 2));
console.table(summary.map(s => ({ model: s.model, 成功: s.success, 平均ms: s.avgMs, 最快: s.minMs, 最慢: s.maxMs, 入tok: s.promptTok, 出tok: s.completionTok })));

// 引擎侧 stats 校验（HttpChatProvider 自己数了多少 token）
console.log("\n（引擎 stats.token 计数走 wire 里的 usage.total_tokens 求和：",
  wire.reduce((a, w) => a + (w.usage?.total || 0), 0), "）");

import { writeFileSync } from "node:fs";
writeFileSync(fileURLToPath(new URL("./xlite-matrix-report.json", import.meta.url)), JSON.stringify({ report, wire }, null, 1));
console.log("\n完整报告: scripts/xlite-matrix-report.json");
