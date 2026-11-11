<script setup lang="ts">
import { onMounted, ref } from "vue";
import { api } from "../api";
import type { AgentConfig, AgentStateOut, SkillMeta, SnapshotMeta } from "../api";

/**
 * 奥粒状态（v0.14.0，学 PenguinHarness 的 Agent State）：
 * prompt 与运行参数是文件，保存即自动快照（版本单调 +1），可一步回滚。
 * 进化实验室的优化器也通过同一机制落盘采纳的 prompt。
 */

const loading = ref(true);
const error = ref("");
const prompt = ref("");
const config = ref<AgentConfig | null>(null);
const snapshots = ref<SnapshotMeta[]>([]);
const saving = ref(false);
const savedFlash = ref("");
const restoring = ref(0); // 待确认回滚的版本号（0=无）

const TOOL_NAMES = [
  "list_projects", "read_project", "read_project_file", "write_project_file",
  "list_templates", "list_assets", "get_workbench_status", "update_plan", "propose_choices",
];

function fmtTime(ts: number) {
  if (!ts) return "";
  const d = new Date(ts * 1000);
  return `${d.getMonth() + 1}/${d.getDate()} ${d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })}`;
}

async function load() {
  loading.value = true;
  error.value = "";
  try {
    const s: AgentStateOut = await api.agentStateGet();
    prompt.value = s.system_prompt;
    config.value = s.config;
    snapshots.value = s.snapshots;
  } catch (e) {
    error.value = String(e).slice(0, 160);
  }
  loading.value = false;
}

async function save() {
  if (!config.value || saving.value) return;
  saving.value = true;
  savedFlash.value = "";
  try {
    const v = await api.agentStateSave(prompt.value, config.value, "手动保存");
    savedFlash.value = `已保存并快照 → 当前 v${v}`;
    await load();
  } catch (e) {
    error.value = String(e).slice(0, 160);
  }
  saving.value = false;
}

async function restore(version: number) {
  if (restoring.value !== version) {
    restoring.value = version; // 两段确认：第一次点变红提示，再点执行
    setTimeout(() => { if (restoring.value === version) restoring.value = 0; }, 3000);
    return;
  }
  restoring.value = 0;
  try {
    const v = await api.agentStateRestore(version);
    savedFlash.value = `已回滚到 v${version} 的内容 → 当前 v${v}`;
    await load();
  } catch (e) {
    error.value = String(e).slice(0, 160);
  }
}

function toolOn(name: string): boolean {
  return config.value?.tools[name] ?? true;
}
function toggleTool(name: string) {
  if (!config.value) return;
  config.value.tools[name] = !toolOn(name);
}

/* ---------- 技能管理（SKILL.md） ---------- */
const skills = ref<SkillMeta[]>([]);
const openSkill = ref("");       // 展开全文的技能名
const skillBody = ref("");
const newSkillName = ref("");
const newSkillBody = ref("");
const creatingSkill = ref(false);
const deletingSkill = ref("");

async function loadSkills() {
  try {
    skills.value = await api.skillList();
  } catch { /* 浏览器调试 */ }
}
async function toggleSkillBody(name: string) {
  if (openSkill.value === name) { openSkill.value = ""; return; }
  try {
    skillBody.value = await api.skillGet(name);
    openSkill.value = name;
  } catch (e) {
    error.value = String(e).slice(0, 160);
  }
}
async function saveNewSkill() {
  const name = newSkillName.value.trim();
  if (!name || !newSkillBody.value.trim()) return;
  try {
    await api.skillSave(name, newSkillBody.value);
    creatingSkill.value = false;
    newSkillName.value = "";
    newSkillBody.value = "";
    await loadSkills();
    savedFlash.value = `技能 ${name} 已保存（立即进入奥粒的技能索引）`;
  } catch (e) {
    error.value = String(e).slice(0, 160);
  }
}
async function removeSkill(name: string) {
  if (deletingSkill.value !== name) {
    deletingSkill.value = name;
    setTimeout(() => { if (deletingSkill.value === name) deletingSkill.value = ""; }, 3000);
    return;
  }
  deletingSkill.value = "";
  try {
    await api.skillDelete(name);
    if (openSkill.value === name) openSkill.value = "";
    await loadSkills();
  } catch (e) {
    error.value = String(e).slice(0, 160);
  }
}

onMounted(() => { load(); loadSkills(); });
</script>

