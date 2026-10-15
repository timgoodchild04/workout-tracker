# Workout Tracker

A desktop app (Rust + Tauri) for the **Workout Tracker 2026** Google Sheet. The sheet
stays the single source of truth: every computer running the app reads it and writes new
sessions into it, and the sheet keeps working on its own exactly as before.

## What it does

| Screen | What's on it |
|---|---|
| Today | Today's scheduled session (or a rest-day note), the load to put on the bar with the plates for each side, last time's numbers and trend, and boxes to type in your reps. Each exercise saves itself to the sheet (see below), and its card shows Saved / Queued / Not saved yet. |
| History | Every logged exercise, grouped by day, filterable by exercise. |
| Progress | The 📈 Progress table, plus a chart of every session for the exercise you pick. |
| Coach | Suggestions from your plan and log, each with a one-click change to the plan: weekly sets per muscle against your targets, stalled exercises, and bodyweight moves ready for their harder (or easier) version. Plus a sets-per-muscle table. |
| Programme | Your days (📅 Plan) with what each slot uses today, and the whole 📚 Exercises library by movement pattern, with the days each exercise suits and its easier/harder versions. |
| Settings | The connection, kit on/off, the **plan editor** (swap an exercise, change sets, switch a slot off, remove it, or add one from a list that puts what suits the day first), coach rules, plates and updates. |

### Exercises, days and kit

The programme lives in two tabs:

- **📚 Exercises** is the library: one row per exercise with its movement pattern
  (horizontal push, hinge, core: rotation…), primary and secondary muscles, kit, default sets,
  rep range and start weight, a how-to video, easier and harder versions, and **Fits**: the day
  focuses it suits (Push, Pull, Legs, Upper, Morning). Add an exercise by adding a row.
- **📅 Plan** holds your days. Each day is a list of **slots**: a pattern, the exercise you
  want for it, an optional sets override and On?. Beside it, the **Days** table gives each day
  its focus (that's what the Fits hints match against) and the **targets** table the weekly
  sets per muscle the Coach aims for.

If a slot's exercise needs kit that's switched off, the slot uses the first exercise in the
library with the same pattern whose kit is on (so the order inside a pattern matters), or
drops out if there's none. Switching kit on makes its exercises available in every slot of
their pattern. The sheet works this out too (📅 Plan columns G:P), so the 🏠 Dashboard and
the app always agree; the parity tests check it. The old 💪 Workouts tab is hidden and no longer
used; it keeps its name only so app versions before 0.3.0 still sync until they update.

### On the Today screen

- **Easing back in.** If an exercise hasn't been done for more than 10 days, its first session
  back suggests about 85% of the load you used last (the nearest buildable load at or below
  it), and the plate plan loads for that. Loads under 10kg aren't eased. From then on the
  usual progression builds you back up from what you actually did.
- **Warm-ups.** Plate-loaded compound lifts (presses, rows, pull-ups, squats, lunges, hinges)
  of 10kg or more list warm-up sets: empty bar ×10 (barbell only), about 50% ×8 and 75% ×5,
  each with its plates.
- **Rest timer.** Entering a set starts a countdown at the bottom: 2½ minutes for compound
  lifts, 90 seconds for isolation work, 60 seconds for core. +30s or Skip; it beeps at zero.
- **Personal bests.** An entry that beats every earlier one for that exercise (heavier, or more
  total reps at the heaviest weight; bodyweight: more total reps or seconds) gets a 🏆 PB on
  its card and in History.
- **Session order.** By default Today follows the Schedule tab's weekdays. Set *Session order*
  to *Rotation* (Settings → Plan, or 📅 Plan S2) and the days are done in **rounds**, in the
  Days table's order, whatever the weekday: next up is the first day not yet done this
  round. A day you skip stays owed and stays next up until you do it, and everything after
  it shifts back; the round ends once every day is done. The header shows the round
  (e.g. "Push ✓ · Pull ✓ · Legs (owed) · Upper ✓"), and opening another session while one is
  owed says so. Morning still follows the weekday schedule.

### Morning session

Core and pull-up work is a short 🌅 Morning session, a day on 📅 Plan like any other. Column E of the Schedule tab says which days it's due (weekdays). On those days the
Today screen opens on Morning until it's logged or it's past noon, then switches to the main
session. The header shows whether Morning is done. Pick any session chip to override.

### Exercise order and plate stacking

Plates stay on between sessions. The Today screen starts from what's on the bar and dumbbells now,
worked out by replaying the log up to and including the day you're viewing (the weights you actually
used, in the order you logged them), so exercises already logged today count. If part of a session
is logged, those exercises stay at the top and only the rest are planned. Because it comes from the
shared sheet, every computer gets the same answer.

Plates sit on a sleeve as a stack, so the order you load them decides how many come off at the next
change. The Today screen plans both the exercise order and the stacking order for the whole session,
with the bar and dumbbells sharing one pool of plates. Each card says what to take off (outermost
first), what to put on and in which order, and what's on the sleeve afterwards. Bodyweight and band
work keeps its slot, the first plate-loaded exercise stays first, and compound and isolation slots
don't swap (`src-tauri/src/plan.rs`). On the 15 Oct data the plan takes 24 / 26 / 46 / 18 plate
moves for Push / Pull / Legs / Upper, against 36 / 34 / 60 / 26 loading heaviest plate first.

