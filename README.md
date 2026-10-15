# Workout Tracker

A desktop app (Rust + Tauri) for the **Workout Tracker 2026** Google Sheet. The sheet
stays the single source of truth: every computer running the app reads it and writes new
sessions into it, and the sheet keeps working on its own exactly as before.

## What it does

| Screen | What's on it |
|---|---|
| Today | Today's scheduled session (or a rest-day note), the load to put on the bar with the plates for each side, last time's numbers and trend, and boxes to type in your reps. **Save** writes the rows to the sheet. |
| History | Every logged exercise, grouped by day, filterable by exercise. |
| Progress | The 📈 Progress table, plus a chart of every session for the exercise you pick. |
| Programme | The 💪 Workouts tab, read-only, with kit that's switched off struck through. |
| Settings | The connection, switches to turn kit and individual exercises on or off (written to the sheet), the plates and coach rules, and updates. |

### Morning session

Core and pull-up work is a short 🌅 Morning session, defined on 💪 Workouts like any other
session. Column E of the Schedule tab says which days it's due (weekdays). On those days the
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

Only columns **A:H** (Date, Workout, Exercise, Weight, Set 1-4) of the next empty row(s) on
📝 Workout Log. The helper formulas in I:AI are already filled down those rows, so the
sheet's Dashboard and Progress tabs update as if you'd typed the rows yourself. Before
writing, the app re-reads column C so two computers can't overwrite each other.

The only other writes are the on/off switches in **Settings**: an exercise's *Exercise on?*
(💪 Workouts column U) and a piece of kit's *Enabled?* (⚙️ Settings column B), one Yes/No
cell per click. Before each one the app checks that the row still holds that exercise or kit,
so a rearranged sheet is never written to the wrong row. Switching an exercise off drops it
from its session (the sheet's *Available?* column respects it, so the Dashboard agrees);
switching kit off moves exercises to their alternatives. Everything else, including
exercises, sets and rep ranges, is changed in the sheet.

The log formulas stop at row 2000. The app warns you when fewer than 40 rows are left and
refuses to write past that row.

If the sheet can't be reached, saved sessions are queued on that computer and sent on the
next sync. You can also click the sync button, or just switch back to the window, and the
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
5. In the app, open **Settings → Google Sheet** and pick the .json key file.

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
