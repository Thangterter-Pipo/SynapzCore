// SynapzCore Dashboard v2 — Preact + htm, no build step. ponytail: single module; split into /views if it passes ~1200 lines.
import { h, render, Fragment } from "https://esm.sh/preact@10.23.1";
import { useState, useEffect, useRef, useCallback } from "https://esm.sh/preact@10.23.1/hooks";
import htm from "https://esm.sh/htm@3.1.1";
const html = htm.bind(h);

// ---- API client: real backend, callers supply demo fallback ----
const tok = () => localStorage.getItem("synapz_token") || "";
const api = {
  async get(path) {
    const r = await fetch(path, { headers: { "X-Synapz-Token": tok() } });
    if (!r.ok) throw new Error(r.status + " " + path);
    return r.json();
  },
  async post(path, body) {
    const r = await fetch(path, { method: "POST",
      headers: { "Content-Type": "application/json", "X-Synapz-Token": tok() },
      body: JSON.stringify(body || {}) });
    if (!r.ok) throw new Error(r.status + " " + path);
    return r.json().catch(() => ({}));
  },
};

const DEMO = {
  catalog: [
    { agent_id: "antigravity", label: "Antigravity (host)", role: "orchestrator", icon: "\u{1F9E0}", installed: true, enabled: true, live: true, detail: "host process" },
    { agent_id: "coder", label: "Coder CLI", role: "builder", icon: "\u{1F4BB}", installed: true, enabled: true, live: true, detail: "coder --watch" },
    { agent_id: "tester", label: "Tester", role: "tester", icon: "\u{1F9EA}", installed: true, enabled: false, live: null, detail: "tester.exe" },
    { agent_id: "researcher", label: "Researcher", role: "research", icon: "\u{1F50D}", installed: false, enabled: false, live: null, detail: "researcher --serve" },
  ],
  state: { agents: {
    antigravity: { role: "orchestrator", model: "gemini", _ts: Date.now()/1000, stale: false, status: "active" },
    coder: { role: "builder", model: "model-coder", parent_id: "antigravity", _ts: Date.now()/1000, stale: false, status: "active" },
    tester: { role: "tester", model: "model-tester", parent_id: "antigravity", _ts: Date.now()/1000-400, stale: true, status: "idle" },
  } },
  tasks: [
    { id: "api-login", prompt: "Implement POST /login", status: "in_progress", assigned_to: "coder" },
    { id: "ui-login", prompt: "Build login form", status: "todo", assigned_to: "coder" },
    { id: "test-login", prompt: "Write login tests", status: "done", assigned_to: "tester" },
  ],
  log: Array.from({length: 8}, (_, i) => ({ ts: Date.now()/1000 - i*120, agent: ["coder","tester","antigravity"][i%3], category: ["task","msg","sys"][i%3], text: "demo log entry #" + (8-i) })),
  memories: Array.from({length: 12}, (_, i) => ({ id: i, agent: "antigravity", category: ["decision","insight","incident"][i%3], importance: 3 + (i%3), content: "Demo memory record " + (12-i) + " \u2014 connect Supabase to see real data.", created_at: new Date(Date.now()-i*36e5).toISOString() })),
  chat: [
    { role: "user", content: "Refactor the auth module", created_at: new Date(Date.now()-6e5).toISOString(), images: [] },
    { role: "assistant", content: "I'll split auth into token + session helpers, then update callers. Demo transcript \u2014 launch the IDE bridge to see live chat.", created_at: new Date(Date.now()-5e5).toISOString(), images: [] },
  ],
  changes: { has_changes: true, title: "2 Files With Changes", files: [
    { added: "+24", removed: "-6", filename: "auth.ts", filepath: "src/auth/auth.ts" },
    { added: "+8", removed: "-0", filename: "session.ts", filepath: "src/auth/session.ts" },
  ] },
  files: ["src/auth/auth.ts","src/auth/session.ts","src/api/login.ts","scripts/dashboard2.js","README.md"],
  dispatch: { ok: true, prompt: "Add input validation", agents: 3, completed: 2, failed: 1, results: [
    { agent: "coder", ok: true, summary: "Added zod schema to login handler" },
    { agent: "tester", ok: true, summary: "Wrote 4 validation tests" },
    { agent: "researcher", ok: false, summary: "offline" },
  ] },
  webhooks: { coder: { url: "https://hooks.example/coder", events: ["message","task"], registered_at: new Date(Date.now()-72e5).toISOString() } },
};

