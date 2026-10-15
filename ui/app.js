"use strict";

const T = window.__TAURI__;
const invoke = T.core.invoke;

const S = {
  view: null,
  tab: "today",
  session: null,        // session chosen on the Today tab (null = follow schedule)
  logDate: isoToday(),
  progressPick: null,
  historyFilter: "",
  busy: false,
};

// ------------------------------------------------------------------ helpers

function isoToday() {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
const num = (n) => (n == null ? "" : String(Math.round(n * 100) / 100));
const cap = (s) => (s ? s[0].toUpperCase() + s.slice(1) : s);
function fmtDate(iso, opts = { weekday: "short", day: "numeric", month: "short" }) {
  const [y, m, d] = iso.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString("en-GB", opts);
}
const VERDICT = { increase: "Increase", repeat: "Repeat", reduce: "Reduce / review", new: "New" };
const pill = (v, label) => `<span class="pill ${v}">${esc(label ?? VERDICT[v] ?? v)}</span>`;

function toast(msg, ms = 3200) {
  const t = $("#toast");
  t.textContent = msg;
  t.hidden = false;
  clearTimeout(toast.t);
  toast.t = setTimeout(() => (t.hidden = true), ms);
}

function openUrl(url) {
  if (url) T.opener.openUrl(url).catch((e) => toast(String(e)));
}

// "16kg total — 1x5kg + 1x2.5kg each side (6 plates)" -> ["16kg total", "1x5kg + … "]
function splitLoad(text, loadType) {
  if (!text) return ["—", ""];
  if (text === "BW") return ["Bodyweight", loadType === "Bodyweight" ? "No load — progress by reps or seconds" : ""];
  const i = text.indexOf(" — ");
  return i < 0 ? [text, ""] : [text.slice(0, i), text.slice(i + 3)];
}

// The sheet's decision text is written for the log ("next time…"); on the
// Today screen we only need the coaching part of it.
function coachLine(c) {
  const bw = c.load_type === "Bodyweight";
  const range = `${num(c.rep_low)}–${num(c.rep_high)}`;
  const t = c.instruction || "";
  switch (c.verdict) {
    case "new":
      return `First session — aim for ${range} ${bw ? "reps or seconds" : "reps"} per set.`;
    case "increase":
      return bw ? "You topped the range on every set. Add a few reps or seconds, or move to a harder variation."
                : `You topped the range on every set last time, so step up to this load. Aim for ${range} reps.`;
    case "reduce":
      return cap(t.split("Reduce/Review — ")[1] || t);
    default: {
      const parts = t.split(" — ");
      return cap(parts[parts.length - 1]);
    }
  }
}

// ------------------------------------------------------------------ drafts
// Reps typed into the Today screen survive closing the app (per date + session).

const draftKey = () => `draft|${S.logDate}|${currentSession()}`;
function readDraft() {
  try { return JSON.parse(localStorage.getItem(draftKey())) || {}; } catch { return {}; }
}
function writeDraft(d) {
  try { localStorage.setItem(draftKey(), JSON.stringify(d)); } catch {}
}
function clearDraft() {
  try { localStorage.removeItem(draftKey()); } catch {}
}

// ------------------------------------------------------------------ data

async function load(refresh = false) {
  setBusy(true);
  try {
    S.view = await invoke("load", { refresh });
  } catch (e) {
    toast(String(e));
  }
  setBusy(false);
  render();
}

function setBusy(b) {
  S.busy = b;
  $("#refresh").classList.toggle("spin", b);
}

function currentSession() {
  const v = S.view;
  if (S.session) return S.session;
  if (v?.today && !v.today.is_rest) return v.today.session;
  return v?.sessions?.[0]?.name ?? "";
}

// ------------------------------------------------------------------ render

function render() {
  $$(".tab").forEach((b) => b.classList.toggle("active", b.dataset.view === S.tab));
  renderStatus();
  const v = S.view;
  const el = $("#view");
  if (!v) return (el.innerHTML = `<p class="muted">Loading…</p>`);
  if (!v.today && S.tab !== "settings") {
    el.innerHTML = setupNeeded();
    $("#go-settings")?.addEventListener("click", () => switchTab("settings"));
    return;
  }
  ({ today: renderToday, history: renderHistory, progress: renderProgress, programme: renderProgramme, settings: renderSettings })[S.tab](el);
}

function renderStatus() {
  const st = S.view?.status;
  const dot = $("#sync-dot");
  const txt = $("#sync-text");
  const notice = $("#notice");
  if (!st) return;
  dot.className = "dot " + (st.online ? "on" : "off");
  let t = st.online ? `Synced ${st.last_sync}` : st.mode === "snapshot" ? "Snapshot (offline)" : "Offline";
  if (st.pending) t += ` · ${st.pending} queued`;
  txt.textContent = t;
  const msgs = [];
  if (st.message) msgs.push(esc(st.message));
  if (st.rows_left != null && st.rows_left < 40)
    msgs.push(`The Workout Log has room for ${st.rows_left} more rows before its formulas run out (row 384). Extend the sheet's formulas soon.`);
  notice.hidden = !msgs.length || S.tab === "settings";
  notice.className = "notice" + (st.online ? " info" : "");
  notice.innerHTML = msgs.join("<br>");
}

function setupNeeded() {
  return `<h1>Connect to your sheet</h1>
    <p class="sub">${esc(S.view?.status?.message || "")}</p>
    <button class="btn primary" id="go-settings">Open settings</button>`;
}

// ---------------------------------------------------------------- Today

function renderToday(el) {
  const v = S.view;
  const td = v.today;
  const session = currentSession();
  const sv = v.sessions.find((s) => s.name === session) || { cards: [], skipped: [] };
  const draft = readDraft();

  const head = `
    <h1>${esc(fmtDate(td.date, { weekday: "long", day: "numeric", month: "long" }))}</h1>
    <p class="sub">${td.is_rest
      ? `Rest day${td.notes ? " · " + esc(td.notes) : ""} · next training day is ${esc(td.next_training_day)}`
      : `Scheduled: <b>${esc(td.session)}</b>${td.notes ? " · " + esc(td.notes) : ""}`}</p>`;

  const chips = `<div class="chips" role="tablist">${v.sessions
    .map((s) => `<button class="chip ${s.name === session ? "active" : ""}" data-session="${esc(s.name)}" role="tab">${esc(s.name)}${
      !td.is_rest && s.name === td.session ? '<span class="today-dot" title="Scheduled today"></span>' : ""}</button>`)
    .join("")}</div>`;

  const off = !td.is_rest && session !== td.session
    ? `<p class="small muted">You're looking at a different session from your schedule. <a href="#" id="back-to-schedule">Back to ${esc(td.session)}</a></p>` : "";
  const rest = td.is_rest && !S.session
    ? `<div class="rest"><h3>Rest day</h3><p class="muted" style="margin:0">Nothing scheduled. Pick a session above if you're training anyway.</p></div>` : "";

  const toolbar = `<div class="toolbar">
      <label class="field">Log date <input type="date" id="log-date" value="${S.logDate}"></label>
      ${sv.skipped.length ? `<span class="small muted">Skipped (kit switched off): ${esc(sv.skipped.join(", "))}</span>` : ""}
    </div>`;

  const cards = sv.cards.map((c, i) => card(c, i, draft[c.exercise])).join("");

  el.innerHTML = head + chips + off + rest + toolbar + `<div class="cards">${cards}</div>` + saveBar();

  $$(".chip", el).forEach((b) => b.addEventListener("click", () => { S.session = b.dataset.session; render(); }));
  $("#back-to-schedule")?.addEventListener("click", (e) => { e.preventDefault(); S.session = null; render(); });
  $("#log-date").addEventListener("change", (e) => { S.logDate = e.target.value || isoToday(); render(); });
  $$("a[data-url]", el).forEach((a) => a.addEventListener("click", (e) => { e.preventDefault(); openUrl(a.dataset.url); }));
  $$(".card input", el).forEach((inp) => inp.addEventListener("input", onEntryInput));
  $$(".copy-btn", el).forEach((b) => b.addEventListener("click", () => fillSets(b.closest(".card"))));
  $("#save").addEventListener("click", saveSession);
  updateSaveBar();
}

function card(c, i, d) {
  const bw = c.load_type === "Bodyweight";
  const [main, detail] = splitLoad(c.recommended, c.load_type);
  const n = Math.min(c.sets, 4);
  const sets = d?.sets || [];
  const weight = d?.weight ?? (bw ? 0 : c.recommended_kg);
  const done = sets.some((x) => x !== "" && x != null);
  const title = c.video
    ? `<a href="#" data-url="${esc(c.video)}" title="Watch how-to video">${esc(c.exercise)}<span class="play">▶ video</span></a>`
    : esc(c.exercise);
  const last = c.previous_date
    ? `Last time (${esc(fmtDate(c.previous_date))}): <b>${c.previous_weight != null ? num(c.previous_weight) + "kg" : "BW"}</b> · ${esc(c.previous_reps)} · ${esc(c.trend)}`
    : "No sessions logged yet";
  const unit = bw ? "" : c.load_type === "Fixed steps" ? "kg equiv" : c.load_type === "Dumbbell plates" ? "kg each" : "kg";
  return `<article class="card ${done ? "done" : ""}" data-i="${i}" data-exercise="${esc(c.exercise)}" data-sets="${n}">
    <div class="card-head">
      <div class="card-title">${title}
        <div class="meta">${esc(c.equipment)} · ${c.sets} × ${num(c.rep_low)}–${num(c.rep_high)}${c.substituted ? ` · <span title="Main kit is switched off">instead of ${esc(c.original)}</span>` : ""}</div>
      </div>
      ${pill(c.verdict)}
    </div>
    <div class="load">
      <div><div class="sublabel">Load today</div><div class="load-main">${esc(main)}</div><div class="load-detail">${esc(detail)}</div></div>
      <div><div class="sublabel">History</div><div class="last">${last}</div></div>
    </div>
    <div class="instr" title="${esc(c.instruction)}">${esc(coachLine(c))}</div>
    <div class="entry">
      ${bw ? "" : `<label>Weight<input class="wt" type="number" step="0.5" min="0" inputmode="decimal" data-k="weight" value="${esc(num(weight))}"></label><span class="unit">${unit}</span><span class="gap"></span>`}
      ${Array.from({ length: n }, (_, k) => `<label>Set ${k + 1}<input type="number" min="0" step="1" inputmode="numeric" data-k="set" data-s="${k}" placeholder="" value="${esc(sets[k] ?? "")}"></label>`).join("")}
      <button class="copy-btn" type="button" title="Fill every set with the top of the range">All ${num(c.rep_high)}</button>
    </div>
  </article>`;
}

function readCard(cardEl) {
  const w = $('input[data-k="weight"]', cardEl);
  return {
    weight: w ? w.value : 0,
    sets: $$('input[data-k="set"]', cardEl).map((i) => i.value),
  };
}

function onEntryInput(e) {
  const cardEl = e.target.closest(".card");
  const d = readDraft();
  d[cardEl.dataset.exercise] = readCard(cardEl);
  writeDraft(d);
  cardEl.classList.toggle("done", d[cardEl.dataset.exercise].sets.some((x) => x !== ""));
  updateSaveBar();
}

function fillSets(cardEl) {
  const c = currentCards()[cardEl.dataset.i];
  $$('input[data-k="set"]', cardEl).forEach((i) => (i.value = num(c.rep_high)));
  onEntryInput({ target: cardEl.querySelector("input") });
}

function currentCards() {
  return (S.view.sessions.find((s) => s.name === currentSession()) || { cards: [] }).cards;
}

function saveBar() {
  return `<div class="savebar"><div class="savebar-inner">
    <span class="count" id="save-count"></span>
    <button class="btn primary" id="save" disabled>Save</button></div></div>`;
}

function updateSaveBar() {
  const done = $$(".card.done").length;
  const total = $$(".card").length;
  const snap = S.view.status.mode === "snapshot";
  $("#save-count").textContent = total ? `${done} of ${total} exercises filled in` : "No exercises in this session";
  const b = $("#save");
  b.disabled = !done || S.busy;
  b.textContent = snap ? `Queue ${done || ""} (snapshot mode)` : `Save ${done || ""} to sheet`;
}

async function saveSession() {
  const entries = $$(".card.done").map((cardEl) => {
    const r = readCard(cardEl);
    return {
      date: S.logDate,
      workout: currentSession(),
      exercise: cardEl.dataset.exercise,
      weight: Number(r.weight) || 0,
      sets: r.sets.map((x) => (x === "" ? null : Number(x))),
    };
  });
  if (!entries.length) return;
  setBusy(true);
  $("#save").disabled = true;
  try {
    S.view = await invoke("save_entries", { entries });
    const st = S.view.status;
    if (st.pending) toast(`${entries.length} saved on this computer. They'll go to the sheet next time it's reachable.`, 5000);
    else toast(`${entries.length} exercise${entries.length > 1 ? "s" : ""} saved to the sheet.`);
    clearDraft();
  } catch (e) {
    toast(String(e), 6000);
  }
  setBusy(false);
  render();
}

// ---------------------------------------------------------------- History

function renderHistory(el) {
  const v = S.view;
  const names = [...new Set(v.history.map((h) => h.exercise))].sort();
  const rows = v.history.filter((h) => !S.historyFilter || h.exercise === S.historyFilter);
  let html = `<h1>History</h1><p class="sub">Every logged exercise, newest first. ${rows.length} entries.</p>
    <div class="toolbar"><label class="field">Exercise
      <select id="hist-filter"><option value="">All exercises</option>${names.map((n) => `<option ${n === S.historyFilter ? "selected" : ""}>${esc(n)}</option>`).join("")}</select></label></div>
    <div class="table-wrap"><table><thead><tr><th>Exercise</th><th class="num">Weight</th><th>Reps</th><th class="num">Total</th><th class="num">Volume</th><th>Outcome</th></tr></thead><tbody>`;
  let group = "";
  for (const h of rows) {
    const g = `${h.date}|${h.workout}`;
    if (g !== group && !S.historyFilter) {
      group = g;
      html += `<tr class="group"><td colspan="6">${esc(fmtDate(h.date, { weekday: "short", day: "numeric", month: "short", year: "numeric" }))} · ${esc(h.workout)}</td></tr>`;
    }
    html += `<tr title="${esc(h.decision)}"><td>${esc(h.exercise)}${S.historyFilter ? `<div class="small muted">${esc(fmtDate(h.date))}</div>` : ""}</td>
      <td class="num">${h.bodyweight ? "BW" : num(h.weight) + "kg"}</td><td>${esc(h.sets)}</td><td class="num">${num(h.total_reps)}</td><td class="num">${h.bodyweight ? "—" : num(h.volume)}</td>
      <td>${h.pending ? pill("queued", "Queued") + " " : ""}${pill(h.verdict)}</td></tr>`;
  }
  el.innerHTML = html + `</tbody></table></div>`;
  $("#hist-filter").addEventListener("change", (e) => { S.historyFilter = e.target.value; render(); });
}

// ---------------------------------------------------------------- Progress

function renderProgress(el) {
  const rows = S.view.progress;
  const withData = rows.filter((r) => r.sessions > 0);
  if (!S.progressPick || !rows.find((r) => r.exercise === S.progressPick)) S.progressPick = withData[0]?.exercise;
  const pick = rows.find((r) => r.exercise === S.progressPick);
  el.innerHTML = `<h1>Progress</h1><p class="sub">One row per exercise. Pick one to chart every session.</p>
    ${pick ? chartCard(pick) : ""}
    <div class="table-wrap"><table><thead><tr><th>Exercise</th><th class="num">Current</th><th class="num">Best</th><th class="num">Sessions</th><th>Change</th><th>Next</th></tr></thead><tbody>
    ${rows.map((r) => `<tr class="clickable ${r.exercise === S.progressPick ? "selected" : ""}" data-ex="${esc(r.exercise)}">
      <td>${esc(r.exercise)}<div class="small muted">${esc(r.equipment)}</div></td><td class="num">${esc(r.current)}</td><td class="num">${esc(r.best)}</td>
      <td class="num">${r.sessions}</td><td>${esc(r.change)}</td><td>${esc(r.next)}</td></tr>`).join("")}
    </tbody></table></div>`;
  $$("tr[data-ex]", el).forEach((tr) => tr.addEventListener("click", () => { S.progressPick = tr.dataset.ex; render(); }));
  if (pick) drawChart(pick);
}

function chartCard(p) {
  const metric = p.bodyweight ? "Total reps" : "Weight (kg)";
  return `<div class="chart-card" id="chart-card">
    <h3>${esc(p.exercise)} · ${metric}</h3>
    <div class="chart-stats">
      <div class="stat"><div class="v">${esc(p.current)}</div><div class="k">Current</div></div>
      <div class="stat"><div class="v">${esc(p.best)}</div><div class="k">Best</div></div>
      <div class="stat"><div class="v">${p.sessions}</div><div class="k">Sessions</div></div>
      <div class="stat"><div class="v">${esc(p.next)}</div><div class="k">Next</div></div>
    </div>
    <svg class="chart" id="chart" role="img" aria-label="${esc(p.exercise)} ${metric} per session"></svg>
    <div class="tooltip" id="tip" hidden></div>
  </div>`;
}

function niceTicks(min, max, count = 4) {
  if (min === max) { min -= 1; max += 1; }
  const step0 = (max - min) / count;
  const mag = Math.pow(10, Math.floor(Math.log10(step0)));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= step0);
  const lo = Math.floor(min / step) * step;
  const hi = Math.ceil(max / step) * step;
  const out = [];
  for (let v = lo; v <= hi + 1e-9; v += step) out.push(Math.round(v * 100) / 100);
  return out;
}

