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
| Settings | The connection, plus the plates, kit and coach rules read from ⚙️ Settings. |

### Exercise order and plate stacking

Plates stay on between sessions. Each session starts from what the previous one left on the bar and
dumbbells, worked out by replaying the log (the weights you actually used, in the order you logged
them). Because it comes from the shared sheet, every computer gets the same answer. The Today screen
shows what should be on the kit before you start.

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
writing, the app re-reads column C so two computers can't overwrite each other. Nothing
else in the sheet is ever changed. Programme and settings changes are made in the sheet.

The log formulas stop at row 384. The app warns you when fewer than 40 rows are left and
refuses to write past that row.

If the sheet can't be reached, saved sessions are queued on that computer and sent on the
next sync. You can also click the sync button, or just switch back to the window, and the
app pulls in anything logged on another computer.

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