// ---- helpers ----
const ago = (ts) => { const s = Math.floor(Date.now()/1000 - ts); if (s<60) return s+"s"; if (s<3600) return Math.floor(s/60)+"m"; if (s<86400) return Math.floor(s/3600)+"h"; return Math.floor(s/86400)+"d"; };
const useAsync = (fn, deps=[]) => {
  const [state, setState] = useState({ loading: true, data: null, demo: false });
  const reload = useCallback(() => {
    setState(s => ({ ...s, loading: true }));
    fn().then(d => setState({ loading: false, data: d, demo: false }))
        .catch(() => setState({ loading: false, data: null, demo: true }));
  }, deps);
  useEffect(() => { let alive = true;
    fn().then(d => alive && setState({ loading: false, data: d, demo: false }))
        .catch(() => alive && setState({ loading: false, data: null, demo: true }));
    return () => { alive = false; };
  }, deps);
  return { ...state, reload };
};

const Skel = ({ n=3 }) => html`<div class="list">${Array.from({length:n}).map(()=>html`<div class="skel" style="height:46px;margin:3px 0"></div>`)}</div>`;
const Empty = ({ ico="\u{1F4ED}", msg }) => html`<div class="empty"><div class="ico">${ico}</div>${msg}</div>`;
const Stat = ({ k, v, trend, ico }) => html`<div class="card stat fade"><h3>${ico} ${k}</h3><div class="v">${v}</div>${trend && html`<div class="trend">${trend}</div>`}</div>`;
const DemoTag = ({ on }) => on ? html`<span class="chip warn">demo</span>` : null;
const md = (s) => { try { return window.marked ? window.marked.parse(s||"") : (s||""); } catch { return s||""; } };

// ---- Views ----
function Overview({ go }) {
  const st = useAsync(() => api.get("/api/coord/state").catch(()=>DEMO.state));
  const tk = useAsync(() => api.get("/api/coord/tasks").catch(()=>DEMO.tasks));
  const lg = useAsync(() => api.get("/api/coord/log?limit=8").catch(()=>DEMO.log));
  const agents = Object.entries((st.data||DEMO.state).agents||{}).map(([id,a]) => ({ id, ...a }));
  const tasks = tk.data || DEMO.tasks;
  const active = agents.filter(a => !a.stale).length;
  const done = tasks.filter(t => t.status === "done").length;
  return html`<div class="fade">
    <div class="grid cols-4" style="margin-bottom:var(--gap)">
      <${Stat} ico="\u{1F916}" k="Agents online" v=${active + "/" + agents.length} trend=${active+" live"} />
      <${Stat} ico="\u{1F4CB}" k="Tasks" v=${tasks.length} trend=${done+" done"} />
      <${Stat} ico="\u26A1" k="Throughput" v=${(done/Math.max(tasks.length,1)*100|0)+"%"} />
      <${Stat} ico="\u{1F9E0}" k="Activity" v=${(lg.data||DEMO.log).length+"+"} trend="recent events" />
    </div>
    <div class="grid cols-2">
      <div class="card"><h3>\u{1F916} Agents <${DemoTag} on=${st.demo} /><a class="tag" href="#agents">manage \u2192</a></h3>
        ${st.loading ? html`<${Skel}/>` : html`<div class="list">${agents.map(a => html`
          <div class="row"><div class="av">${(a.role||"?")[0].toUpperCase()}</div>
            <div class="meta"><div class="t">${a.id}</div><div class="s">${a.role} \u00B7 ${a.model||"\u2014"}</div></div>
            <span class="chip ${a.stale?"":"ok"}">${a.stale?"idle":"live"}</span></div>`)}</div>`}
      </div>
      <div class="card"><h3>\u{1F4DC} Activity <${DemoTag} on=${lg.demo} /><a class="tag" href="#logs">all \u2192</a></h3>
        ${lg.loading ? html`<${Skel}/>` : html`<div class="list">${(lg.data||DEMO.log).map(e => html`
          <div class="row"><div class="av">${(e.category||"\u2022")[0].toUpperCase()}</div>
            <div class="meta"><div class="t">${e.text||e.message||"\u2014"}</div><div class="s">${e.agent||"system"}</div></div>
            <span class="when">${ago(e.ts||Date.now()/1000)}</span></div>`)}</div>`}
      </div>
    </div>
  </div>`;
}