function drawChart(p) {
  const svg = $("#chart");
  const W = svg.clientWidth || 800, H = 240;
  const m = { l: 40, r: 14, t: 12, b: 26 };
  svg.setAttribute("viewBox", `0 0 ${W} ${H}`);
  const pts = p.points.map((x) => ({ ...x, t: new Date(x.date).getTime(), y: p.bodyweight ? x.total_reps : x.weight }));
  if (!pts.length) return;
  const ys = pts.map((q) => q.y);
  // Pad the range so a 1kg step doesn't look like a cliff.
  const lo = Math.min(...ys), hi = Math.max(...ys), pad = Math.max(p.bodyweight ? 5 : 2, (hi - lo) * 0.25);
  const ticks = niceTicks(Math.max(0, lo - pad), hi + pad / 2);
  const y0 = ticks[0], y1 = ticks[ticks.length - 1];
  const t0 = pts[0].t, t1 = pts[pts.length - 1].t || t0 + 1;
  const X = (t) => m.l + (t1 === t0 ? (W - m.l - m.r) / 2 : ((t - t0) / (t1 - t0)) * (W - m.l - m.r));
  const Y = (v) => m.t + (1 - (v - y0) / (y1 - y0 || 1)) * (H - m.t - m.b);
  let g = ticks.map((v) => `<line class="grid" x1="${m.l}" x2="${W - m.r}" y1="${Y(v)}" y2="${Y(v)}"/><text class="axis-label" x="${m.l - 8}" y="${Y(v) + 4}" text-anchor="end">${v}</text>`).join("");
  // date labels: first, last, and a couple in between
  const labelIdx = pts.length < 4 ? pts.map((_, i) => i) : [0, Math.round((pts.length - 1) / 2), pts.length - 1];
  g += labelIdx.map((i) => `<text class="axis-label" x="${X(pts[i].t)}" y="${H - 6}" text-anchor="${pts.length > 1 && i === 0 ? "start" : pts.length > 1 && i === pts.length - 1 ? "end" : "middle"}">${esc(fmtDate(pts[i].date, { day: "numeric", month: "short" }))}</text>`).join("");
  const path = pts.map((q, i) => `${i ? "L" : "M"}${X(q.t).toFixed(1)},${Y(q.y).toFixed(1)}`).join("");
  g += `<path class="series" d="${path}"/>`;
  g += pts.map((q) => `<circle class="marker" cx="${X(q.t)}" cy="${Y(q.y)}" r="4"/>`).join("");
  g += `<line class="cross" id="cross" y1="${m.t}" y2="${H - m.b}" visibility="hidden"/><rect id="hit" stroke="none" x="${m.l}" y="0" width="${W - m.l - m.r}" height="${H}" fill="transparent"/>`;
  svg.innerHTML = g;

  const tip = $("#tip"), cross = $("#cross"), card = $("#chart-card");
  const move = (ev) => {
    const r = svg.getBoundingClientRect();
    const x = ((ev.clientX - r.left) / r.width) * W;
    let best = pts[0];
    for (const q of pts) if (Math.abs(X(q.t) - x) < Math.abs(X(best.t) - x)) best = q;
    cross.setAttribute("x1", X(best.t)); cross.setAttribute("x2", X(best.t)); cross.setAttribute("visibility", "visible");
    const cr = card.getBoundingClientRect();
    tip.hidden = false;
    tip.innerHTML = `<b>${esc(fmtDate(best.date))}</b> · ${p.bodyweight ? num(best.y) + " reps" : num(best.y) + "kg"} · ${esc(best.sets)} · ${VERDICT[best.verdict]}`;
    tip.style.left = `${r.left - cr.left + (X(best.t) / W) * r.width}px`;
    tip.style.top = `${r.top - cr.top + (Y(best.y) / H) * r.height - 10}px`;
  };
  $("#hit").addEventListener("mousemove", move);
  $("#hit").addEventListener("mouseleave", () => { tip.hidden = true; cross.setAttribute("visibility", "hidden"); });
}