<template>
  <div class="sv-root">
    <header class="sv-head">
      <div class="sv-title">
        <h2>奥粒状态</h2>
        <span v-if="config" class="sv-version mono">v{{ config.version }}</span>
        <span class="sv-sub">prompt 与参数是文件——保存即自动快照，可随时回滚（学 PenguinHarness）</span>
      </div>
      <span style="flex: 1"></span>
      <span v-if="savedFlash" class="sv-flash">{{ savedFlash }}</span>
      <button class="sv-save" :disabled="saving || !config" @click="save">
        {{ saving ? "保存中…" : "保存并快照" }}
      </button>
    </header>

    <div v-if="error" class="sv-error">{{ error }}</div>
    <div v-if="loading" class="sv-loading">读取中…</div>

    <div v-else class="sv-body">
      <div class="sv-editor">
        <div class="sv-label">system_prompt.md <span class="sv-hint">占位符 {{ "\{\{WORKBENCH\}\}" }} 运行时注入引擎快照与端口</span></div>
        <textarea v-model="prompt" spellcheck="false" class="sv-prompt"></textarea>

        <div class="sv-label" style="margin-top: 18px">运行参数</div>
        <div class="sv-config">
          <label>最大轮数 <input type="number" min="1" max="16" v-model.number="config!.max_turns" /></label>
          <label>温度 <input type="number" min="0" max="1.5" step="0.1" v-model.number="config!.temperature" /></label>
          <label>max_tokens <input type="number" min="256" max="8192" step="128" v-model.number="config!.max_tokens" /></label>
        </div>

        <div class="sv-label" style="margin-top: 18px">工具开关</div>
        <div class="sv-tools">
          <button
            v-for="t in TOOL_NAMES" :key="t"
            class="sv-tool" :class="{ off: config && !toolOn(t) }"
            :title="toolOn(t) ? '点击停用' : '点击启用'" @click="toggleTool(t)"
          >{{ t }}</button>
        </div>

        <div class="sv-label" style="margin-top: 18px">
          技能（SKILL.md）<span class="sv-hint">索引注入 system prompt，奥粒按需用 read_skill 读全文——学 PenguinHarness</span>
          <button class="sv-mini" style="margin-left: 8px" @click="creatingSkill = !creatingSkill">{{ creatingSkill ? "收起" : "＋ 新建" }}</button>
        </div>
        <div v-if="creatingSkill" class="sv-skill-new">
          <input v-model="newSkillName" placeholder="技能名（字母/数字/-，如 plot-twist）" class="sv-skill-name" />
          <textarea v-model="newSkillBody" placeholder="SKILL.md 全文——以 frontmatter 开头：&#10;---&#10;name: xxx&#10;description: 一句话描述（注入索引）&#10;version: 2026.11.08.1&#10;---&#10;正文…" class="sv-skill-body"></textarea>
          <button class="sv-mini ok" @click="saveNewSkill">保存技能</button>
        </div>
        <div class="sv-skills">
          <div v-if="!skills.length" class="sv-snap-empty">没有技能</div>
          <div v-for="sk in skills" :key="sk.name" class="sv-skill-row">
            <div class="sv-skill-head" @click="toggleSkillBody(sk.name)">
              <b class="mono">{{ sk.name }}</b>
              <span class="sv-skill-ver mono">{{ sk.version }}</span>
              <span class="sv-skill-desc">{{ sk.description || "（无描述）" }}</span>
              <span class="sv-chev">{{ openSkill === sk.name ? "▾" : "▸" }}</span>
            </div>
            <pre v-if="openSkill === sk.name" class="sv-skill-pre">{{ skillBody }}</pre>
            <button class="sv-restore danger" :class="{ confirm: deletingSkill === sk.name }" @click="removeSkill(sk.name)">
              {{ deletingSkill === sk.name ? "再点确认删除" : "删除" }}
            </button>
          </div>
        </div>
      </div>

      <aside class="sv-snaps">
        <div class="sv-label">快照（改动前的完整状态）</div>
        <div v-if="!snapshots.length" class="sv-snap-empty">还没有快照——第一次保存后出现</div>
        <div v-for="s in snapshots" :key="s.version" class="sv-snap">
          <div class="sv-snap-head">
            <b class="mono">v{{ s.version }}</b>
            <span class="sv-snap-time">{{ fmtTime(s.saved_at) }}</span>
          </div>
          <div class="sv-snap-note">{{ s.note }}</div>
          <button class="sv-restore" :class="{ confirm: restoring === s.version }" @click="restore(s.version)">
            {{ restoring === s.version ? "再点一次确认回滚" : "恢复此版本" }}
          </button>
        </div>
      </aside>
    </div>
  </div>
</template>