function Agents() {
  const cat = useAsync(() => api.get("/api/agents/catalog").then(d=>d.agents).catch(()=>DEMO.catalog));
  const [busy, setBusy] = useState(null);
  const list = cat.data || DEMO.catalog;
  const power = async (a, on) => { setBusy(a.agent_id); try { await api.post("/api/agents/power", { agent_id: a.agent_id, on }); } catch {} setBusy(null); cat.reload(); };
  const toggle = async (a) => { setBusy(a.agent_id); try { await api.post("/api/agents/toggle", { agent_id: a.agent_id, enabled: !a.enabled, role: a.role }); } catch {} setBusy(null); cat.reload(); };
  return html`<div class="fade">
    <div class="grid cols-3">${cat.loading ? [0,1,2].map(()=>html`<div class="card"><div class="skel" style="height:150px"></div></div>`) :
      list.map(a => html`<div class="card">
        <h3>${a.icon||"\u26AA"} ${a.label} ${cat.demo?html`<${DemoTag} on=${true}/>`:null}<span class="tag">${a.role}</span></h3>
        <div class="row"><div class="av">${(a.role||"?")[0].toUpperCase()}</div>
          <div class="meta"><div class="t" style="font-family:var(--mono);font-size:12px">${a.agent_id}</div><div class="s">${a.detail||""}</div></div>
          <span class="chip ${a.installed?(a.live?"ok":(a.enabled?"warn":"")):"err"}">${!a.installed?"not installed":a.live?"live":a.enabled?"stale":"ready"}</span>
        </div>
        <div style="display:flex;gap:8px;margin-top:12px">
          <button class="btn sm primary" disabled=${!a.installed||busy===a.agent_id} onClick=${()=>power(a, !(a.enabled))}>
            ${busy===a.agent_id?html`<span class="spin">\u25CC</span>`:(a.enabled?"Power off":"Power on")}</button>
          <button class="btn sm" disabled=${busy===a.agent_id} onClick=${()=>toggle(a)}>${a.enabled?"Deregister":"Register"}</button>
        </div>
      </div>`)}</div>
  </div>`;
}

function Tasks() {
  const tk = useAsync(() => api.get("/api/coord/tasks").catch(()=>DEMO.tasks));
  const [draft, setDraft] = useState({ id: "", prompt: "", assigned_to: "" });
  const cols = { todo: "To do", in_progress: "In progress", done: "Done" };
  const tasks = tk.data || DEMO.tasks;
  const create = async () => { if (!draft.id||!draft.prompt) return;
    try { await api.post("/api/coord/task", { ...draft, status: "todo" }); } catch {}
    setDraft({ id:"", prompt:"", assigned_to:"" }); tk.reload(); };
  const move = async (t, status) => { try { await api.post("/api/coord/task/update", { id: t.id, status }); } catch {} tk.reload(); };
  return html`<div class="fade">
    <div class="card" style="margin-bottom:var(--gap)"><h3>\u2795 New task <${DemoTag} on=${tk.demo}/></h3>
      <div style="display:grid;grid-template-columns:1fr 2fr 1fr auto;gap:10px">
        <input class="input" placeholder="task id" value=${draft.id} onInput=${e=>setDraft({...draft,id:e.target.value})}/>
        <input class="input" placeholder="prompt / description" value=${draft.prompt} onInput=${e=>setDraft({...draft,prompt:e.target.value})}/>
        <input class="input" placeholder="assignee" value=${draft.assigned_to} onInput=${e=>setDraft({...draft,assigned_to:e.target.value})}/>
        <button class="btn primary" onClick=${create}>Create</button>
      </div></div>
    <div class="grid cols-3">${Object.entries(cols).map(([k,label]) => html`
      <div class="card"><h3>${label}<span class="tag">${tasks.filter(t=>(t.status||"todo")===k).length}</span></h3>
        ${tk.loading ? html`<${Skel} n=2/>` : html`<div class="list">${tasks.filter(t=>(t.status||"todo")===k).map(t=>html`
          <div class="row"><div class="meta"><div class="t">${t.id}</div><div class="s">${t.prompt||""}</div></div>
            ${t.assigned_to && html`<span class="chip brand">${t.assigned_to}</span>`}
            <div style="display:flex;gap:4px">
              ${k!=="todo" && html`<button class="btn sm" title="back" onClick=${()=>move(t, k==="done"?"in_progress":"todo")}>\u2039</button>`}
              ${k!=="done" && html`<button class="btn sm" title="forward" onClick=${()=>move(t, k==="todo"?"in_progress":"done")}>\u203A</button>`}
            </div></div>`)}
          ${tasks.filter(t=>(t.status||"todo")===k).length===0 && html`<${Empty} msg="none" />`}</div>`}
      </div>`)}</div>
  </div>`;
}