// ---------------------------------------------------------------- Programme

function renderProgramme(el) {
  const v = S.view;
  const on = (name) => v.settings.equipment.some((e) => e.name === name && e.enabled);
  let html = `<h1>Programme</h1><p class="sub">Read from the sheet's 💪 Workouts tab. Edit exercises, sets and alternatives there; the app picks changes up on the next sync.</p>
    <div class="table-wrap"><table><thead><tr><th>Exercise</th><th>Sets × reps</th><th>Equipment</th><th class="num">Start</th><th>Alternative</th></tr></thead><tbody>`;
  let group = "";
  for (const p of v.programme) {
    if (p.workout !== group) { group = p.workout; html += `<tr class="group"><td colspan="5">${esc(group)}</td></tr>`; }
    const mainOn = on(p.equipment);
    html += `<tr><td>${p.video ? `<a href="#" data-url="${esc(p.video)}">${esc(p.exercise)}</a>` : esc(p.exercise)}</td>
      <td>${p.sets} × ${num(p.rep_low)}–${num(p.rep_high)}</td>
      <td class="${mainOn ? "" : "off"}">${esc(p.equipment)}</td><td class="num">${num(p.start_weight)}kg</td>
      <td>${p.alt_exercise ? `${esc(p.alt_exercise)} <span class="small muted">(${esc(p.alt_equipment)}, ${num(p.alt_start_weight)}kg)</span>` : '<span class="muted">—</span>'}</td></tr>`;
  }
  el.innerHTML = html + `</tbody></table></div>`;
  $$("a[data-url]", el).forEach((a) => a.addEventListener("click", (e) => { e.preventDefault(); openUrl(a.dataset.url); }));
}

