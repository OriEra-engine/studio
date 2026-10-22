// 《雪夜灯守》生成器 —— 走 OriEra Studio create_project 同一契约产出项目，
// 并按 singlefile.rs 同一逻辑导出「双击即玩」单文件 HTML。
// 用法: node scripts/make-snow-lamp.mjs
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const REPO = path.resolve(import.meta.dirname, "..");
const TEMPLATE = fs.readFileSync(path.join(REPO, "src-tauri/runtime/index.html"), "utf8");
const SNAPSHOT = path.join(REPO, "src-tauri/engine-snapshot"); // /examples/* 的磁盘根
const PROJECTS_DIR = path.join(process.env.APPDATA, "cn.oriera.studio", "projects");

// ---------- 与 project.rs 相同的转义规则 ----------
const htmlEscape = s => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const jsonForHtml = v => JSON.stringify(v).replace(/</g, "\\u003c");

// ---------- 游戏数据（GameSpec 同构） ----------
const ID = "snow-lamp-keeper-" + Date.now().toString(16);
const game = {
  id: ID,
  name: "雪夜灯守",
  tagline: "百年噬光暴风将至，灯塔为你而亮——或为你而熄",
  gameplay_id: "snow-lamp-keeper",
  core_loop: [
    "与三位居民对话，拼出老灯守失踪的真相",
    "用忆晶向行商购买线索，抓住关键情报",
    "在岔路口做出抉择，决定灯塔的命运",
    "把线索燃成火种，点亮长明灯",
  ],
  mechanics: [
    { name: "忆晶经济", detail: "剧情推进 +15 忆晶；买线索 -10、关键抉择各有代价。忆晶不足的选项会被真实禁用。" },
    { name: "线索门槛", detail: "「线索」只能通过冰桥上的半页手记获得。真结局路线需要 1 条线索，绕行风廊将错过它。" },
    { name: "三结局分支", detail: "第二幕终点的抉择把故事引向三个不同结局：强行点灯、寻回老灯守（真结局）、全员南撤。" },
    { name: "离线可玩", detail: "未配置模型时由 OriEra 离线性格引擎驱动 NPC；在 Studio 模型中心接入任意大模型后，NPC 获得记忆与自由对话。" },
  ],
  economy: [
    { name: "忆晶", source: "剧情推进与居民赠予", sink: "购买线索、关键抉择的代价" },
    { name: "线索", source: "旧地图与半页手记等关键抉择", sink: "解锁真结局路线" },
  ],
  difficulty: "无战斗，只有取舍：忆晶与线索都有限，每一次选择都通向不同的结局",
  user_idea: "参考抖音 AI充电官《Codex+Claude 0基础用AI做游戏》流程：游戏构思→技术选型→人物设计→AI 全流程。轻量、可交互、三结局。",
  theme: "snow",
};
const story = {
  framework: "三幕式 · 风暴倒计时",
  premise: "极北雪原尽头有一座以「忆晶」为燃料的灯塔站。你是新来的灯守学徒。今夜，百年一遇的「噬光暴风」正在逼近，而守了这座灯四十年的老灯守忽然失踪了。灯芯已经熄灭。你必须在暴风抵达之前，向站里仅剩的三位居民问出真相，凑齐点灯的办法——并亲手决定，这盏灯将为谁而亮。",
  beats: [
    { phase: "第一幕 · 暴风前夜", title: "熄灯", desc: "你在查夜时发现灯室空了：老灯守的外套还挂在椅背，灯芯冷得像块黑冰。风声里似乎有他的名字。墙上的气压计落到最左端——百年一遇的噬光暴风，还有一夜就到。" },
    { phase: "第一幕 · 暴风前夜", title: "三张面孔", desc: "站里只剩下三个人：行商雪雀守着她最后一炉忆晶；守灯诗人白砚对着熄灭的灯芯出神；机械师阿绒把齿轮拆了一地。他们都看见了什么，又都藏着什么。（试试选中雪雀或白砚，点 💡10 买一条线索）" },
    { phase: "第一幕 · 雪雀的价码", title: "旧地图", desc: "雪雀摊开一块油布：老灯守失踪前夜曾向她买走过一张旧冰桥地图。「童叟无欺——你若出 8 块忆晶，地图归你，桥上有他留下的东西。」她顿了顿，「不买也行，绕风廊，就是……费晶石。」", choices: [
      { label: "买下旧地图（忆晶-8）", effect: "忆晶-8", next: 3 },
      { label: "婉拒，绕行风廊", effect: "", next: 4 },
    ] },
    { phase: "第二幕 · 冰桥", title: "半页手记", desc: "旧地图带你走上冰桥。桥心冰缝里卡着老灯守的灯杖，还有半页被雪浸透的手记：「灯芯不是烧晶石的——它烧记忆。我这条命里最亮的一段，够它亮到天明。」你把手记贴身收好。", choices: [
      { label: "收好手记，返回灯塔（线索+1）", effect: "线索+1", next: 5 },
    ] },
    { phase: "第二幕 · 风廊", title: "白色低语", desc: "绕开冰桥的风廊里，白噪风把人的念头吹得七零八落。你在冰缝里捡到老灯守的铜徽章，除此之外一无所获——但省下的忆晶沉甸甸地压在口袋里，暖得像一句承诺。", choices: [
      { label: "顶风挤过去，直奔灯室（忆晶-5）", effect: "忆晶-5", next: 5 },
      { label: "慢下来，避开风眼再走", effect: "", next: 5 },
    ] },
    { phase: "第二幕 · 灯室", title: "火种的真相", desc: "灯室暗格里，你找到老灯守留下的字条，字迹被泪水泡开：「灯要烧记忆才能亮。四十年来烧的都是我的。我的记忆快见底了——我去风眼里，把最后一段取回来。别找我。把灯亮着。」此刻，暴风已抵桥头。你面前有三条路。", choices: [
      { label: "循手记与地图推演他的去向（线索-1）", effect: "线索-1", next: 8 },
      { label: "守在灯塔，用忆晶强行点灯（忆晶-10）", effect: "忆晶-10", next: 7 },
      { label: "放弃点灯，全员撤往南镇", effect: "", next: 6 },
    ] },
    { phase: "终幕 · 结局", title: "结局 · 南迁的长夜", desc: "队伍在暴风抵达前一刻南下。回头望，灯塔的轮廓被白色一寸寸吞掉。多年后，雪原上再没有归人，也没有人为归人留灯——只有南镇酒馆里，雪雀偶尔还讲起那盏没亮起来的灯。", choices: [
      { label: "❄️ 故事在这里落幕", effect: "", next: 9 },
    ] },
    { phase: "终幕 · 结局", title: "结局 · 强行点燃的黎明", desc: "忆晶在灯芯上炸出刺目的白光。灯亮了——只亮了一夜。暴风过境，老灯守终究没有回来。天明时你剪掉烧焦的灯芯，忽然明白：从今往后，这座灯烧的就是你的记忆了。你成了新的灯守。", choices: [
      { label: "🕯 故事在这里落幕", effect: "", next: 9 },
    ] },
    { phase: "终幕 · 真结局", title: "真结局 · 为归人长明的灯", desc: "手记与地图在你手里拼成一条风眼之路。你在暴风最深处找到蜷缩的老灯守——他正把自己最后一段记忆捧在掌心，像捧一小捧火。「原来灯在等的不止是我的记忆，」他笑了，「还有来接我的人。」你们回到灯下。那一夜，灯塔为所有归人长明。", choices: [
      { label: "✨ 故事在这里落幕", effect: "", next: 9 },
    ] },
  ],
};
const agents = [
  { name: "雪雀", role: "行商·情报贩子", persona: "八面玲珑的走私行商，被暴风困在站里。嘴上童叟无欺、生死不打折，心里比谁都盼着灯塔亮——她也是靠这盏灯认路回家的人。", mood: "外热内焦", first_line: "最后一炉忆晶，出价吧——开玩笑的，学徒。帮我个忙，晶石白送。" },
  { name: "白砚", role: "守灯诗人", persona: "与老灯守共事四十年的前任灯守，如今只写诗不点灯。诗句里全是真相的碎片，只是没人读得懂。他早就知道灯烧的是什么，也早就知道老灯守会走。", mood: "沉静", first_line: "灯不是照亮雪原的，是照亮回家的路。……他走反了方向，你没发现吗。" },
  { name: "阿绒", role: "机械师学徒", persona: "什么都能拆开、一半装不回去的乐天派少女，负责灯塔齿轮。坚信老灯守还活着，口袋里永远有热乎的灶猫饼干。她不知道灯的真相——所有人都默契地瞒着她。", mood: "元气", first_line: "齿轮上还留着他的手套味儿！他绝对没走远——顶多，顶多走到旧冰桥！" },
];
const provider = { kind: "rule", model: "rule-v1", base: "", note: "API Key 不落盘，由 OriEra Studio 运行时注入浏览器" };

