#!/usr/bin/env node
/**
 * engine-snapshot 同步器（studio#3 的解法 · A+B 组合）
 *
 * 把引擎仓 examples/ 的演示目录同步进 engine-snapshot/examples/：
 *   - 机械复制 + 文本 LF 归一（Windows autocrlf 不再污染快照）
 *   - 删除源里已消失的目录（真同步，不搞只增不减）
 *   - --check 模式：只报告漂移，退出码即结论（CI 漂移监视用）
 *   - 顺带核对 demos.rs 元数据覆盖（新 demo 缺中文条目 → WARN，不拦门）
 *
 * 用法：
 *   node scripts/sync-engine-snapshot.mjs --engine /path/to/engine          # 实同步
 *   node scripts/sync-engine-snapshot.mjs --engine /path/to/engine --check  # 只查
 * 引擎路径缺省取 ORIERA_ENGINE_ROOT，否则假定与 studio 同级（../engine）。
 */
import fs from "node:fs";
import path from "node:path";
import { execSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const check = args.includes("--check");
const engineArgIdx = args.indexOf("--engine");
const engineRoot = path.resolve(
  engineArgIdx >= 0 ? args[engineArgIdx + 1]
    : process.env.ORIERA_ENGINE_ROOT
    ? path.resolve(repoRoot, process.env.ORIERA_ENGINE_ROOT)
    : path.resolve(repoRoot, "..", "engine"),
);
const srcRoot = path.join(engineRoot, "examples");
const dstRoot = path.join(repoRoot, "engine-snapshot", "examples");

const TEXT_EXT = new Set([".js", ".mjs", ".html", ".json", ".md", ".css", ".txt", ".ts"]);
const isDemoDir = (n) => /^\d\d-/.test(n) || n === "_template";

const listDemoDirs = (root) =>
  fs.existsSync(root) ? fs.readdirSync(root).filter((n) => isDemoDir(n) && fs.statSync(path.join(root, n)).isDirectory()) : [];

const walk = (dir, base = dir) =>
  fs.readdirSync(dir).flatMap((f) => {
    const p = path.join(dir, f);
    return fs.statSync(p).isDirectory() ? walk(p, base) : [path.relative(base, p).split(path.sep).join("/")];
  });

const content = (file) => {
  const buf = fs.readFileSync(file);
  const rel = path.relative(srcRoot, file);
  const ext = path.extname(file);
  return TEXT_EXT.has(ext) ? Buffer.from(buf.toString("utf8").replace(/\r\n/g, "\n"), "utf8") : buf;
};

const engineHead = (() => {
  try { return execSync("git rev-parse --short HEAD", { cwd: engineRoot }).toString().trim(); }
  catch { return "unknown"; }
})();

// ---------- 漂移收集 ----------
const srcDirs = listDemoDirs(srcRoot);
const dstDirs = listDemoDirs(dstRoot);
const drift = [];
for (const d of dstDirs) if (!srcDirs.includes(d)) drift.push(`- 源已删除：examples/${d}/`);
for (const d of srcDirs) {
  const sFiles = new Map(walk(path.join(srcRoot, d)).map((f) => [f, path.join(srcRoot, d, f)]));
  const dFiles = new Map(fs.existsSync(path.join(dstRoot, d)) ? walk(path.join(dstRoot, d)).map((f) => [f, path.join(dstRoot, d, f)]) : []);
  for (const [f, p] of sFiles) if (!dFiles.has(f)) drift.push(`- 新文件：examples/${d}/${f}`);
  for (const [f, p] of dFiles) if (!sFiles.has(f)) drift.push(`- 多余文件：examples/${d}/${f}`);
  for (const [f, sp] of sFiles) {
    if (!dFiles.has(f)) continue;
    if (!content(sp).equals(content(dFiles.get(f)))) drift.push(`- 内容不同：examples/${d}/${f}`);
  }
}

// ---------- contracts/golden 单一来源同步（#98 双端 fixture） ----------
// 快照里的 kernel.test.mjs 与引擎共读同一份 golden JSON；契约文件必须
// 随快照分发，否则快照测试因相对路径不可达而假红。
const goldenSrc = path.join(engineRoot, "contracts", "golden");
const goldenDst = path.join(repoRoot, "engine-snapshot", "contracts", "golden");
const goldenNames = fs.existsSync(goldenSrc)
  ? fs.readdirSync(goldenSrc).filter((f) => f.endsWith(".json"))
  : [];
const goldenContent = (root, f) =>
  fs.readFileSync(path.join(root, f)).toString("utf8").replace(/\r\n/g, "\n");
for (const f of goldenNames) {
  const dst = path.join(goldenDst, f);
  if (!fs.existsSync(dst)) drift.push(`- 新文件：contracts/golden/${f}`);
  else if (goldenContent(goldenSrc, f) !== goldenContent(goldenDst, f))
    drift.push(`- 内容不同：contracts/golden/${f}`);
}

// ---------- demos.rs 元数据核对（编辑性，WARN 不拦门） ----------
const demosRs = fs.readFileSync(path.join(repoRoot, "src-tauri", "src", "demos.rs"), "utf8");
const warnings = [];
for (const d of srcDirs.filter((x) => x !== "_template"))
  if (!demosRs.includes(`"${d}"`)) warnings.push(`- demos.rs 缺元数据条目：${d}（Studio 演示厅不显示，需手工补中文简介）`);

// ---------- 输出与落盘 ----------
if (drift.length === 0 && warnings.length === 0) {
  console.log(`快照与引擎 ${engineHead} 一致：${srcDirs.length} 个演示目录，零漂移。`);
  process.exit(0);
}
if (warnings.length) console.log("WARN（元数据，需人工补）：\n" + warnings.join("\n"));
if (drift.length) console.log(`漂移 ${drift.length} 项（相对引擎 ${engineHead}）：\n` + drift.join("\n"));
if (check) {
  console.log(`\n--check：发现 ${drift.length} 项漂移${warnings.length ? ` + ${warnings.length} 条元数据提醒` : ""}。`);
  process.exit(drift.length ? 1 : 0);
}

// 实同步：先删源已无的目录，再整目录覆盖写入（文本 LF 归一）
for (const d of dstDirs) if (!srcDirs.includes(d)) fs.rmSync(path.join(dstRoot, d), { recursive: true });
let files = 0;
for (const d of srcDirs) {
  const dstDir = path.join(dstRoot, d);
  fs.rmSync(dstDir, { recursive: true, force: true });
  for (const f of walk(path.join(srcRoot, d))) {
    const dst = path.join(dstDir, f);
    fs.mkdirSync(path.dirname(dst), { recursive: true });
    const ext = path.extname(f);
    if (TEXT_EXT.has(ext)) fs.writeFileSync(dst, content(path.join(srcRoot, d, f)));
    else fs.copyFileSync(path.join(srcRoot, d, f), dst);
    files++;
  }
}
for (const f of goldenNames) {
  fs.mkdirSync(goldenDst, { recursive: true });
  fs.writeFileSync(path.join(goldenDst, f), goldenContent(goldenSrc, f));
  files++;
}
fs.writeFileSync(
  path.join(repoRoot, "engine-snapshot", "VERSION.json"),
  JSON.stringify({ engine: engineHead, syncedAt: new Date().toISOString(), demos: srcDirs.length, files }, null, 2) + "\n",
);
console.log(`已同步：${srcDirs.length} 个目录 / ${files} 个文件 ← 引擎 ${engineHead}${warnings.length ? `（另有 ${warnings.length} 条元数据 WARN 待人工处理）` : ""}`);