// ---------------------------------------------------------------- Settings

function renderSettings(el) {
  const v = S.view;
  const c = v.config;
  const st = v.status;
  const set = v.settings;
  el.innerHTML = `<h1>Settings</h1><p class="sub">${st.online ? `Connected · last synced ${esc(st.last_sync)}` : esc(st.message || "Not connected")}</p>
    <div class="panel">
      <h3>Data source</h3>
      <div class="radio">
        <label><input type="radio" name="mode" value="sheets" ${c.mode !== "snapshot" ? "checked" : ""}> Google Sheet (shared across computers)</label>
        <label><input type="radio" name="mode" value="snapshot" ${c.mode === "snapshot" ? "checked" : ""}> Local .xlsx snapshot (read-only, for trying it out)</label>
      </div>
      <div class="row" data-for="sheets"><span>Sheet ID</span><input type="text" id="sheet-id" value="${esc(c.sheet_id)}"><span></span></div>
      <div class="row" data-for="sheets"><span>Service-account key</span><input type="text" id="key-path" value="${esc(c.key_path)}" placeholder="/path/to/key.json"><button class="btn" data-browse="key-path" data-ext="json">Browse</button></div>
      <div class="row" data-for="snapshot"><span>Snapshot file</span><input type="text" id="snap-path" value="${esc(c.snapshot_path)}" placeholder="/path/to/Workout Tracker 2026.xlsx"><button class="btn" data-browse="snap-path" data-ext="xlsx">Browse</button></div>
      ${st.account ? `<div class="small">Share the sheet (Editor) with <span class="copyable">${esc(st.account)}</span></div>` : ""}
      <div><button class="btn primary" id="save-config">Save and connect</button></div>
      <details><summary>How to set up the Google connection (once, about 5 minutes)</summary>
        <ol class="steps">
          <li>Go to console.cloud.google.com with the Google account that owns the sheet, and create a project (any name).</li>
          <li>Search for "Google Sheets API" and click Enable.</li>
          <li>Search for "Service accounts", create one (any name, no roles needed), open it, go to Keys → Add key → JSON. A .json file downloads.</li>
          <li>Open the Workout Tracker sheet → Share, and add the service account's email (ends in iam.gserviceaccount.com) as an Editor.</li>
          <li>Copy the .json file to each computer and pick it above. Keep it private: anyone with it can edit the sheet.</li>
        </ol>
      </details>
    </div>
    ${st.pending ? `<div class="panel"><h3>Queued entries</h3><p class="muted" style="margin:0">${st.pending} exercise(s) are saved on this computer and haven't reached the sheet yet. They're sent automatically on the next successful sync.</p>
      <div><button class="btn danger" id="discard">Discard queued entries</button></div></div>` : ""}
    ${set ? `<div class="panel"><h3>Coach rules (from the sheet's ⚙️ Settings tab)</h3>
      <div class="table-wrap"><table><tbody>
        <tr><td>Auto-approve a weight increase if the jump is at most</td><td class="num">${Math.round(set.jump_pct * 100)}% or ${num(set.jump_abs)}kg</td></tr>
        <tr><td>Sessions topping the rep range before testing a jump</td><td class="num">${num(set.ceiling_streak)}</td></tr>
        <tr><td>Flag Reduce/Review if total reps fall by at least</td><td class="num">${Math.round(set.drop_pct * 100)}%</td></tr>
        <tr><td>Bar / handle weight counted</td><td class="num">${num(set.bar_weight)}kg / ${num(set.handle_weight)}kg</td></tr>
      </tbody></table></div>
      <h3>Plates</h3>
      <div class="table-wrap"><table><thead><tr><th class="num">Plate</th><th class="num">Owned</th><th>Enabled</th></tr></thead><tbody>
        ${set.plates.map((p) => `<tr><td class="num">${num(p.weight)}kg</td><td class="num">${num(p.owned)}</td><td>${p.enabled ? "Yes" : "No"}</td></tr>`).join("")}
      </tbody></table></div>
      <h3>Equipment</h3>
      <div class="table-wrap"><table><thead><tr><th>Kit</th><th>Load type</th><th>Enabled</th></tr></thead><tbody>
        ${set.equipment.map((e) => `<tr><td>${esc(e.name)}</td><td>${esc(e.load_type)}</td><td>${e.enabled ? "Yes" : '<span class="muted">No</span>'}</td></tr>`).join("")}
      </tbody></table></div>
      <p class="small muted" style="margin:0">Change these in the sheet. The app reads them on every sync.</p></div>` : ""}
    <p class="small muted" id="data-dir"></p>`;

  const showMode = () => {
    const mode = $('input[name="mode"]:checked', el).value;
    $$("[data-for]", el).forEach((r) => (r.hidden = r.dataset.for !== mode));
  };
  $$('input[name="mode"]', el).forEach((r) => r.addEventListener("change", showMode));
  showMode();
  $$("[data-browse]", el).forEach((b) => b.addEventListener("click", async () => {
    const f = await T.dialog.open({ multiple: false, filters: [{ name: b.dataset.ext, extensions: [b.dataset.ext] }] });
    if (f) $("#" + b.dataset.browse).value = f;
  }));
  $("#save-config").addEventListener("click", async () => {
    setBusy(true);
    try {
      S.view = await invoke("save_config", { config: {
        mode: $('input[name="mode"]:checked', el).value,
        sheet_id: $("#sheet-id").value.trim(),
        key_path: $("#key-path").value.trim(),
        snapshot_path: $("#snap-path").value.trim(),
      } });
      toast(S.view.status.online || S.view.status.mode === "snapshot" ? "Connected." : "Saved, but couldn't connect. See the message at the top.");
    } catch (e) { toast(String(e)); }
    setBusy(false);
    render();
  });
  $("#discard")?.addEventListener("click", async () => {
    if (!confirm("Discard the queued entries? They haven't been saved to the sheet.")) return;
    S.view = await invoke("discard_pending");
    render();
  });
  invoke("data_dir").then((d) => ($("#data-dir").textContent = `App data (config, offline copy, queue): ${d}`));
}

// ------------------------------------------------------------------ boot

function switchTab(t) {
  S.tab = t;
  render();
  $("#main").scrollTop = 0;
}
$$(".tab").forEach((b) => b.addEventListener("click", () => switchTab(b.dataset.view)));
$("#refresh").addEventListener("click", () => !S.busy && load(true));
window.addEventListener("resize", () => { if (S.tab === "progress") render(); });
// Pick up entries logged on another computer when the window comes back into focus.
window.addEventListener("focus", () => { if (S.view?.status?.mode === "sheets" && !S.busy) load(true); });

load(false);