// ---------- 1) 生成 Studio 项目 ----------
const dir = path.join(PROJECTS_DIR, ID);
fs.mkdirSync(dir, { recursive: true });

const html = TEMPLATE
  .replaceAll("__PROJECT_NAME__", htmlEscape(game.name))
  .replaceAll("__GAME_JSON__", jsonForHtml(game))
  .replaceAll("__STORY_JSON__", jsonForHtml(story))
  .replaceAll("__AGENTS_JSON__", jsonForHtml(agents));

const readme = `# ${game.name}\n\n> ${game.tagline}\n\n由 **OriEra Studio** 生成的 AI 原生游戏原型。\n\n- 玩法模板：\`${game.gameplay_id}\`（雪原 · 分支叙事 · 三结局）\n- 叙事框架：${story.framework}\n- NPC：${agents.map(a => a.name).join("、")}\n- 在线体验：打开 OriEra Studio → 我的项目 → 运行\n\n## 玩法\n\n- 📖「剧情」页推进节拍、做分支抉择；忆晶不足的选项会被禁用\n- 💬「对话」页与三位居民交谈（离线为性格引擎；配置模型后由大模型驱动）\n- 💡 选中雪雀/白砚可花 10 忆晶买当前剧情线索\n- 🎯「玩法」页核心循环按真实行为自动点亮；存档可导出/导入\n\n## 文件说明\n\n- \`index.html\` 游戏运行时（Three.js 场景 + AI NPC + 剧情导演）\n- \`game.json\` 玩法数据（核心循环 / 机制 / 经济）\n- \`story.json\` 剧情节拍（含分支 choices）\n- \`agents.json\` NPC 人设\n- \`provider.json\` 模型接入配置（不含密钥）\n\n## 说明\n\n- 原型依赖 OriEra 引擎内置的静态资源（Three.js / kernel.js），请在 Studio 内运行；\n  同目录导出的单文件 HTML 可脱离 Studio 双击即玩。\n- 未配置模型时自动降级为离线性格引擎；在 Studio「模型中心」配置后，NPC 由大模型驱动并拥有记忆。\n`;

