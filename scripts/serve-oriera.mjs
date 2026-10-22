// Studio 同构静态服务器：/projects/* + /examples/* + /__provider.json（规则引擎）
// 用法: node scripts/serve-oriera.mjs [port]
import http from "node:http";
import fs from "node:fs";
import path from "node:path";

const REPO = path.resolve(import.meta.dirname, "..");
const SNAPSHOT = path.join(REPO, "src-tauri", "engine-snapshot");
const PROJECTS = path.join(process.env.APPDATA, "cn.oriera.studio", "projects");
const PORT = +(process.argv[2] || 17890);

const MIME = {
  html: "text/html; charset=utf-8", js: "text/javascript; charset=utf-8",
  mjs: "text/javascript; charset=utf-8", css: "text/css; charset=utf-8",
  json: "application/json; charset=utf-8", png: "image/png", jpg: "image/jpeg",
  glb: "model/gltf-binary", gltf: "model/gltf+json", svg: "image/svg+xml",
};

http.createServer((req, res) => {
  const urlPath = decodeURIComponent(new URL(req.url, "http://x").pathname);
  const send = (code, type, body) => {
    res.writeHead(code, { "Content-Type": type, "Cache-Control": "no-store" });
    res.end(body);
  };
  if (urlPath === "/__provider.json") return send(200, MIME.json, JSON.stringify({ kind: "rule" }));
  let root = null, rel = null;
  if (urlPath.startsWith("/examples/")) { root = SNAPSHOT; rel = urlPath.slice("/examples/".length); }
  else if (urlPath.startsWith("/projects/")) { root = PROJECTS; rel = urlPath.slice("/projects/".length); }
  if (!root) return send(404, "text/plain", "not found");
  const full = path.join(root, rel);
  if (rel.split("/").some(seg => seg === "..")) return send(404, "text/plain", "not found");
  const target = fs.existsSync(full) && fs.statSync(full).isDirectory() ? path.join(full, "index.html") : full;
  if (!fs.existsSync(target)) return send(404, "text/plain", "not found");
  const ext = path.extname(target).slice(1).toLowerCase();
  send(200, MIME[ext] || "application/octet-stream", fs.readFileSync(target));
}).listen(PORT, "127.0.0.1", () => console.log(`oriera-serve http://127.0.0.1:${PORT}`));