function Memory() {
  const [q, setQ] = useState("");
  const mem = useAsync(async () => {
    const cfg = await fetch("/data/supabase_config.json").then(r=>r.json()).catch(()=>null);
    if (!cfg?.supabase_url) throw new Error("no-config");
    const h = { apikey: cfg.supabase_key, Authorization: "Bearer " + cfg.supabase_key };
    return fetch(cfg.supabase_url + "/rest/v1/memories?select=id,agent,category,importance,content,created_at&order=created_at.desc&limit=300", { headers: h }).then(r=>r.json());
  });
  const rows = (mem.data || DEMO.memories).filter(m => !q || (m.content||"").toLowerCase().includes(q.toLowerCase()) || (m.category||"").toLowerCase().includes(q.toLowerCase()));
  return html`<div class="fade">
    <div class="card" style="margin-bottom:var(--gap)"><h3>\u{1F50D} Search memories <${DemoTag} on=${mem.demo} /></h3>
      <input class="input" placeholder="filter by content or category\u2026" value=${q} onInput=${e=>setQ(e.target.value)} /></div>
    <div class="card"><h3>Records<span class="tag">${rows.length}</span></h3>
      ${mem.loading ? html`<${Skel} n=5/>` : rows.length ? html`<div class="list">${rows.map(m=>html`
        <div class="row"><div class="av" title=${"importance "+m.importance}>${"\u2605"}</div>
          <div class="meta"><div class="t">${m.content}</div><div class="s">${m.agent} \u00B7 ${m.category} \u00B7 imp ${m.importance}</div></div>
          <span class="when">${m.created_at?m.created_at.slice(5,16).replace("T"," "):""}</span></div>`)}</div>`
        : html`<${Empty} ico="\u{1F9E0}" msg="No memories \u2014 connect Supabase in data/supabase_config.json" />`}
    </div></div>`;
}

function Logs() {
  const [feed, setFeed] = useState([]);
  const lg = useAsync(() => api.get("/api/coord/log?limit=60").catch(()=>DEMO.log));
  useEffect(() => { let es;
    try { es = new EventSource("/api/events");
      es.onmessage = (ev) => { try { const d = JSON.parse(ev.data); if (d && Object.keys(d).length) setFeed(f => [{ ts: Date.now()/1000, ...d }, ...f].slice(0,80)); } catch {} };
    } catch {}
    return () => es && es.close();
  }, []);
  const rows = [...feed, ...(lg.data || DEMO.log)];
  return html`<div class="card fade"><h3>\u{1F6F0}\uFE0F Live log <${DemoTag} on=${lg.demo} /><span class="tag">${rows.length}</span></h3>
    ${lg.loading ? html`<${Skel} n=8/>` : html`<div class="list">${rows.map((e,i)=>html`
      <div class="row" key=${i}><div class="av">${(e.category||"\u2022")[0].toUpperCase()}</div>
        <div class="meta"><div class="t">${e.text||e.message||JSON.stringify(e).slice(0,90)}</div><div class="s">${e.agent||"system"} \u00B7 ${e.category||"event"}</div></div>
        <span class="when">${ago(e.ts||Date.now()/1000)}</span></div>`)}</div>`}
  </div>`;
}

