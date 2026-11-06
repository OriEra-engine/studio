#!/usr/bin/env node
/**
 * engine-snapshot 同步器（studio#3 的解法 · A+B 组合）
 *
 * 把引擎仓 examples/ 的演示目录同步进 engine-snapshot/examples/：
 *   - 机械复制 + 文本 LF 归一（Windows autocrlf 不再污染快照）
 *   - 删除源里已消失的目录（真同步，不搞只增不减）
 *   - --check 模式：只报告漂移，退出码即结论（CI 漂移监视用）
 *   - 顺带核对 demos.rs 元数据覆盖（新 demo 缺中文条目 → WARN，不拦门）
 *   - M2-C：官方 spec 库 → engine-snapshot/specs/（Studio /sim 三库工作台的预设墙），
 *     含 extends 展开版 + 内核版本守卫（WARN）
 *
 * 用法：
 *   node scripts/sync-engine-snapshot.mjs --engine /path/to/engine          # 实同步
 *   node scripts/sync-engine-snapshot.mjs --engine /path/to/engine --check  # 只查
 * 引擎路径缺省取 ORIERA_ENGINE_ROOT，否则假定与 studio 同级（../engine）。
 */
import fs from "node:fs";
import path from "node:path";
import { execSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

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

// ---------- 官方 spec 库（M2-C · PLAN-embodied-general §四 M2「Studio 三库工作台」/ G-10） ----------
// 引擎 crates/oriera-agent/specs/{official/*.json, INDEX.json} → engine-snapshot/specs/：
//   official/<name>.json  逐字（LF 归一）——CLI `--spec` 吃的同一份文件（一份规格·四个入口）
//   INDEX.json            逐字——name → spec_hash 登记表（Studio 预设卡片的身份锚）
//   resolved/<name>.json  生成——`extends` 展开后的完整 spec。浏览器里的 wasm 没有文件系统，
//                         读不到 extends 的基 spec（spec.rs resolve_extends_in 走 std::fs），
//                         所以 Studio 发给内核的是展开版。展开不改身份：引擎加载时同样先展开、
//                         extends 键不进 hash——spec_hash 仍以 INDEX.json 为准（内核版本守卫逐个核对）。
// 展开规则逐条移植 spec.rs merge_overlay：对象逐键合并、数组逐下标合并（多出的追加）、其余替换；
// `kind` 标签不同 / 单键外部标签变体换了键（{"fixed":…} 盖 {"range":…}）= 整体替换。
// L9 拆分后（engine #175）spec 库搬进 oriera-embodied；旧路径兜底读老引擎。
const specsSrc = ["crates/oriera-embodied/specs", "crates/oriera-agent/specs"]
  .map((rel) => path.join(engineRoot, ...rel.split("/")))
  .find((p) => fs.existsSync(p)) ?? path.join(engineRoot, "crates", "oriera-agent", "specs");
const specsDst = path.join(repoRoot, "engine-snapshot", "specs");
const officialDir = path.join(specsSrc, "official");
const isObj = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
const deepEqual = (a, b) => {
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((x, i) => deepEqual(x, b[i]));
  if (isObj(a) && isObj(b)) {
    const ka = Object.keys(a);
    return ka.length === Object.keys(b).length && ka.every((k) => Object.hasOwn(b, k) && deepEqual(a[k], b[k]));
  }
  return a === b;
};
function mergeOverlay(base, overlay) {
  if (isObj(base) && isObj(overlay)) {
    const kindChanges = Object.hasOwn(base, "kind") && Object.hasOwn(overlay, "kind") && !deepEqual(base.kind, overlay.kind);
    const bk = Object.keys(base);
    const ok = Object.keys(overlay);
    const variantChanges = bk.length === 1 && ok.length === 1 && bk[0] !== ok[0];
    if (kindChanges || variantChanges) return overlay;
    for (const k of ok) base[k] = Object.hasOwn(base, k) ? mergeOverlay(base[k], overlay[k]) : overlay[k];
    return base;
  }
  if (Array.isArray(base) && Array.isArray(overlay)) {
    overlay.forEach((ov, i) => {
      if (i < base.length) base[i] = mergeOverlay(base[i], ov);
      else base.push(ov);
    });
    return base;
  }
  return overlay;
}
const EXTENDS_MAX_DEPTH = 8; // = spec.rs EXTENDS_MAX_DEPTH
function resolveExtends(v, dir, depth = 0, chain = []) {
  if (!isObj(v) || !Object.hasOwn(v, "extends")) return v;
  const ext = v.extends;
  delete v.extends;
  if (typeof ext !== "string") throw new Error(`extends 须为字符串（官方 spec 名），当前 ${JSON.stringify(ext)}`);
  const name = ext.trim().replace(/^(?:official\/)+/, "").replace(/(?:\.json)+$/, "");
  if (!name) throw new Error("extends：名字为空");
  if (depth >= EXTENDS_MAX_DEPTH) throw new Error(`extends：继承链过深（> ${EXTENDS_MAX_DEPTH}）：${[...chain, name].join(" → ")}`);
  if (chain.includes(name)) throw new Error(`extends：循环继承：${[...chain, name].join(" → ")}`);
  const file = path.join(dir, `${name}.json`);
  if (!fs.existsSync(file)) throw new Error(`extends = "${name}"：官方库里没有（${dir}）`);
  chain.push(name);
  const base = resolveExtends(JSON.parse(fs.readFileSync(file, "utf8")), dir, depth + 1, chain);
  return mergeOverlay(base, v);
}
// 期望的快照内容（rel → Buffer），drift 与实同步共用一份。
const specFiles = new Map();
const specErrors = [];
const officialNames = fs.existsSync(officialDir)
  ? fs.readdirSync(officialDir).filter((f) => f.endsWith(".json")).sort()
  : [];
const lfBuf = (file) => Buffer.from(fs.readFileSync(file, "utf8").replace(/\r\n/g, "\n"), "utf8");
for (const f of officialNames) {
  specFiles.set(`official/${f}`, lfBuf(path.join(officialDir, f)));
  try {
    const resolved = resolveExtends(JSON.parse(fs.readFileSync(path.join(officialDir, f), "utf8")), officialDir);
    specFiles.set(`resolved/${f}`, Buffer.from(JSON.stringify(resolved, null, 2) + "\n", "utf8"));
  } catch (e) {
    specErrors.push(`- specs/official/${f} 展开失败：${e.message}`);
  }
}
if (fs.existsSync(path.join(specsSrc, "INDEX.json"))) specFiles.set("INDEX.json", lfBuf(path.join(specsSrc, "INDEX.json")));
{
  const have = fs.existsSync(specsDst) ? walk(specsDst) : [];
  for (const rel of have) if (!specFiles.has(rel)) drift.push(`- 多余文件：specs/${rel}`);
  for (const [rel, buf] of specFiles) {
    const dst = path.join(specsDst, rel);
    if (!fs.existsSync(dst)) drift.push(`- 新文件：specs/${rel}`);
    else if (!fs.readFileSync(dst).equals(buf)) drift.push(`- 内容不同：specs/${rel}`);
  }
}
if (specErrors.length) {
  console.error("✗ 官方 spec 展开失败（快照的 specs/ 保持原样，不做半同步）：\n" + specErrors.join("\n"));
  process.exitCode = 1;
}

// ---------- demos.rs 元数据核对（编辑性，WARN 不拦门） ----------
const demosRs = fs.readFileSync(path.join(repoRoot, "src-tauri", "src", "demos.rs"), "utf8");
const warnings = [];
for (const d of srcDirs.filter((x) => x !== "_template"))
  if (!demosRs.includes(`"${d}"`)) warnings.push(`- demos.rs 缺元数据条目：${d}（Studio 演示厅不显示，需手工补中文简介）`);

// ---------- 输出与落盘 ----------
let earlyClean = false;
if (drift.length === 0 && warnings.length === 0) {
  console.log(`快照与引擎 ${engineHead} 一致：${srcDirs.length} 个演示目录 + ${officialNames.length} 份官方 spec，零漂移。`);
  earlyClean = true; // 文末还有 lerobot 单源检查，不在中途退出
}
if (warnings.length) console.log("WARN（元数据，需人工补）：\n" + warnings.join("\n"));
if (drift.length) console.log(`漂移 ${drift.length} 项（相对引擎 ${engineHead}）：\n` + drift.join("\n"));
if (check) {
  if (!earlyClean) {
    console.log(`\n--check：发现 ${drift.length} 项漂移${warnings.length ? ` + ${warnings.length} 条元数据提醒` : ""}。`);
    process.exitCode = drift.length ? 1 : 0;
  }
}

if (!check) {
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
// 官方 spec 库：整目录重写（真同步）；展开有错就整段跳过（不做半同步）。
if (specErrors.length === 0) {
  fs.rmSync(specsDst, { recursive: true, force: true });
  for (const [rel, buf] of specFiles) {
    const dst = path.join(specsDst, rel);
    fs.mkdirSync(path.dirname(dst), { recursive: true });
    fs.writeFileSync(dst, buf);
    files++;
  }
}
fs.writeFileSync(
  path.join(repoRoot, "engine-snapshot", "VERSION.json"),
  JSON.stringify({ engine: engineHead, syncedAt: new Date().toISOString(), demos: srcDirs.length, specs: officialNames.length, files }, null, 2) + "\n",
);
console.log(`已同步：${srcDirs.length} 个目录 + ${officialNames.length} 份官方 spec / ${files} 个文件 ← 引擎 ${engineHead}${warnings.length ? `（另有 ${warnings.length} 条元数据 WARN 待人工处理）` : ""}`);
} // if (!check) — 实同步段结束

// ── 内核版本守卫（M2-C，WARN 不拦门）────────────────────────────────
// 用快照里的 oriera-wasm（Studio /sim 实际跑的那份）逐个加载 resolved spec：
// 能加载且 spec_hash == INDEX.json ⇒ 预设卡片在画面里可换装、与 CLI 同一身份。
// 拒载/不一致 ⇒ 浏览器内核快照落后于引擎 spec 库（Studio 卡片会提示「画面内核待重建」），
// 处置：引擎侧重建 oriera-wasm（docs/WASM-RENDERER.md §2）→ 重跑本同步器。
{
  const vendor = path.join(repoRoot, "engine-snapshot", "examples", "15-embodied-sim", "vendor", "oriera");
  const glue = path.join(vendor, "oriera_wasm.js");
  const bin = path.join(vendor, "oriera_wasm_bg.wasm");
  const indexBuf = specFiles.get("INDEX.json");
  if (fs.existsSync(glue) && fs.existsSync(bin) && indexBuf) {
    try {
      const wasm = await import(pathToFileURL(glue).href);
      await wasm.default({ module_or_path: fs.readFileSync(bin) });
      const index = JSON.parse(indexBuf.toString("utf8")).specs ?? {};
      const rows = [];
      // Studio 首屏不发 spec：内核启动自带的默认实验必须就是「作业基线」卡片那一份。
      {
        const want = index["tabletop-4dof-pick"]?.hash ?? "<INDEX 无此名>";
        try {
          const sim = wasm.OrieraSim.from_spec_json(wasm.OrieraSim.preset_spec_json());
          const got = sim.spec_hash();
          sim.free?.();
          rows.push({ ok: got === want, line: got === want ? `内核默认实验 = tabletop-4dof-pick ${got}` : `内核默认实验 spec_hash ${got} ≠ 作业基线 ${want}` });
        } catch (e) {
          rows.push({ ok: false, line: `内核默认实验加载失败——${String(e?.message ?? e).slice(0, 140)}` });
        }
      }
      for (const f of officialNames) {
        const buf = specFiles.get(`resolved/${f}`);
        if (!buf) continue;
        const name = f.replace(/\.json$/, "");
        const want = index[name]?.hash ?? "<INDEX 无此名>";
        try {
          const sim = wasm.OrieraSim.from_spec_json(buf.toString("utf8"));
          const got = sim.spec_hash();
          sim.free?.();
          rows.push({ ok: got === want, line: got === want ? `${name} ${got}` : `${name}：spec_hash ${got} ≠ INDEX ${want}` });
        } catch (e) {
          rows.push({ ok: false, line: `${name}：内核拒载——${String(e?.message ?? e).slice(0, 140)}` });
        }
      }
      const bad = rows.filter((r) => !r.ok);
      if (bad.length)
        console.log(
          `WARN（内核版本守卫：${bad.length}/${rows.length} 项不过（官方预设 + 内核默认实验）——Studio 画面里会换装失败或身份对不上；浏览器 wasm 快照落后于引擎 spec 库，` +
            `重建 oriera-wasm 后重跑本同步器）：\n` + bad.map((r) => "  - " + r.line).join("\n"),
        );
      else console.log(`  ✓ 内核版本守卫：快照 wasm 可加载全部 ${rows.length - 1} 个官方预设（spec_hash 与 INDEX 一致），默认实验 = 作业基线`);
    } catch (e) {
      console.log(`WARN：内核版本守卫未能运行（${e?.message ?? e}）`);
    }
  }
}

// ── 单源漂移检查（PLAN-embodied-sim #6）────────────────────────────
// src-tauri/src/lerobot.rs 是 engine lerobot.rs 的 vendored 副本；
// sha256 漂移 = 双实现回潮，同步失败。
{
  const { createHash } = await import("node:crypto");
  const src = path.join(engineRoot, "crates", "oriera-agent", "src", "lerobot.rs");
  const dst = path.join(repoRoot, "src-tauri", "src", "lerobot.rs");
  if (fs.existsSync(src) && fs.existsSync(dst)) {
    const sha = (f) =>
      createHash("sha256").update(fs.readFileSync(f)).digest("hex").slice(0, 16);
    // 头部 vendored 声明不计入比较（副本多一段 header）
    const body = (f) => {
      const t = fs.readFileSync(f, "utf8");
      const i = t.indexOf("use std::io::Write");
      return t.slice(i >= 0 ? i : 0);
    };
    if (body(src) !== body(dst)) {
      const msg = `lerobot.rs 单源漂移（${sha(src)} vs ${sha(dst)}）——重拷 engine/crates/oriera-agent/src/lerobot.rs`;
      if (check) {
        // WARN 不拦门（2026-11-03 主控拍板）：src-tauri 侧 vendored 副本
        // 引用 studio 不存在的 crate::embodied_env，Tauri 构建本就非本仓
        // 门禁；同步仍自动重拷，检查失败不再挡 CI。
        console.warn("  ⚠ " + msg + "（src-tauri vendored 副本，WARN 不拦门）");
      } else {
        fs.copyFileSync(src, dst);
        console.log("  ✓ lerobot.rs 单源已同步（自动重拷）");
      }
    } else {
      console.log(`  ✓ lerobot.rs 单源一致（${sha(src)}）`);
    }
  }
}
