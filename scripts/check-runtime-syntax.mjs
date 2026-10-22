// 提取 runtime/index.html 的 module script 并做语法检查（node --check 等效）
import fs from "node:fs";
import { execSync } from "node:child_process";

const html = fs.readFileSync("src-tauri/runtime/index.html", "utf-8");
const m = html.match(/<script type="module">([\s\S]*?)<\/script>/);
if (!m) throw new Error("module script not found");
fs.writeFileSync("./_rtcheck.mjs", m[1]);
execSync("node --check ./_rtcheck.mjs", { stdio: "inherit" });
console.log("runtime/index.html JS syntax OK, script bytes:", m[1].length);