function Chat() {
  const [msgs, setMsgs] = useState(null);
  const [demo, setDemo] = useState(false);
  const [status, setStatus] = useState("checking");
  const [prompt, setPrompt] = useState("");
  const [sending, setSending] = useState(false);
  const [models, setModels] = useState({ current: null, models: [] });
  const [opts, setOpts] = useState({ has_options: false, options: [] });
  const [changes, setChanges] = useState({ has_changes: false, files: [] });
  const [mention, setMention] = useState({ open: false, items: [], idx: 0 });
  const inputRef = useRef(null);
  const endRef = useRef(null);
  const loadHistory = useCallback(async () => {
    try { const d = await api.get("/api/ide/chat"); setMsgs(d.messages||[]); setDemo(false); }
    catch { setMsgs(DEMO.chat); setDemo(true); }
  }, []);
  const poll = useCallback(async () => {
    try { const st = await api.get("/api/ide/status"); setStatus(st.ok?"connected":"offline"); } catch { setStatus("offline"); }
    try { const o = await api.get("/api/ide/options"); setOpts(o); } catch {}
    try { const c = await api.get("/api/ide/composer_changes"); setChanges(c.has_changes?c:DEMO.changes); } catch { setChanges(DEMO.changes); }
    loadHistory();
  }, []);
  useEffect(() => { poll(); const t = setInterval(poll, 5000); return () => clearInterval(t); }, []);
  useEffect(() => { api.get("/api/ide/models").then(setModels).catch(()=>{}); }, []);
  useEffect(() => { endRef.current && endRef.current.scrollIntoView({ behavior: "smooth" }); }, [msgs]);
  const send = async () => { if (!prompt.trim()) return; setSending(true);
    try { await api.post("/api/ide/chat", { prompt }); setPrompt(""); setTimeout(loadHistory, 800); } catch {} setSending(false); };
  const act = async (action) => { try { await api.post("/api/ide/action", { action }); } catch {} setTimeout(loadHistory, 400); };
  const autopilot = async () => { try { await api.post("/api/ide/autopilot", { allow: true, accept: true }); } catch {} };
  const pick = async (idx) => { try { await api.post("/api/ide/select_option", { index: idx, submit: true }); } catch {} setTimeout(poll, 600); };
  const switchModel = async (m) => { try { await api.post("/api/ide/model", { model: m }); setModels({...models, current: m}); } catch {} };
  const launch = async () => { try { await api.post("/api/ide/launch", { force: false }); } catch {} setTimeout(poll, 1500); };
  const composer = async (action) => { try { await api.post("/api/ide/composer_action", { action }); } catch {} setTimeout(poll, 600); };
  // @-mention file search
  const onInput = async (e) => {
    const v = e.target.value; setPrompt(v);
    const m = v.match(/@([\w./\\-]*)$/);
    if (m) { try { const d = await api.get("/api/ide/files?q=" + encodeURIComponent(m[1])); setMention({ open: true, items: (d.files||DEMO.files).slice(0,8), idx: 0 }); }
             catch { setMention({ open: true, items: DEMO.files.filter(f=>f.includes(m[1])).slice(0,8), idx: 0 }); } }
    else setMention({ open: false, items: [], idx: 0 });
  };
  const applyMention = (f) => { setPrompt(prompt.replace(/@([\w./\\-]*)$/, "@" + f + " ")); setMention({ open: false, items: [], idx: 0 }); inputRef.current && inputRef.current.focus(); };
  const onKey = (e) => {
    if (mention.open && mention.items.length) {
      if (e.key === "ArrowDown") { e.preventDefault(); setMention({ ...mention, idx: (mention.idx+1)%mention.items.length }); return; }
      if (e.key === "ArrowUp") { e.preventDefault(); setMention({ ...mention, idx: (mention.idx-1+mention.items.length)%mention.items.length }); return; }
      if (e.key === "Enter" || e.key === "Tab") { e.preventDefault(); applyMention(mention.items[mention.idx]); return; }
      if (e.key === "Escape") { setMention({ open: false, items: [], idx: 0 }); return; }
    }
    if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); }
  };
  const list = msgs || DEMO.chat;
  return html`<div class="fade" style="display:flex;flex-direction:column;height:calc(100vh - 130px)">
    <div class="card" style="margin-bottom:12px;display:flex;align-items:center;gap:12px;flex:none;flex-wrap:wrap">
      <span class="pill"><span class="dot ${status==="connected"?"ok":status==="offline"?"err":"warn"}"></span>${status==="connected"?"IDE bridge":status==="offline"?"bridge offline":"\u2026"}</span>
      ${demo && html`<${DemoTag} on=${true}/>`}
      ${models.models?.length ? html`<select class="input" style="width:auto" value=${models.current||""} onChange=${e=>switchModel(e.target.value)}>
        ${(models.current && !models.models.includes(models.current))?html`<option>${models.current}</option>`:null}
        ${models.models.map(m=>html`<option value=${m}>${m}</option>`)}</select>` : null}
      <div class="spacer" style="flex:1"></div>
      <button class="btn sm" onClick=${autopilot} title="auto-allow + accept">\u{1F6E9}\uFE0F Autopilot</button>
      <button class="btn sm" onClick=${()=>act("stop")}>\u23F9 Stop</button>
      <button class="btn sm" onClick=${()=>act("clear")}>\u{1F9F9} Clear</button>
      ${status==="offline" && html`<button class="btn sm primary" onClick=${launch}>Launch IDE</button>`}
    </div>
    <div style="display:flex;gap:12px;flex:1;min-height:0">
      <div class="card" style="flex:1;overflow-y:auto;min-height:0">
        ${list.length===0 ? html`<${Empty} ico="\u{1F4AC}" msg="No messages yet" /> ` :
          list.map((m,i)=>html`<div key=${i} style="margin-bottom:16px;display:flex;gap:10px">
            <div class="av" style="flex:none">${m.role==="user"?"\u{1F464}":"\u{1F9E0}"}</div>
            <div style="min-width:0;flex:1">
              <div class="s" style="font-size:11px;color:var(--text-mute);margin-bottom:3px">${m.role==="user"?"you":"antigravity"} \u00B7 ${m.created_at?new Date(m.created_at).toLocaleTimeString():""}</div>
              <div class="bubble" style="background:var(--surface-2);border:1px solid var(--border);border-radius:10px;padding:10px 14px;font-size:14px;line-height:1.55" dangerouslySetInnerHTML=${{ __html: md(m.content) }}></div>
              ${(m.images||[]).map(img=>html`<img src=${"/api/ide/localfile?path="+encodeURIComponent(img)} style="max-width:280px;border-radius:8px;margin-top:8px;border:1px solid var(--border)"/>`)}
            </div></div>`)}
        <div ref=${endRef}></div>
      </div>
      ${changes.has_changes && html`<div class="card" style="width:300px;flex:none;overflow-y:auto;min-height:0">
        <h3>\u{1F4DD} ${changes.title||"Changes"}</h3>
        <div class="list">${(changes.files||[]).map((f,i)=>html`
          <div class="row" key=${i}><div class="meta"><div class="t" style="font-size:13px">${f.filename}</div><div class="s" style="font-family:var(--mono);font-size:11px">${f.filepath}</div></div>
            <span class="chip ok">${f.added}</span><span class="chip err">${f.removed}</span></div>`)}</div>
        <div style="display:flex;gap:8px;margin-top:12px">
          <button class="btn sm primary" style="flex:1" onClick=${()=>composer("accept")}>Accept all</button>
          <button class="btn sm" style="flex:1" onClick=${()=>composer("reject")}>Reject all</button>
        </div></div>`}
    </div>
    ${opts.has_options && html`<div class="card" style="margin-top:12px;flex:none"><h3>\u2753 Choose an option</h3>
      <div style="display:flex;flex-wrap:wrap;gap:8px">${(opts.options||[]).map(o=>html`
        <button class="btn ${o.checked?"primary":""}" onClick=${()=>pick(o.idx)}>${o.text||("option "+(o.idx+1))}</button>`)}</div></div>`}
    <div style="position:relative;margin-top:12px;flex:none">
      ${mention.open && mention.items.length ? html`<div class="card" style="position:absolute;bottom:100%;left:0;right:90px;margin-bottom:6px;max-height:220px;overflow-y:auto;padding:6px;z-index:5">
        ${mention.items.map((f,i)=>html`<div class="row ${i===mention.idx?"":""}" key=${i} style=${"cursor:pointer;"+(i===mention.idx?"background:var(--surface-2)":"")} onMouseDown=${(e)=>{e.preventDefault();applyMention(f);}}>
          <div class="av">\u{1F4C4}</div><div class="meta"><div class="t" style="font-family:var(--mono);font-size:12px">${f}</div></div></div>`)}
      </div>` : null}
      <div style="display:flex;gap:10px">
        <input class="input" ref=${inputRef} placeholder="Message Antigravity\u2026 (@file to mention)" value=${prompt} onInput=${onInput} onKeyDown=${onKey}/>
        <button class="btn primary" disabled=${sending||!prompt.trim()} onClick=${send}>${sending?html`<span class="spin">\u25CC</span>`:"Send"}</button>
      </div>
    </div>
  </div>`;
}