<style scoped>
.sv-root { max-width: 1080px; margin: 0 auto; padding: 24px 28px 96px; }
.sv-head { display: flex; align-items: center; gap: 12px; margin-bottom: 16px; }
.sv-title h2 { margin: 0; font-size: 17px; font-weight: 700; }
.sv-version {
  font-size: 11px; color: var(--good); border: 1px solid #215437;
  border-radius: 999px; padding: 1px 9px;
}
.sv-sub { color: var(--text-3); font-size: 12px; }
.sv-flash { color: var(--good); font-size: 12px; }
.sv-save {
  background: #e8e8f0; color: #101014; border: none; border-radius: 8px;
  font-size: 12.5px; font-weight: 600; padding: 6px 16px; cursor: pointer;
}
.sv-save:hover:not(:disabled) { background: #fff; }
.sv-save:disabled { opacity: .4; cursor: default; }
.sv-error { color: #ffb3b3; font-size: 12.5px; border: 1px solid #4a2226; border-radius: 8px; padding: 8px 12px; margin-bottom: 12px; }
.sv-loading { color: var(--text-3); font-size: 13px; padding: 40px 0; }

.sv-body { display: flex; gap: 20px; align-items: flex-start; }
.sv-editor { flex: 1; min-width: 0; }
.sv-label { font-size: 12px; color: var(--text-2); margin-bottom: 6px; font-weight: 600; }
.sv-hint { color: var(--text-3); font-weight: 400; font-size: 11px; }
.sv-prompt {
  width: 100%; min-height: 420px; resize: vertical;
  background: #0e0e12; border: 1px solid #26262e; border-radius: 12px;
  color: #d6d6de; font-family: var(--mono, monospace); font-size: 12.5px; line-height: 1.7;
  padding: 14px 16px; outline: none; transition: border-color .12s ease;
}
.sv-prompt:focus { border-color: #3c3c48; }
.sv-config { display: flex; gap: 18px; }
.sv-config label { display: flex; align-items: center; gap: 8px; font-size: 12px; color: var(--text-2); }
.sv-config input {
  width: 90px; background: #0e0e12; border: 1px solid #26262e; border-radius: 7px;
  color: #e8eaf2; font-size: 12.5px; padding: 4px 8px; outline: none; font-family: var(--mono, monospace);
}
.sv-tools { display: flex; flex-wrap: wrap; gap: 6px; }
.sv-tool {
  background: #121218; border: 1px solid #26262e; border-radius: 999px;
  color: var(--text-2); font-size: 11px; padding: 4px 11px; cursor: pointer;
  font-family: var(--mono, monospace); transition: all .12s ease;
}
.sv-tool:hover { border-color: #3c3c48; }
.sv-tool.off { color: var(--text-3); text-decoration: line-through; opacity: .55; }

.sv-snaps { width: 264px; flex: none; }
.sv-snap-empty { color: var(--text-3); font-size: 12px; padding: 10px 0; }
.sv-snap { border: 1px solid #22222a; border-radius: 10px; padding: 10px 12px; margin-bottom: 8px; }
.sv-snap-head { display: flex; align-items: center; gap: 8px; }
.sv-snap-head b { color: #e2e4ec; font-size: 12px; }
.sv-snap-time { color: var(--text-3); font-size: 10.5px; font-family: var(--mono, monospace); }
.sv-snap-note { color: var(--text-3); font-size: 11.5px; margin: 4px 0 8px; word-break: break-all; }
.sv-restore {
  background: none; border: 1px solid var(--line-strong); color: var(--text-2);
  border-radius: 7px; font-size: 11.5px; padding: 3px 10px; cursor: pointer;
}
.sv-restore:hover { color: var(--text); border-color: #3a3a46; }
.sv-restore.confirm { border-color: #4a2226; color: #ff9aa8; }
.sv-mini {
  background: none; border: 1px solid var(--line-strong); border-radius: 7px;
  color: var(--text-2); font-size: 11px; padding: 2px 10px; cursor: pointer;
}
.sv-mini:hover { color: var(--text); border-color: #3a3a46; }
.sv-mini.ok { color: #101014; background: #e8e8f0; border-color: transparent; font-weight: 600; margin-top: 6px; }
.sv-skills { display: flex; flex-direction: column; gap: 6px; }
.sv-skill-row { border: 1px solid #22222a; border-radius: 10px; padding: 8px 12px; }
.sv-skill-head { display: flex; align-items: center; gap: 8px; cursor: pointer; }
.sv-skill-head b { color: #e2e4ec; font-size: 12px; }
.sv-skill-ver { color: var(--text-3); font-size: 10px; }
.sv-skill-desc { flex: 1; color: var(--text-3); font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sv-chev { color: var(--text-3); font-size: 9px; }
.sv-skill-pre {
  margin: 8px 0; padding: 10px 12px; background: #0a0a0d; border: 1px solid #1d1d25; border-radius: 8px;
  font-family: var(--mono, monospace); font-size: 11px; line-height: 1.6; color: #9aa3b8;
  max-height: 260px; overflow: auto; white-space: pre-wrap; word-break: break-all;
}
.sv-skill-new { display: flex; flex-direction: column; gap: 6px; margin-bottom: 10px; }
.sv-skill-name {
  background: #0e0e12; border: 1px solid #26262e; border-radius: 7px;
  color: #e8eaf2; font-size: 12px; padding: 6px 10px; outline: none; font-family: var(--mono, monospace);
}
.sv-skill-body {
  min-height: 160px; resize: vertical;
  background: #0e0e12; border: 1px solid #26262e; border-radius: 8px;
  color: #d6d6de; font-family: var(--mono, monospace); font-size: 11.5px; line-height: 1.6;
  padding: 10px 12px; outline: none;
}
.sv-restore.danger { margin-top: 6px; }
</style>
