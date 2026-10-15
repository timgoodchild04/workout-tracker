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
  update: null,         // { current, latest, notes } from check_update, plus checking / installing / error
  order: (() => { try { return localStorage.getItem("order") || "best"; } catch { return "best"; } })(),
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

// "🟢 Increase — next time load 16kg total — …" -> "Increase — next time load 16kg total — …"
function nextLine(decision) {
  return (decision || "").replace(/^\S+\s/, "");
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

// ------------------------------------------------------------------ data

async function load(refresh = false) {
  setBusy(true);
  try {
    S.view = await invoke("load", { refresh, date: S.logDate });
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
  if (!v) return "";
  const has = (name) => !!name && v.sessions.some((x) => x.name === name);
  const td = v.today;
  const isToday = td && S.logDate === td.date;
  const morning = isToday && has(td.morning) ? td.morning : "";
  const main = isToday && !td.is_rest ? td.session : "";
  const logged = v.history.filter((h) => h.date === S.logDate && has(h.workout)).map((h) => h.workout);
  // Already logged a main session on this date? Show that one, even off-schedule.
  const done = logged.find((w) => w !== morning);
  if (done) return done;
  // The morning session comes first until it's logged or it's past noon.
  if (morning && !logged.includes(morning) && (new Date().getHours() < 12 || !main)) return morning;
  return main || morning || logged[0] || v.sessions[0]?.name || "";
}

/** "🌅 Morning ✓" / "🌅 Morning not done yet" for the Today header ("" when none is due). */
function morningNote(v) {
  const td = v.today;
  if (!td.morning || !v.sessions.some((s) => s.name === td.morning)) return "";
  const done = v.history.some((h) => h.date === td.date && h.workout === td.morning);
  return ` · ${esc(td.morning)} ${done ? "✓" : "not done yet"}`;
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
  ({ today: renderToday, history: renderHistory, progress: renderProgress, coach: renderCoach, programme: renderProgramme, settings: renderSettings })[S.tab](el);
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
    msgs.push(`The Workout Log has room for ${st.rows_left} more rows before its formulas run out. Extend the sheet's formulas soon.`);
  if (S.update?.latest)
    msgs.push(`Version ${esc(S.update.latest)} is available. ${updateAction(false)}`);
  notice.hidden = !msgs.length || S.tab === "settings";
  notice.className = "notice" + (st.online ? " info" : "");
  notice.innerHTML = msgs.join("<br>");
  $(".install-update", notice)?.addEventListener("click", installUpdate);
  $$("a[data-url]", notice).forEach((a) => a.addEventListener("click", (e) => { e.preventDefault(); openUrl(a.dataset.url); }));
}

// ------------------------------------------------------------------ updates
// New versions are GitHub Releases; the updater checks their signature before installing.

async function checkUpdate(silent) {
  S.update = { ...S.update, checking: true, error: "" };
  if (!silent) render();
  try {
    S.update = { ...(await invoke("check_update")), checking: false };
    if (!silent && !S.update.latest) toast("You're on the latest version.");
  } catch (e) {
    S.update = { ...S.update, checking: false, error: String(e) };
    if (!silent) toast(`Couldn't check for updates: ${e}`);
  }
  render();
}

const RELEASES_URL = "https://github.com/timgoodchild04/workout-tracker/releases/latest";

/** Update button/link; a .deb install can't replace itself, so it points at the release page. */
function updateAction(button) {
  if (!S.update.can_install)
    return `<a href="#" data-url="${RELEASES_URL}">Download it from GitHub</a>`;
  const label = S.update.installing ? "Updating…" : "Update and restart";
  return button ? `<button class="btn primary install-update">${label}</button>` : `<a href="#" class="install-update">${label}</a>`;
}

async function installUpdate(e) {
  e?.preventDefault();
  if (S.update?.installing) return;
  S.update = { ...S.update, installing: true };
  render();
  toast("Downloading the update. The app restarts when it's done.");
  try {
    await invoke("install_update");
  } catch (err) {
    S.update = { ...S.update, installing: false };
    toast(`Update failed: ${err}`);
    render();
  }
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
  const sv = v.sessions.find((s) => s.name === session) || { cards: [], skipped: [], switched_off: [] };
  const draft = readDraft();

  const head = `
    <h1>${esc(fmtDate(td.date, { weekday: "long", day: "numeric", month: "long" }))}</h1>
    <p class="sub">${td.is_rest
      ? `Rest day${td.notes ? " · " + esc(td.notes) : ""} · next training day is ${esc(td.next_training_day)}`
      : `Scheduled: <b>${esc(td.session)}</b>${td.notes ? " · " + esc(td.notes) : ""}`}${morningNote(v)}</p>`;

  const chips = `<div class="chips" role="tablist">${v.sessions
    .map((s) => `<button class="chip ${s.name === session ? "active" : ""}" data-session="${esc(s.name)}" role="tab">${esc(s.name)}${
      (!td.is_rest && s.name === td.session) || s.name === td.morning ? '<span class="today-dot" title="Scheduled today"></span>' : ""}</button>`)
    .join("")}</div>`;

  const off = !td.is_rest && session !== td.session && session !== td.morning
    ? `<p class="small muted">You're looking at a different session from your schedule. <a href="#" id="back-to-schedule">Back to ${esc(td.session)}</a></p>` : "";
  const rest = td.is_rest && !S.session && session !== td.morning
    ? `<div class="rest"><h3>Rest day</h3><p class="muted" style="margin:0">Nothing scheduled. Pick a session above if you're training anyway.</p></div>` : "";

  const pp = sv.programme_plan, bp = sv.best_plan;
  const saves = pp && bp ? pp.moves - bp.moves : 0;
  const plan = S.order === "best" && saves > 0 ? bp : pp;
  const orderCtl = !pp ? "" : saves > 0
    ? `<div class="seg" role="group" aria-label="Exercise order">
        <button class="${plan === pp ? "on" : ""}" data-order="programme">As programmed · ${pp.moves} plate moves</button>
        <button class="${plan === bp ? "on" : ""}" data-order="best">Fewest plate changes · ${bp.moves} (saves ${saves})</button></div>`
    : `<span class="small muted">${pp.moves ? `This order already needs the fewest plate changes (${pp.moves} moves).` : ""}</span>`;
  const kit = v.kit;
  const kgList = (a) => (a.length ? a.map((x) => num(x) + "kg").join(", ") : "nothing");
  const kitNote = kit && (kit.bar.length || kit.dumbbells.length)
    ? `<div class="kit"><span class="sublabel">On your kit now${kit.from ? `, left from ${esc(fmtDate(kit.from))}` : ""} (inside → out)</span>
        <div>Bar, each side: <b>${esc(kgList(kit.bar))}</b></div>
        <div>Dumbbells, each end: <b>${esc(kgList(kit.dumbbells))}</b></div></div>` : "";
  const stackNote = plan && plan.naive_moves > plan.moves
    ? `<p class="small muted">Loading the plates in the order shown takes ${plan.moves} plate moves this session. Loading heaviest plate first would take ${plan.naive_moves}.</p>` : "";
  const toolbar = `<div class="toolbar">
      <label class="field">Log date <input type="date" id="log-date" value="${S.logDate}"></label>
      <span class="small muted" id="saved-count" title="Each exercise saves to the sheet by itself once its sets are in."></span>
      ${orderCtl}
      ${sv.skipped.length ? `<span class="small muted">Skipped (kit switched off): ${esc(sv.skipped.join(", "))}</span>` : ""}
      ${sv.switched_off?.length ? `<span class="small muted">Switched off in Settings: ${esc(sv.switched_off.join(", "))}</span>` : ""}
    </div>`;

  const order = plan ? plan.order : sv.cards.map((_, i) => i);
  const cards = order.map((i, pos) => card(sv.cards[i], i, draft[sv.cards[i].exercise], pos + 1, plan?.changes[i] || [])).join("");

  el.innerHTML = head + chips + off + rest + toolbar + kitNote + stackNote + `<div class="cards">${cards}</div>`;

  $$(".chip", el).forEach((b) => b.addEventListener("click", () => { S.session = b.dataset.session; render(); }));
  $("#back-to-schedule")?.addEventListener("click", (e) => { e.preventDefault(); S.session = null; render(); });
  $$("[data-order]", el).forEach((b) => b.addEventListener("click", () => {
    S.order = b.dataset.order;
    try { localStorage.setItem("order", S.order); } catch {}
    render();
  }));
  $("#log-date").addEventListener("change", (e) => { S.logDate = e.target.value || isoToday(); load(false); });
  $$("a[data-url]", el).forEach((a) => a.addEventListener("click", (e) => { e.preventDefault(); openUrl(a.dataset.url); }));
  $$(".card input", el).forEach((inp) => inp.addEventListener("input", onEntryInput));
  $$(".card", el).forEach((c) => c.addEventListener("focusout", onCardLeave));
  $$(".copy-btn", el).forEach((b) => b.addEventListener("click", () => fillSets(b.closest(".card"))));
  updateSavedCount();
}

function card(c, i, d, pos, changes) {
  const bw = c.load_type === "Bodyweight";
  const lg = c.logged;
  const [main, detail] = splitLoad(c.recommended, c.load_type);
  const n = Math.min(c.sets, 4);
  // Unsaved edits (the local draft) win over what's saved, so nothing typed is lost on a re-render.
  const sets = d?.sets ?? (lg ? lg.sets.map((x) => (x == null ? "" : num(x))) : []);
  const weight = d?.weight ?? (lg ? lg.weight : bw ? 0 : c.recommended_kg);
  const done = lg || sets.some((x) => x !== "" && x != null);
  const title = c.video
    ? `<a href="#" data-url="${esc(c.video)}" title="Watch how-to video">${esc(c.exercise)}<span class="play">▶ video</span></a>`
    : esc(c.exercise);
  const last = c.previous_date
    ? `Last time (${esc(fmtDate(c.previous_date))}): <b>${c.previous_weight != null ? num(c.previous_weight) + "kg" : "BW"}</b> · ${esc(c.previous_reps)} · ${esc(c.trend)}`
    : "No sessions logged yet";
  const unit = bw ? "" : c.load_type === "Fixed steps" ? "kg equiv" : c.load_type === "Dumbbell plates" ? "kg each" : "kg";
  return `<article class="card ${done ? "done" : ""} ${lg ? "logged" : ""}" data-i="${i}" data-exercise="${esc(c.exercise)}" data-sets="${n}">
    <div class="card-head">
      <span class="pos">${pos}</span>
      <div class="card-title">${title}
        <div class="meta">${esc(c.equipment)} · ${c.sets} × ${num(c.rep_low)}–${num(c.rep_high)}${c.substituted ? ` · <span title="Main kit is switched off">instead of ${esc(c.original)}</span>` : ""}</div>
      </div>
      <span class="save-state">${saveState(c, d)}</span>
    </div>
    <div class="load">
      <div><div class="sublabel">${lg ? "Target was" : "Load today"}</div><div class="load-main">${esc(main)}</div><div class="load-detail">${esc(detail)}</div></div>
      <div><div class="sublabel">History</div><div class="last">${last}</div></div>
    </div>
    ${changes.length ? `<div class="swap"><span class="sublabel">Plates before this exercise</span>${changes.map((x) => `<div class="${x.startsWith("Sleeve now") ? "sleeve" : ""}">${esc(x)}</div>`).join("")}</div>` : ""}
    <div class="instr" title="${esc(lg ? lg.decision : c.instruction)}">${lg ? `<b>Next time:</b> ${esc(nextLine(lg.decision))}` : esc(coachLine(c))}</div>
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

// ---------------------------------------------------------------- auto-save
// A card saves itself only once every set box has something in it (0 for a set you
// didn't do): after a short pause so a typo can be fixed, or straight away when you
// move on. A partly filled card is never saved. A card that's already in the sheet
// updates its own row. Saves run one at a time, in the order they happen.

const SAVE_DELAY = 1200;
const saveTimers = new Map();   // exercise -> pending timeout
const saving = new Set();       // exercises with a save in flight
const saveErrors = {};          // exercise -> last error
let saveChain = Promise.resolve();
let renderQueued = false;

/** Values as typed: weight number, sets as numbers or null. */
function cardEntry(cardEl) {
  const r = readCard(cardEl);
  return { weight: Number(r.weight) || 0, sets: r.sets.map((x) => (x === "" ? null : Number(x))) };
}

const sameSets = (a, b) => [0, 1, 2, 3].every((k) => (a[k] ?? null) === (b[k] ?? null));

/** Does the draft differ from what's saved for this card? */
function unsaved(c, d) {
  if (!d) return false;
  const sets = d.sets.map((x) => (x === "" ? null : Number(x)));
  if (!c.logged) return sets.some((x) => x != null);
  return (Number(d.weight) || 0) !== c.logged.weight || !sameSets(sets, c.logged.sets);
}

/** Status pill for a card. */
function saveState(c, d) {
  const ex = c.exercise;
  if (saving.has(ex)) return pill("queued", "Saving…");
  if (saveErrors[ex]) return `<span title="${esc(saveErrors[ex])}">${pill("reduce", "Not saved")}</span>`;
  if (unsaved(c, d)) return `<span title="Saves once every set has a number in it. Put 0 for a set you didn't do.">${pill("queued", "Fill every set to save")}</span>`;
  if (c.logged) return c.logged.pending ? pill("queued", "Queued") : pill("increase", "✓ Saved");
  return pill(c.verdict);
}

function refreshCardStates() {
  const cards = currentCards();
  const draft = readDraft();
  $$(".card").forEach((el) => {
    const c = cards[el.dataset.i];
    if (c) $(".save-state", el).innerHTML = saveState(c, draft[c.exercise]);
  });
  updateSavedCount();
}

function updateSavedCount() {
  const el = $("#saved-count");
  if (!el) return;
  const cards = currentCards();
  const saved = cards.filter((c) => c.logged).length;
  el.textContent = cards.length ? `${saved} of ${cards.length} saved${saved && cards.some((c) => c.logged?.pending) ? " (some queued)" : ""}` : "";
}

/** Re-render, unless you're mid-entry in a card: then just refresh the statuses and wait. */
function renderWhenIdle() {
  if (document.activeElement?.closest?.(".card")) {
    renderQueued = true;
    refreshCardStates();
  } else {
    renderQueued = false;
    render();
  }
}

function onEntryInput(e) {
  const cardEl = e.target.closest(".card");
  const ex = cardEl.dataset.exercise;
  const d = readDraft();
  d[ex] = readCard(cardEl);
  writeDraft(d);
  delete saveErrors[ex];
  const filled = d[ex].sets.filter((x) => x !== "").length;
  cardEl.classList.toggle("done", filled > 0 || cardEl.classList.contains("logged"));
  clearTimeout(saveTimers.get(ex));
  if (filled === Number(cardEl.dataset.sets)) saveTimers.set(ex, setTimeout(() => autoSave(cardEl), SAVE_DELAY));
  refreshCardStates();
}

/** Focus left a card: save it now, and catch up on any re-render that was held back. */
function onCardLeave(e) {
  const cardEl = e.currentTarget;
  if (e.relatedTarget && cardEl.contains(e.relatedTarget)) return;
  autoSave(cardEl);
  if (renderQueued && !(e.relatedTarget && e.relatedTarget.closest?.(".card"))) setTimeout(renderWhenIdle, 0);
}

function autoSave(cardEl) {
  const ex = cardEl.dataset.exercise;
  clearTimeout(saveTimers.get(ex));
  const c = currentCards()[cardEl.dataset.i];
  if (!c || c.exercise !== ex) return;
  const v = cardEntry(cardEl);
  // Only a complete card saves: every set box filled, 0 for a set you didn't do.
  // (So clearing boxes never deletes or changes a saved row either.)
  const n = Number(cardEl.dataset.sets);
  if (v.sets.slice(0, n).some((x) => x == null || Number.isNaN(x))) return;
  if (c.logged && v.weight === c.logged.weight && sameSets(v.sets, c.logged.sets)) return; // unchanged
  const entry = { date: S.logDate, workout: currentSession(), exercise: ex, weight: v.weight, sets: v.sets, row: c.logged?.row ?? null };
  saving.add(ex);
  refreshCardStates();
  saveChain = saveChain.then(async () => {
    try {
      S.view = await invoke("save_entry", { entry });
      delete saveErrors[ex];
      // Drop the draft only if nothing more was typed while this was saving.
      const d = readDraft();
      if (d[ex] && sameSets(d[ex].sets.map((x) => (x === "" ? null : Number(x))), entry.sets) && (Number(d[ex].weight) || 0) === entry.weight) {
        delete d[ex];
        writeDraft(d);
      }
      if (/^Couldn't save/.test(S.view.status.message)) toast(S.view.status.message, 6000);
    } catch (err) {
      saveErrors[ex] = String(err);
      toast(`${ex} not saved: ${err}`, 6000);
    }
    saving.delete(ex);
    renderWhenIdle();
  });
}

function fillSets(cardEl) {
  const c = currentCards()[cardEl.dataset.i];
  $$('input[data-k="set"]', cardEl).forEach((i) => (i.value = num(c.rep_high)));
  onEntryInput({ target: cardEl.querySelector("input") });
}

function currentCards() {
  return (S.view.sessions.find((s) => s.name === currentSession()) || { cards: [] }).cards;
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

/** Slots grouped by day, days in session order (rows added later sit with their day). */
function planByDay(v) {
  const days = new Map(v.sessions.map((s) => [s.name, []]));
  for (const p of v.plan) {
    if (!days.has(p.day)) days.set(p.day, []);
    days.get(p.day).push(p);
  }
  return days;
}

const focusOf = (v, day) => v.days.find((d) => d.name === day)?.focus ?? "";
const libOf = (v, name) => v.library.find((l) => l.name === name);
const kitOn = (v, name) => v.settings.equipment.some((e) => e.name === name && e.enabled);

/** "fits 🦾 Day 1 — Push, 💪 Day 4 — Upper": the days whose focus the exercise suits. */
function fitsDays(v, lib) {
  const days = v.days.filter((d) => lib.fits.includes(d.focus)).map((d) => d.name);
  return days.length ? `fits ${days.join(", ")}` : lib.fits.length ? `fits ${lib.fits.join(", ")}` : "";
}

function renderProgramme(el) {
  const v = S.view;
  let html = `<h1>Programme</h1><p class="sub">Your days from the sheet's 📅 Plan, and every exercise in 📚 Exercises. Change the plan in Settings (or in the sheet); add exercises to the library in the sheet.</p>
    <div class="table-wrap"><table><thead><tr><th>Exercise</th><th>Slot</th><th>Sets × reps</th><th>Equipment</th></tr></thead><tbody>`;
  for (const [day, slots] of planByDay(v)) {
    html += `<tr class="group"><td colspan="4">${esc(day)}${focusOf(v, day) ? ` <span class="muted">· ${esc(focusOf(v, day))}</span>` : ""}</td></tr>`;
    for (const p of slots) {
      const lib = libOf(v, p.uses || p.exercise);
      const name = p.uses || p.exercise;
      const link = lib?.video ? `<a href="#" data-url="${esc(lib.video)}">${esc(name)}</a>` : esc(name);
      const note = !p.enabled ? ' <span class="small muted">(switched off)</span>'
        : p.stand_in ? ` <span class="small muted">(kit off: instead of ${esc(p.exercise)})</span>`
        : !p.uses ? ' <span class="small muted">(no kit for this, skipped)</span>' : "";
      html += `<tr><td class="${p.enabled && p.uses ? "" : "off"}">${link}${note}</td><td class="muted">${esc(p.pattern)}</td>
        <td>${lib ? `${p.sets ?? lib.sets} × ${num(lib.rep_low)}–${num(lib.rep_high)}` : ""}</td><td>${esc(lib?.equipment ?? "")}</td></tr>`;
    }
  }
  html += `</tbody></table></div>
    <h2>Exercise library</h2><p class="sub">Grouped by movement pattern. When kit is off, a slot uses the first exercise in its pattern whose kit is on, in this order.</p>
    <div class="table-wrap"><table><thead><tr><th>Exercise</th><th>Equipment</th><th>Sets × reps</th><th class="num">Start</th><th>Fits</th><th>Easier / harder</th></tr></thead><tbody>`;
  let pattern = "";
  for (const l of v.library) {
    if (l.pattern !== pattern) { pattern = l.pattern; html += `<tr class="group"><td colspan="6">${esc(pattern)}</td></tr>`; }
    html += `<tr title="${esc(l.notes)}"><td>${l.video ? `<a href="#" data-url="${esc(l.video)}">${esc(l.name)}</a>` : esc(l.name)}<div class="small muted">${esc([l.primary, ...l.secondary].join(", "))}</div></td>
      <td class="${kitOn(v, l.equipment) ? "" : "off"}">${esc(l.equipment)}</td><td>${l.sets} × ${num(l.rep_low)}–${num(l.rep_high)}</td>
      <td class="num">${num(l.start_weight)}kg</td><td class="small">${esc(fitsDays(v, l))}</td>
      <td class="small">${[l.easier && `← ${esc(l.easier)}`, l.harder && `${esc(l.harder)} →`].filter(Boolean).join("<br>") || '<span class="muted">—</span>'}</td></tr>`;
  }
  el.innerHTML = html + `</tbody></table></div>`;
  $$("a[data-url]", el).forEach((a) => a.addEventListener("click", (e) => { e.preventDefault(); openUrl(a.dataset.url); }));
}

// ---------------------------------------------------------------- Coach

function renderCoach(el) {
  const v = S.view;
  const live = v.status.mode === "sheets" && v.status.online;
  const groups = [["volume", "Weekly volume"], ["stall", "Stalled exercises"], ["progress", "Bodyweight progressions"]];
  const actionLabel = (a) => !a ? "" : a.kind === "add" ? `Add ${a.exercise}` : a.kind === "swap" ? `Swap to ${a.to}` : `Set ${a.sets} sets`;
  let html = `<h1>Coach</h1><p class="sub">Suggestions from your plan and your log. Each one changes the 📅 Plan only when you press its button.</p>`;
  for (const [kind, title] of groups) {
    const list = v.suggestions.map((s, i) => [s, i]).filter(([s]) => s.kind === kind);
    if (!list.length) continue;
    html += `<div class="panel"><h3>${title}</h3>${list.map(([s, i]) => `<div class="suggestion">
        <div><b>${esc(s.title)}</b><div class="small muted">${esc(s.detail)}</div></div>
        ${s.action ? `<button class="btn" data-apply="${i}" ${live && !S.busy ? "" : "disabled"}>${esc(actionLabel(s.action))}</button>` : ""}
      </div>`).join("")}</div>`;
  }
  if (!v.suggestions.length) html += `<div class="panel"><p class="muted" style="margin:0">Nothing to suggest right now: volume is on target and nothing has stalled.</p></div>`;
  html += `<div class="panel"><h3>Sets per muscle</h3>
    <p class="small muted" style="margin:0">Planned = what your days give each muscle per week as scheduled (secondary muscles count half). Targets live on 📅 Plan, columns U:V.</p>
    <div class="table-wrap"><table><thead><tr><th>Muscle</th><th class="num">Planned / week</th><th class="num">Target</th><th class="num">Last 7 days</th></tr></thead><tbody>
    ${v.volume.map((r) => `<tr><td>${esc(r.muscle)}</td><td class="num ${r.target && r.planned < r.target * 0.8 ? "short" : ""}">${num(r.planned)}</td>
      <td class="num">${r.target ? num(r.target) : "—"}</td><td class="num">${num(r.last_7_days)}</td></tr>`).join("")}
    </tbody></table></div></div>`;
  el.innerHTML = html;
  $$("[data-apply]", el).forEach((b) => b.addEventListener("click", () => applyAction(v.suggestions[b.dataset.apply].action)));
}

async function applyAction(a) {
  if (a.kind === "add") return planCall("add_slot", { day: a.day, exercise: a.exercise }, `${a.exercise} added to ${a.day}.`);
  if (a.kind === "swap") return planCall("swap_slot", { row: a.row, from: a.from, to: a.to }, `${a.from} swapped for ${a.to}.`);
  if (a.kind === "sets") return planCall("set_slot_sets", { row: a.row, exercise: a.exercise, sets: a.sets }, `${a.exercise}: ${a.sets} sets.`);
}

/** Run a plan-changing command and re-render with the result. */
async function planCall(cmd, args, done) {
  setBusy(true);
  try {
    S.view = await invoke(cmd, args);
    toast(done);
  } catch (e) {
    toast(String(e), 6000);
  }
  setBusy(false);
  render();
}

// ---------------------------------------------------------------- Settings

/** On/off switch; disabled unless connected to the live sheet. */
function toggle(attrs, on, label) {
  const live = S.view.status.mode === "sheets" && S.view.status.online;
  return `<label class="switch"><input type="checkbox" ${attrs} ${on ? "checked" : ""} ${live && !S.busy ? "" : "disabled"} aria-label="${esc(label)}"><span></span></label>`;
}

/** Settings → Plan: each day's slots (swap, sets, on/off, remove) and an add list with day hints. */
function planEditor(v) {
  const live = v.status.mode === "sheets" && v.status.online && !S.busy;
  const dis = live ? "" : "disabled";
  let html = "";
  for (const [day, slots] of planByDay(v)) {
    const focus = focusOf(v, day);
    html += `<tr class="group"><td colspan="5">${esc(day)}${focus ? ` <span class="muted">· ${esc(focus)}</span>` : ""}</td></tr>`;
    for (const p of slots) {
      const lib = libOf(v, p.exercise);
      const options = v.library.filter((l) => l.pattern === p.pattern || l.name === p.exercise)
        .map((l) => `<option value="${esc(l.name)}" ${l.name === p.exercise ? "selected" : ""}>${esc(l.name)}${kitOn(v, l.equipment) ? "" : " (kit off)"}</option>`).join("");
      const note = !p.enabled ? "" : p.stand_in ? `<div class="small muted">Kit off: using ${esc(p.uses)}</div>` : !p.uses ? '<div class="small muted">No kit for this pattern: skipped</div>' : "";
      const setsOpts = [`<option value="">${lib ? lib.sets : "—"} (library)</option>`, ...[1, 2, 3, 4, 5, 6].map((n) => `<option value="${n}" ${p.sets === n ? "selected" : ""}>${n}</option>`)].join("");
      html += `<tr><td class="muted small">${esc(p.pattern)}</td>
        <td><select class="slot-swap" data-row="${p.row}" data-from="${esc(p.exercise)}" aria-label="Exercise for ${esc(p.pattern)} on ${esc(day)}" ${dis}>${options}</select>${note}</td>
        <td><select class="slot-sets" data-row="${p.row}" data-ex="${esc(p.exercise)}" aria-label="Sets" ${dis}>${setsOpts}</select></td>
        <td>${toggle(`data-slot-row="${p.row}" data-name="${esc(p.exercise)}"`, p.enabled, `${p.exercise} on ${day}`)}</td>
        <td><button class="icon-btn slot-remove" data-row="${p.row}" data-ex="${esc(p.exercise)}" title="Remove from ${esc(day)}" aria-label="Remove ${esc(p.exercise)} from ${esc(day)}" ${dis}>×</button></td></tr>`;
    }
    // Add list: exercises that suit this day first, labelled with the days they fit.
    const onDay = new Set(slots.map((p) => p.exercise));
    const opt = (l) => `<option value="${esc(l.name)}">${esc(l.name)} · ${esc(l.pattern)}${kitOn(v, l.equipment) ? "" : " (kit off)"}${l.fits.includes(focus) ? "" : ` · ${esc(fitsDays(v, l))}`}</option>`;
    const free = v.library.filter((l) => !onDay.has(l.name));
    const fit = free.filter((l) => l.fits.includes(focus)), other = free.filter((l) => !l.fits.includes(focus));
    html += `<tr><td colspan="5"><select class="slot-add" data-day="${esc(day)}" aria-label="Add an exercise to ${esc(day)}" ${dis}>
      <option value="">Add an exercise…</option>
      ${fit.length ? `<optgroup label="Suits ${esc(focus || day)}">${fit.map(opt).join("")}</optgroup>` : ""}
      ${other.length ? `<optgroup label="Other days">${other.map(opt).join("")}</optgroup>` : ""}</select></td></tr>`;
  }
  return html;
}

/** Write a switch to the sheet; on failure put it back and say why. */
async function flip(box, cmd, args) {
  setBusy(true);
  try {
    S.view = await invoke(cmd, args);
    toast(`${args.name ?? args.exercise} switched ${args.enabled ? "on" : "off"}.`);
  } catch (e) {
    box.checked = !args.enabled;
    toast(String(e));
  }
  setBusy(false);
  render();
}

/**
 * Browse for the key (.json) or snapshot (.xlsx). The file's contents are copied into the
 * app's own folder and that path goes in the box, so nothing depends on the native dialog
 * handing back a usable path (it didn't on Ubuntu).
 */
function pickFile(ext, target) {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = "." + ext;
  input.hidden = true;
  document.body.append(input);
  input.addEventListener("change", async () => {
    const file = input.files?.[0];
    input.remove();
    if (!file) return;
    try {
      const data = await new Promise((resolve, reject) => {
        const r = new FileReader();
        r.onload = () => resolve(String(r.result).split(",")[1] || "");
        r.onerror = () => reject(r.error);
        r.readAsDataURL(file);
      });
      $("#" + target).value = await invoke("import_file", { kind: ext === "json" ? "key" : "snapshot", data });
      toast(`${file.name} copied into the app. Click Save and connect.`);
    } catch (e) {
      toast(String(e));
    }
  });
  input.click();
}

function renderSettings(el) {
  const v = S.view;
  const c = v.config;
  const st = v.status;
  const set = v.settings;
  const canSwitch = st.mode === "sheets" && st.online;
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
      <p class="small muted" style="margin:0">Change these in the sheet. The app reads them on every sync.</p></div>
    <div class="panel"><h3>Equipment</h3>
      <p class="small muted" style="margin:0">Switch kit off when you can't get to it. Each slot that needs it uses the next exercise in its movement pattern whose kit is on, or drops out if there isn't one. Switching kit on makes its exercises available everywhere they fit.${canSwitch ? "" : " Connect to the Google Sheet to change these."}</p>
      <div class="table-wrap"><table><thead><tr><th>Kit</th><th>Load type</th><th>On</th></tr></thead><tbody>
        ${set.equipment.map((e) => `<tr><td>${esc(e.name)}</td><td>${esc(e.load_type)}</td>
          <td>${toggle(`data-equip-row="${e.row}" data-name="${esc(e.name)}"`, e.enabled, `${e.name} on`)}</td></tr>`).join("")}
      </tbody></table></div></div>
    <div class="panel"><h3>Plan</h3>
      <p class="small muted" style="margin:0">Each day is a list of slots. Pick a different exercise for a slot from the same movement pattern, change its sets, switch it off, or remove it. "Add an exercise…" lists what suits that day first; the others show which days they suit. If an exercise's kit is off, the slot uses the next exercise in its pattern with kit on. Everything is saved to the sheet's 📅 Plan.</p>
      <div class="table-wrap"><table class="plan-edit"><thead><tr><th>Slot</th><th>Exercise</th><th>Sets</th><th>On</th><th></th></tr></thead><tbody>
        ${planEditor(v)}
      </tbody></table></div></div>` : ""}
    <div class="panel"><h3>Updates</h3>
      <p class="muted" style="margin:0">${S.update?.current ? `Version ${esc(S.update.current)}` : "Version unknown"}${
        S.update?.checking ? " · checking…" : S.update?.latest ? ` · <b>${esc(S.update.latest)} is available</b>` : S.update?.current && !S.update.error ? " · up to date" : ""}</p>
      ${S.update?.latest && S.update.notes ? `<p class="small muted" style="margin:0;white-space:pre-line">${esc(S.update.notes)}</p>` : ""}
      ${S.update?.error ? `<p class="small muted" style="margin:0">Last check failed: ${esc(S.update.error)}</p>` : ""}
      <div>${S.update?.latest ? updateAction(true) : `<button class="btn" id="check-update">Check for updates</button>`}</div></div>
    <p class="small muted" id="data-dir"></p>`;

  $$("input[data-equip-row]", el).forEach((b) => b.addEventListener("change", () =>
    flip(b, "set_equipment", { row: +b.dataset.equipRow, name: b.dataset.name, enabled: b.checked })));
  $$("select.slot-add", el).forEach((sel) => sel.addEventListener("change", () => {
    if (sel.value) planCall("add_slot", { day: sel.dataset.day, exercise: sel.value }, `${sel.value} added to ${sel.dataset.day}.`);
  }));
  $$("select.slot-swap", el).forEach((sel) => sel.addEventListener("change", () =>
    planCall("swap_slot", { row: +sel.dataset.row, from: sel.dataset.from, to: sel.value }, `${sel.dataset.from} swapped for ${sel.value}.`)));
  $$("select.slot-sets", el).forEach((sel) => sel.addEventListener("change", () =>
    planCall("set_slot_sets", { row: +sel.dataset.row, exercise: sel.dataset.ex, sets: sel.value ? Number(sel.value) : null }, `${sel.dataset.ex}: ${sel.value || "library"} sets.`)));
  $$("button.slot-remove", el).forEach((b) => b.addEventListener("click", () => {
    if (confirm(`Remove ${b.dataset.ex} from this day? Its history is kept.`)) planCall("remove_slot", { row: +b.dataset.row, exercise: b.dataset.ex }, `${b.dataset.ex} removed.`);
  }));
  $$("input[data-slot-row]", el).forEach((b) => b.addEventListener("change", () =>
    flip(b, "set_slot", { row: +b.dataset.slotRow, exercise: b.dataset.name, enabled: b.checked })));

  const showMode = () => {
    const mode = $('input[name="mode"]:checked', el).value;
    $$("[data-for]", el).forEach((r) => (r.hidden = r.dataset.for !== mode));
  };
  $$('input[name="mode"]', el).forEach((r) => r.addEventListener("change", showMode));
  showMode();
  $$("[data-browse]", el).forEach((b) => b.addEventListener("click", () => pickFile(b.dataset.ext, b.dataset.browse)));
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
  $("#check-update")?.addEventListener("click", () => checkUpdate(false));
  $(".install-update", el)?.addEventListener("click", installUpdate);
  $("#discard")?.addEventListener("click", async () => {
    if (!confirm("Discard the queued entries? They haven't been saved to the sheet.")) return;
    S.view = await invoke("discard_pending");
    render();
  });
  Promise.all([invoke("data_dir"), invoke("platform_info")]).then(([d, p]) =>
    ($("#data-dir").innerHTML = `App data (config, offline copy, queue): ${esc(d)}${p ? `<br>${esc(p)}` : ""}`));
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
// Switching away mid-card saves that card, so nothing waits on you coming back.
window.addEventListener("blur", () => {
  const cardEl = document.activeElement?.closest?.(".card");
  if (cardEl) autoSave(cardEl);
});
// Pick up entries logged on another computer when the window comes back into focus.
window.addEventListener("focus", () => { if (S.view?.status?.mode === "sheets" && !S.busy) load(true); });

load(false);
checkUpdate(true);
