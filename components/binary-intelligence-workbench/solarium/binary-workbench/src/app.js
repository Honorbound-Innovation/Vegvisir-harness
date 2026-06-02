const API = localStorage.getItem('BIW_API') || 'http://127.0.0.1:8765';
let current = null;
let tab = 'summary';

async function get(path) {
  const res = await fetch(API + path);
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return await res.json();
}

function text(value) {
  return typeof value === 'string' ? value : JSON.stringify(value, null, 2);
}

function renderDetail() {
  const el = document.getElementById('content');
  if (!current) return;
  const reports = current.reports || {};
  if (tab === 'summary') el.textContent = reports.summary_md || JSON.stringify(current.summary, null, 2);
  else if (tab === 'findings') el.textContent = text(current.findings || []);
  else if (tab === 'skills') el.textContent = text(current.skill_outputs || []);
  else if (tab === 'functions') el.textContent = text(current.function_reports || []);
  else if (tab === 'artifacts') el.textContent = text(current.artifacts || current.summary?.artifact_counts || {});
  else if (tab === 'notes') el.textContent = reports.notes_md || 'No notes loaded.';
}

async function selectCase(id) {
  current = await get(`/api/cases/${encodeURIComponent(id)}?include_artifacts=false`);
  document.getElementById('title').textContent = current.summary?.title || id;
  document.getElementById('meta').textContent = `${id} · risk ${current.summary?.risk?.level || 'unknown'} · ${current.summary?.source?.format || 'unknown'}`;
  renderDetail();
}

async function refresh() {
  try {
    const health = await get('/health');
    document.getElementById('health').textContent = health.ok ? 'API: healthy' : 'API: not healthy';
    const index = await get('/api/index');
    const list = document.getElementById('cases');
    list.innerHTML = '';
    for (const c of index.cases || []) {
      const li = document.createElement('li');
      const b = document.createElement('button');
      b.textContent = `${c.case_id} (${c.risk?.level || 'unknown'})`;
      b.onclick = () => selectCase(c.case_id);
      li.appendChild(b);
      list.appendChild(li);
    }
  } catch (err) {
    document.getElementById('health').textContent = `API error: ${err.message}`;
  }
}

document.getElementById('refresh').onclick = refresh;
document.getElementById('tabs').addEventListener('click', ev => {
  if (ev.target.dataset?.tab) { tab = ev.target.dataset.tab; renderDetail(); }
});
refresh();