function Orchestrate() {
  const [prompt, setPrompt] = useState("");
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState(null);
  const [demo, setDemo] = useState(false);
  const dispatch = async () => { if (!prompt.trim()) return; setRunning(true);
    try { const r = await api.post("/api/orchestrator/dispatch", { prompt }); setResult(r); setDemo(false); }
    catch { setResult(DEMO.dispatch); setDemo(true); } setRunning(false); };
  const r = result;
  return html`<div class="fade">
    <div class="card" style="margin-bottom:var(--gap)"><h3>\u{1F39B}\uFE0F Dispatch to all connected agents <${DemoTag} on=${demo}/></h3>
      <div style="display:flex;gap:10px">
        <input class="input" placeholder="Task prompt to broadcast\u2026" value=${prompt}
          onInput=${e=>setPrompt(e.target.value)} onKeyDown=${e=>{ if(e.key==="Enter"){e.preventDefault();dispatch();} }}/>
        <button class="btn primary" disabled=${running||!prompt.trim()} onClick=${dispatch}>${running?html`<span class="spin">\u25CC</span> Running`:"Dispatch"}</button>
      </div>
      <div class="s" style="margin-top:8px;color:var(--text-mute)">Runs synapz-orchestrator --live and fans the prompt out to every connected agent.</div>
    </div>
    ${r ? html`<div class="grid cols-4" style="margin-bottom:var(--gap)">
        <${Stat} ico="\u{1F916}" k="Agents" v=${r.agents??0} />
        <${Stat} ico="\u2705" k="Completed" v=${r.completed??0} />
        <${Stat} ico="\u274C" k="Failed" v=${r.failed??0} />
        <${Stat} ico="\u26A1" k="Status" v=${r.ok?"ok":"err"} />
      </div>
      <div class="card"><h3>Results<span class="tag">${(r.results||[]).length}</span></h3>
        ${(r.results||[]).length ? html`<div class="list">${r.results.map((x,i)=>html`
          <div class="row" key=${i}><div class="av">${x.agent?.[0]?.toUpperCase()||"?"}</div>
            <div class="meta"><div class="t">${x.summary||x.result||"\u2014"}</div><div class="s">${x.agent||""}</div></div>
            <span class="chip ${x.ok?"ok":"err"}">${x.ok?"done":"failed"}</span></div>`)}</div>`
          : html`<${Empty} msg=${r.reason||"no results"} />`}
      </div>` : html`<${Empty} ico="\u{1F39B}\uFE0F" msg="Dispatch a task to see per-agent results" />`}
  </div>`;
}

