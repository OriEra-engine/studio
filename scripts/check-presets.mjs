// 预设对齐校验：api.ts 的 CLOUD_PRESETS 与引擎快照 kernel.js 的 PROVIDER_PRESETS
// 必须同 kind 同 base。模型名允许不同（UI 可先于内核快照更新），端点不一致即失败。
// 用法：node scripts/check-presets.mjs  （CI 与本地均可跑，无依赖）
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const apiTs = readFileSync(join(root, "src", "api.ts"), "utf-8");
const kernelJs = readFileSync(
  join(root, "engine-snapshot", "examples", "11-ai-cafe", "engine", "kernel.js"),
  "utf-8",
);

// ---- 从 kernel.js 提取 PROVIDER_PRESETS 对象字面量并求值 ----
const kernelBlock = kernelJs.match(/export const PROVIDER_PRESETS = \{([\s\S]*?)\n\};/);
if (!kernelBlock) {
  console.error("✗ kernel.js 中找不到 PROVIDER_PRESETS");
  process.exit(1);
}
let kernelPresets;
try {
  kernelPresets = new Function(`return {${kernelBlock[1]}}`)();
} catch (e) {
  console.error(`✗ PROVIDER_PRESETS 字面量解析失败：${e.message}`);
  process.exit(1);
}

// ---- 从 api.ts 提取 CLOUD_PRESETS 各记录的 kind/base ----
const cloudBlock = apiTs.match(/export const CLOUD_PRESETS[^=]*= \[([\s\S]*?)\n\];/);
if (!cloudBlock) {
  console.error("✗ api.ts 中找不到 CLOUD_PRESETS");
  process.exit(1);
}
const uiPresets = [];
const recordRe = /\{\s*kind:\s*"([^"]+)"[\s\S]*?\}/g;
// 逐条切割：按顶层大括号配对太重，这里利用每条记录单行 base 的事实
for (const m of cloudBlock[1].matchAll(/kind:\s*"([^"]+)"[\s\S]*?base:\s*"([^"]*)"/g)) {
  uiPresets.push({ kind: m[1], base: m[2] });
}
if (uiPresets.length === 0) {
  console.error("✗ 未能从 api.ts 解析出任何 CLOUD_PRESETS 记录");
  process.exit(1);
}

// ---- 比对 ----
let failed = false;
for (const ui of uiPresets) {
  const k = kernelPresets[ui.kind];
  if (!k) {
    // UI 新增厂商而内核快照还没有：提醒但不阻塞（快照滞后是常态，同步动作是人工的）
    console.warn(`⚠ ${ui.kind} 在内核快照 PROVIDER_PRESETS 中不存在（快照可能滞后，跑 sync-engine-snapshot.mjs 对齐）`);
    continue;
  }
  if (k.base !== ui.base) {
    console.error(`✗ ${ui.kind} base 不一致：api.ts=${ui.base} kernel=${k.base}`);
    failed = true;
  }
}
if (failed) {
  console.error("\n预设表漂移：请将 api.ts 与 engine-snapshot kernel.js 对齐后重试。");
  process.exit(1);
}
console.log(`✓ 预设对齐通过（api.ts ${uiPresets.length} 条 vs 内核快照 ${Object.keys(kernelPresets).length} 行）`);