The coaching logic (double progression, plate maths, jump limits, reduce/review) is a
straight port of the sheet's formulas into Rust (`src-tauri/src/coach.rs`, `engine.rs`).
`tests/parity.rs` checks it against the sheet's own results. On the 15 Oct 2026 export,
all 74 log rows, the Dashboard and the Progress tab matched exactly.

## What the app writes to the sheet

Only columns **A:H** (Date, Workout, Exercise, Weight, Set 1-4) of 📝 Workout Log. The helper
formulas in I:AI are already filled down every row, so the sheet's Dashboard and Progress
tabs update as if you'd typed the rows yourself.

There's no Save button. A card saves itself only once **every** set box has something in it;
put 0 for a set you didn't do. It saves after a 1.2 second pause (so a typo can be fixed
first), or straight away when you move on to another card or switch away from the app. A
partly filled card is never saved; it shows "Fill every set to save". A new exercise goes into the next empty row (the app re-reads column C
first, so two computers can't overwrite each other). Changing a card that's already saved
updates **that same row**, after re-reading it to check it still holds that date and
exercise. Clearing a saved card's boxes never changes or deletes its row; delete it in the sheet.
Saves go one at a time, and a sync never runs twice at once, so nothing is written twice.

The other writes come from **Settings** and the **Coach** tab, and only touch 📅 Plan
columns A:E and ⚙️ Settings column B (kit on/off):

- swap a slot's exercise (C), change its sets (D) or switch it on/off (E): one cell, after
  re-reading the row to check it still holds that exercise;
- add a slot: the first empty 📅 Plan row (re-read first to check it's still empty);
- remove a slot: clears that row's A:E, after the same check;
- Session order: 📅 Plan S2.

**History** can edit or delete any entry. An edit updates that log row in place (the same
guarded update auto-save uses). A delete clears that row's A:H after re-reading it to check
it still holds that date and exercise; the blank line is skipped by everything that reads the
log. An entry still in the local queue is just edited or dropped there.

The library (📚 Exercises), the Days and targets tables, and rep ranges are edited in the
sheet.

The log formulas stop at row 2000. The app warns you when fewer than 40 rows are left and
refuses to write past that row.

If the sheet can't be reached, saves are queued on that computer (the card says Queued) and
sent on the next sync; editing a queued exercise just replaces it in the queue. You can also click the sync button, or just switch back to the window, and the
app pulls in anything logged on another computer.

## Updates

Releases are published on GitHub (`timgoodchild04/workout-tracker`). The app checks for a newer
version on launch and shows a banner, and **Settings → Updates** has a manual check. Updating
downloads the new version, checks its signature against the public key in `tauri.conf.json`,
replaces the app and restarts it. Queued entries and typed-in reps are kept.

To publish a release:

1. Bump `version` in `src-tauri/tauri.conf.json` (and `Cargo.toml`, `package.json`).
2. Commit, then tag and push: `git tag v0.3.0 && git push origin main v0.3.0`.
3. The **Release** workflow builds a universal macOS app, an Ubuntu AppImage + .deb and a
   Windows installer (`-setup.exe`), signs the updates and publishes the release with
   `latest.json`. Nothing needs building locally.

On Windows the installer isn't code-signed, so SmartScreen warns on the first install
(More info → Run anyway). Updates after that install from inside the app.

On Ubuntu, only the **AppImage** can update itself (`chmod +x` it and run it). A `.deb` install
still checks for updates but links to the release page to download the next `.deb`.

The update signing key lives outside the repo in `~/.tauri/workout-tracker.key` (password in
`workout-tracker.key.password`) and in the repo's Actions secrets. Back it up: without it,
installed copies can't accept new updates and would need a manual reinstall.

## One-time Google setup

The app signs in as a Google *service account*, so there's no browser login on each machine
and no token that runs out every week.

1. Go to <https://console.cloud.google.com> with the account that owns the sheet and create a project.
2. Enable the **Google Sheets API**.
3. Under **Service accounts**, create one (no roles needed), then go to **Keys → Add key → JSON**.
4. Share the sheet with the service account's email (`…@….iam.gserviceaccount.com`) as **Editor**.
5. In the app, open **Settings**, click **Browse** next to the service-account key and pick the
   .json file, or paste or type its path (quotes, `~`, a `file://` link, a path missing `/home/`
   or a slash are all fine; if it's not found, the app looks for that file in Downloads, Desktop
   and Documents). The app keeps its own private copy, so the original can be deleted.

Copy the same .json to each computer. Keep it private, because anyone who has it can edit the sheet.

Until then, **snapshot mode** reads a downloaded .xlsx copy (File → Download → .xlsx), so you
can try the app without connecting. Anything you save in snapshot mode is queued on that
computer and **will be sent to the sheet once you connect**. Use *Discard queued entries*
in Settings if it was only a test.

## Building

You need Rust (<https://rustup.rs>) and Node.js.

```sh
npm install
npm run dev                         # run with live reload
npx tauri build --bundles app       # macOS .app → src-tauri/target/release/bundle/macos/
```

Parity test against a fresh export:

```sh
cd src-tauri && WT_SNAPSHOT="/path/to/Workout Tracker 2026.xlsx" cargo test -- --nocapture
```

### Ubuntu / Windows (later)

The code has nothing Mac-specific in it. TLS is rustls and the window is Tauri's system webview.

- **Ubuntu:** `sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`, then `npx tauri build` (makes .deb / .AppImage).
- **Windows:** install the Visual Studio C++ Build Tools (WebView2 is built into Windows 11), then `npx tauri build` (makes .msi / .exe).

Config, the offline copy and the queue live in the OS app-data folder (shown at the bottom of Settings).