function Webhooks() {
  const wh = useAsync(() => api.get("/api/coord/webhooks").catch(()=>DEMO.webhooks));
  const [draft, setDraft] = useState({ agent_id: "", url: "" });
  const rows = Object.entries(wh.data || DEMO.webhooks).map(([id, w]) => ({ id, ...w }));
  const register = async () => { if (!draft.agent_id||!draft.url) return;
    try { await api.post("/api/coord/webhook/register", { agent_id: draft.agent_id, url: draft.url }); } catch {}
    setDraft({ agent_id:"", url:"" }); wh.reload(); };
  const remove = async (id) => { try { await api.post("/api/coord/webhook/unregister", { agent_id: id }); } catch {} wh.reload(); };
  const trigger = async (id) => { try { await api.post("/api/coord/webhook/trigger", { event: "ping", agent_id: id }); } catch {} };
  return html`<div class="fade">
    <div class="card" style="margin-bottom:var(--gap)"><h3>\u2795 Register webhook <${DemoTag} on=${wh.demo}/></h3>
      <div style="display:grid;grid-template-columns:1fr 2fr auto;gap:10px">
        <input class="input" placeholder="agent id" value=${draft.agent_id} onInput=${e=>setDraft({...draft,agent_id:e.target.value})}/>
        <input class="input" placeholder="https://your-endpoint/hook" value=${draft.url} onInput=${e=>setDraft({...draft,url:e.target.value})}/>
        <button class="btn primary" onClick=${register}>Register</button>
      </div></div>
    <div class="card"><h3>Registered<span class="tag">${rows.length}</span></h3>
      ${wh.loading ? html`<${Skel} n=2/>` : rows.length ? html`<div class="list">${rows.map(w=>html`
        <div class="row" key=${w.id}><div class="av">\u{1F517}</div>
          <div class="meta"><div class="t" style="font-family:var(--mono);font-size:12px">${w.id}</div><div class="s">${w.url}</div></div>
          ${(w.events||[]).map(ev=>html`<span class="chip brand">${ev}</span>`)}
          <button class="btn sm" onClick=${()=>trigger(w.id)}>Test</button>
          <button class="btn sm" onClick=${()=>remove(w.id)}>\u2715</button></div>`)}</div>`
        : html`<${Empty} ico="\u{1F517}" msg="No webhooks registered" />`}
    </div></div>`;
}

function Constellation() {
  const st = useAsync(() => api.get("/api/coord/state").catch(()=>DEMO.state));
  const agents = Object.entries((st.data||DEMO.state).agents||{}).map(([id,a]) => ({ id, ...a }));
  const W = 760, H = 460, cx = W/2, cy = H/2;
  const roots = agents.filter(a => !a.parent_id);
  const kids = agents.filter(a => a.parent_id);
  const center = roots[0] || { id: "orchestrator", role: "orchestrator", stale: false };
  const others = agents.filter(a => a.id !== center.id);
  const pos = {}; pos[center.id] = { x: cx, y: cy };
  others.forEach((a, i) => { const ang = (i / Math.max(others.length,1)) * Math.PI * 2 - Math.PI/2; pos[a.id] = { x: cx + Math.cos(ang)*170, y: cy + Math.sin(ang)*150 }; });
  const color = (a) => a.stale ? "var(--text-mute)" : a.role==="orchestrator" ? "var(--brand)" : "var(--brand-2)";
  return html`<div class="card fade"><h3>\u{1F30C} Agent constellation <${DemoTag} on=${st.demo}/><span class="tag">${agents.length} nodes</span></h3>
    ${st.loading ? html`<div class="skel" style="height:460px"></div>` : html`
    <svg viewBox=${"0 0 "+W+" "+H} style="width:100%;height:auto">
      ${others.map(a => { const p = pos[a.id]; const parent = pos[a.parent_id] || pos[center.id]; return html`
        <line x1=${parent.x} y1=${parent.y} x2=${p.x} y2=${p.y} stroke="var(--border-2)" stroke-width="1.5" stroke-dasharray=${a.stale?"4 4":"0"} />`; })}
      ${agents.map(a => { const p = pos[a.id]; const r = a.id===center.id?34:24; return html`<g>
        <circle cx=${p.x} cy=${p.y} r=${r+6} fill=${color(a)} opacity="0.12"/>
        <circle cx=${p.x} cy=${p.y} r=${r} fill="var(--bg-1)" stroke=${color(a)} stroke-width="2"/>
        <text x=${p.x} y=${p.y+1} text-anchor="middle" dominant-baseline="middle" font-size=${a.id===center.id?18:15}>${(a.role||"?")[0].toUpperCase()}</text>
        <text x=${p.x} y=${p.y+r+15} text-anchor="middle" font-size="11" fill="var(--text-dim)">${a.id}</text>
        ${!a.stale && html`<circle cx=${p.x+r-3} cy=${p.y-r+3} r="4" fill="var(--ok)"/>`}
      </g>`; })}
    </svg>
    <div style="display:flex;gap:16px;justify-content:center;margin-top:8px;font-size:12px;color:var(--text-mute)">
      <span><span class="dot ok"></span> live</span><span><span class="dot"></span> stale</span><span>\u2014 parent link</span>
    </div>`}
  </div>`;
}