fs.writeFileSync(path.join(dir, "index.html"), html);
fs.writeFileSync(path.join(dir, "game.json"), JSON.stringify(game, null, 2));
fs.writeFileSync(path.join(dir, "story.json"), JSON.stringify(story, null, 2));
fs.writeFileSync(path.join(dir, "agents.json"), JSON.stringify(agents, null, 2));
fs.writeFileSync(path.join(dir, "provider.json"), JSON.stringify(provider, null, 2));
fs.writeFileSync(path.join(dir, "README.md"), readme);

// ---------- 2) 单文件「双击即玩」导出（复刻 singlefile.rs） ----------
const b64 = buf => buf.toString("base64");
const resolve = url => {
  const rel = url.replace(/^\/examples\//, "");
  const p = path.join(SNAPSHOT, rel);
  return fs.existsSync(p) ? fs.readFileSync(p) : null;
};
const mimeFor = key => key.endsWith(".glb") ? "model/gltf-binary" : "text/javascript";
const rewriteVendor = (bytes, key) => {
  if (key.endsWith("GLTFLoader.js")) {
    return bytes.toString("utf8")
      .replaceAll("'../utils/BufferGeometryUtils.js'", '"oriera-vendor/utils/BufferGeometryUtils.js"');
  }
  return bytes.toString("utf8");
};

const START = '<script type="importmap">';
const END = "</script>";
const s = html.indexOf(START);
const e = s + html.slice(s).indexOf(END);
const map = JSON.parse(html.slice(s + START.length, e));
const imports = {};
for (const [key, url] of Object.entries(map.imports)) {
  const bytes = resolve(url);
  if (!bytes) throw new Error(`导出依赖缺失：${url}（引擎快照不完整）`);
  imports[key] = `data:${mimeFor(key)};base64,${b64(Buffer.from(rewriteVendor(bytes, key), "utf8"))}`;
}
let out = html.slice(0, s + START.length) + JSON.stringify({ imports }) + html.slice(e);
// fetch 加载的资产字面量（GLB_URL）同样内联
let from = 0;
while (true) {
  const pos = out.indexOf('"/examples/', from);
  if (pos < 0) break;
  const open = pos + 1;
  const close = open + out.slice(open).indexOf('"');
  const url = out.slice(open, close);
  const bytes = resolve(url);
  if (!bytes) throw new Error(`导出依赖缺失：${url}`);
  const dataUrl = `"data:application/octet-stream;base64,${b64(bytes)}"`;
  out = out.slice(0, open - 1) + dataUrl + out.slice(close + 1);
  from = open - 1 + dataUrl.length;
}
if (out.includes("/examples/")) throw new Error("单文件导出后仍残留 /examples/ 引用");

// 桌面优先，找不到退回仓库 dist
const desktop = path.join(os.homedir(), "Desktop");
const singleDir = fs.existsSync(desktop) ? desktop : path.join(REPO, "dist");
fs.mkdirSync(singleDir, { recursive: true });
const singlePath = path.join(singleDir, "雪夜灯守-双击即玩.html");
fs.writeFileSync(singlePath, out);

console.log("project_dir:", dir);
console.log("singlefile :", singlePath, `(${(fs.statSync(singlePath).size / 1048576).toFixed(1)} MB)`);
console.log("beats      :", story.beats.length, "| agents:", agents.length, "| theme:", game.theme);
