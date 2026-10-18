// P1 修复验证：复刻 Rust 侧 json_for_html 的转义，生成恶意项目名测试项目
// 运行：node scripts/xss-test.mjs
import fs from "node:fs";

const TPL = "C:/Users/Lenovo/.zcode/workspace/default/oriera-studio/src-tauri/runtime/index.html";
const OUT = process.env.APPDATA + "/cn.oriera.studio/projects/xss-test";

const tpl = fs.readFileSync(TPL, "utf8");
const evilName = "测试</script><script>window.__PWNED=true;</script>";
const game = { id: "xss-test", name: evilName, tagline: "安全测试", core_loop: ["a"], mechanics: [], economy: [], difficulty: "x" };
const story = { framework: "三幕式", premise: "前提</script><script>alert(1)</script>", beats: [{ phase: "一", title: "t", desc: "d" }] };
const agents = [{ name: "NPC</script><script>alert(2)</script>", role: "r", persona: "p", mood: "m", first_line: "f" }];

// 与 Rust json_for_html 完全一致的逻辑
const jsonForHtml = (o) => JSON.stringify(o).replace(/</g, "\\u003c");
const htmlEscape = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

const html = tpl
  .replaceAll("__PROJECT_NAME__", htmlEscape(evilName))
  .replaceAll("__GAME_JSON__", jsonForHtml(game))
  .replaceAll("__STORY_JSON__", jsonForHtml(story))
  .replaceAll("__AGENTS_JSON__", jsonForHtml(agents));

fs.mkdirSync(OUT, { recursive: true });
fs.writeFileSync(OUT + "/index.html", html);

// 断言：JSON 中的 < 必须全部以 \u003c 形式存在，不能有裸的 </script><script> 相邻序列
const raw = (html.match(/<\/script><script>/g) || []).length;
const escaped = (html.match(/\\u003c/g) || []).length;
console.log("裸 script 逃逸序列:", raw, raw === 0 ? "PASS" : "FAIL");
console.log("\\u003c 转义出现次数:", escaped, escaped >= 4 ? "PASS" : "FAIL");
// JSON.parse 模拟页面侧解析（\u003c 在 JSON 里就是 '<'）
const parsed = JSON.parse(html.match(/const GAME = ([\s\S]*?);\n/)[1]);
console.log("页面侧解析出的游戏名还原:", parsed.name === evilName ? "PASS" : "FAIL");