const VIEWS = {
  overview:      { label: "Overview", ico: "\u{1F3E0}", sub: "System at a glance", comp: Overview },
  agents:        { label: "Agents", ico: "\u{1F916}", sub: "Roster & power controls", comp: Agents },
  tasks:         { label: "Tasks", ico: "\u{1F4CB}", sub: "Coordination board", comp: Tasks },
  chat:          { label: "Chat", ico: "\u{1F4AC}", sub: "Antigravity IDE bridge", comp: Chat },
  orchestrate:   { label: "Orchestrate", ico: "\u{1F39B}\uFE0F", sub: "Dispatch to all agents", comp: Orchestrate },
  constellation: { label: "Constellation", ico: "\u{1F30C}", sub: "Agent graph", comp: Constellation },
  memory:        { label: "Memory", ico: "\u{1F9E0}", sub: "Long-term recall", comp: Memory },
  webhooks:      { label: "Webhooks", ico: "\u{1F517}", sub: "Event subscriptions", comp: Webhooks },
  logs:          { label: "Logs", ico: "\u{1F6F0}\uFE0F", sub: "Live activity stream", comp: Logs },
};

function App() {
  const [view, setView] = useState(location.hash.slice(1) || "overview");
  const [collapsed, setCollapsed] = useState(false);
  const [theme, setTheme] = useState(localStorage.getItem("synapz_theme") || "dark");
  const [health, setHealth] = useState("checking");
  useEffect(() => { document.documentElement.dataset.theme = theme; localStorage.setItem("synapz_theme", theme); }, [theme]);
  useEffect(() => { const go = () => setView(location.hash.slice(1) || "overview"); addEventListener("hashchange", go); return () => removeEventListener("hashchange", go); }, []);
  useEffect(() => { let t; const ping = () => api.get("/api/coord/state").then(()=>setHealth("ok")).catch(()=>setHealth("offline")); ping(); t = setInterval(ping, 8000); return () => clearInterval(t); }, []);
  const V = VIEWS[view] || VIEWS.overview;
  const groups = [
    { sec: "Workspace", keys: ["overview","agents","tasks"] },
    { sec: "Live", keys: ["chat","orchestrate","constellation","logs"] },
    { sec: "Knowledge", keys: ["memory","webhooks"] },
  ];
  return html`<div class="app ${collapsed?"collapsed":""}">
    <aside class="sidebar">
      <div class="brand"><div class="logo">\u{1F9E0}</div><div class="name">SynapzCore<small>command center</small></div></div>
      <nav class="nav">${groups.map(g => html`<${Fragment}>
        <div class="nav-sec">${g.sec}</div>
        ${g.keys.map(k => html`<a class="nav-item ${view===k?"active":""}" href="#${k}">
          <span class="ico">${VIEWS[k].ico}</span><span class="label">${VIEWS[k].label}</span></a>`)}
      <//>`)}</nav>
      <div class="side-foot">
        <button onClick=${()=>setCollapsed(c=>!c)} title="collapse">${collapsed?"\u00BB":"\u00AB"}</button>
        <button onClick=${()=>setTheme(t=>t==="dark"?"light":"dark")} title="theme">${theme==="dark"?"\u2600\uFE0F":"\u{1F319}"}</button>
      </div>
    </aside>
    <main class="main">
      <header class="topbar">
        <button class="burger" onClick=${()=>setCollapsed(c=>!c)}>\u2630</button>
        <h1>${V.label}<small>${V.sub}</small></h1>
        <div class="spacer"></div>
        <span class="pill"><span class="dot ${health==="ok"?"ok":health==="offline"?"err":"warn"}"></span>${health==="ok"?"connected":health==="offline"?"demo mode":"\u2026"}</span>
      </header>
      <section class="content"><${V.comp} key=${view} go=${(v)=>location.hash=v} /></section>
    </main>
  </div>`;
}
render(html`<${App} />`, document.getElementById("root"));
