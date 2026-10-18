// 把 src/data 下的模板数据导出为 src-tauri/templates-builtin.json（Rust include_str! 内置）
// 运行：node scripts/export-templates.mjs   （修改模板数据后、打包前执行）
import { build } from "esbuild";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { execFileSync } from "node:child_process";

const root = path.resolve(import.meta.dirname, "..");
const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "oriera-tpl-"));
const entry = path.join(tmpDir, "entry.ts");
const outfile = path.join(tmpDir, "bundle.mjs");

const slash = (p) => JSON.stringify(p.replace(/\\/g, "/"));
fs.writeFileSync(
  entry,
  `import { GAMEPLAY_PACKS } from ${slash(path.join(root, "src/data/gameplay-packs.ts"))};\n` +
    `import { STORY_FRAMEWORKS } from ${slash(path.join(root, "src/data/story-frameworks.ts"))};\n` +
    `console.log(JSON.stringify({ gameplay: GAMEPLAY_PACKS, story_frameworks: STORY_FRAMEWORKS }));\n`
);

await build({ entryPoints: [entry], bundle: true, format: "esm", platform: "node", outfile });
const stdout = execFileSync(process.execPath, [outfile], { encoding: "utf8" });
const data = JSON.parse(stdout.trim());

const target = path.join(root, "src-tauri", "templates-builtin.json");
fs.writeFileSync(target, JSON.stringify(data, null, 1), "utf8");
fs.rmSync(tmpDir, { recursive: true, force: true });
console.log(`exported ${data.gameplay.length} packs + ${data.story_frameworks.length} frameworks -> ${path.relative(root, target)}`);
