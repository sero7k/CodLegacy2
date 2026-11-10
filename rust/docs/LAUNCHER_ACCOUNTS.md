# Launcher, accounts and website

Status (5 Nov 2026): the account service, Steam sign-in, launcher keys and the website
account panel are built and tested (`crates/accounts`, `web/index.html`). The launcher
program is built (`crates/player_launcher`, see below). Hosting is in `HOSTING.md`.

## How a player joins (same flow as the ROTK launcher)

1. On the website the player signs in with **Steam** (OpenID). Steam's own page does the
   sign-in; the service never sees a Steam password. A password account also works.
2. The website's Account panel makes a **launcher key**: 32 hex characters, shown once,
   replaceable and revocable. The launcher never generates it.
3. The player pastes the key into the launcher once. The launcher stores it on the PC
   and uses it to prove who they are.
4. When the player presses Play, the launcher trades the key for a **join ticket**
   (`POST /join-ticket`): single use, valid for 60 seconds. It starts the game with the
   ticket. The game sends the ticket to the server, and the server confirms it with the
   service (`POST /verify`). The server never sees the key.

The key links every launch to the player's account and Steam identity. Anyone holding it
can play as that player, so the site tells players to keep it private.

## Account service (`crates/accounts`)

Rust, axum, SQLite (one file; the planned Postgres was not needed for one server, and the
storage is in one module, `db.rs`, if that changes). Settings are `ACCOUNTS_*`
environment variables (`deploy/accounts.env.example`).

| Route | Who | What |
|---|---|---|
| `POST /register`, `POST /login` | anyone | Password accounts. argon2id. Returns a website session token. |
| `GET /auth/steam/start?return=URL` | anyone | Sends the browser to Steam. Comes back to `return` with `?code=` or `?error=`. |
| `POST /auth/exchange` | anyone | Trades the one-time `code` (2 minutes, once) for a session token. |
| `GET /auth/steam/callback` | Steam | Checks the assertion, then asks Steam to confirm its signature. |
| `POST /auth/steam/link`, `DELETE /auth/steam` | website session | Link or unlink Steam. Unlinking needs a password set first. |
| `GET /me`, `GET/PUT /profile` | session or key | Callsign, emblem, custom classes, settings. |
| `POST /api-key`, `GET /api-key`, `DELETE /api-key` | website session only | Make (replacing), inspect, revoke the launcher key. |
| `POST /join-ticket` | session or key | One-minute single-use ticket. |
| `POST /verify` | game server (`X-Server-Key`) | Turns a ticket into the player's account id and callsign. |
| `POST /results` | game server | A finished match's kills, deaths, score. Repeating a match changes nothing. |
| `GET /leaderboard` | anyone | Top players by score. |
| `POST /password`, `POST /logout` | website session | Change password (signs out other sessions), sign out. |

Security decisions:

- **Sign-in returns only to known places.** `return` must be on an allowed origin or be a
  loopback address (the launcher's short-lived listener). Anything else is refused, so
  the service cannot be used as an open redirect.
- **Steam assertions are checked twice.** Locally (the right Steam endpoint, our own
  `return_to` and one-time `state`, the required fields signed, a 17-digit Steam ID, a
  nonce never seen before), then Steam confirms its signature (`check_authentication`).
- **Secrets are stored hashed** (SHA-256): sessions, keys, tickets, codes. Passwords are
  argon2id. Unknown usernames take as long as wrong passwords.
- **Launcher keys are limited.** A key can read the profile and ask for tickets; it cannot
  make keys, change the password or unlink Steam. Those need a website session.
- **Game servers never get a player's key**, only a ticket that is already spent when the
  server has seen it.
- Rate limits: sign-ins by address (20 a minute) and by username (5 failures per 5
  minutes), registrations by address (5 an hour), tickets per account.

Not done yet: bans from an admin page (the `banned` flag exists and is enforced), email
or password reset (Steam sign-in covers most players), match-result signing beyond the
per-server key, and per-server key rotation tooling.

## Launcher (next)

Layout and behaviour follow the ROTK launcher (looked at by reading its installer; none of
its code or art is copied, and it is GPL-3.0):

- A custom title bar, a news column with the latest changes, a large PLAY button and the
  server picker (with player count) at the bottom, and a status line.
- A **launcher key** panel: a field that takes exactly 32 hex characters, with show, copy
  and remove. The key is kept on the PC (Windows data protection where available).
- An install panel: find the game files already on the PC (`asset_transport::game_scan`),
  never download or ship them, verify them by SHA-256 and update the build from a version
  manifest.
- PLAY: check the key (`GET /me`), get a ticket, start `CodLegacy.exe` with the ticket and
  the chosen server.

## Anti-cheat

Server-authoritative hits, movement and damage; sanity checks; bans by account. A
kernel-level client anti-cheat is out of scope.

## Legal

The project uses Call of Duty content. A public login service and servers are far more
visible than a private build. Get advice before going public, and never host game files.

## Website routes (added 2026-11-05)

Public: `GET /servers`, `GET /leaderboard?region&mode&season=all|30d|7d&q&limit`, `GET /competitions?kind=league|tournament`, `GET /overlay/{token}`.
Signed in (website session): `GET /me/stats`, `GET/POST /teams`, `DELETE /teams/{id}`, `POST /teams/{id}/leave`, `GET/POST /tickets`, `POST /creator-requests`, `GET/PUT /overlay`, `POST /overlay/regenerate`.
Game server (`X-Server-Key`): `POST /servers/heartbeat {region, mode, map, players, max_players}` every minute or so; `POST /results` now also takes `region` and `duration_secs`.
Admin (`X-Admin-Key`, set `ACCOUNTS_ADMIN_KEY`): `GET /admin/tickets`, `POST /admin/tickets/{id} {reply, close}`, `GET /admin/creator-requests`, `POST /admin/competitions`, `POST /admin/accounts/{id}/ban {banned}`.

## The launcher (`crates/player_launcher`, binary `codlegacy-launcher`)

A small window (egui) with Home (news, ready check, **Play**), Servers and Settings.
The logic is in the library and is tested; `main.rs` is only layout and threads.

- **Sign-in:** paste the launcher key from the website's Account page into Settings. It is
  stored in `%APPDATA%\CodLegacy\launcher.json` (mode 600 on Unix) and checked against `GET /me`.
- **Game scan:** uses `asset_transport::game_scan` / `game_paths` to find the player's own Black Ops II
  folder; a folder picked by hand is checked the same way. Nothing in that folder is read beyond that check or copied.
- **Updates:** `/data/build/manifest.json` lists CodLegacy's own build files (path, size, SHA-256) and a
  `base_url`, next to `manifest.json.sig`. The launcher checks the signature first (below), then downloads
  files that are missing or differ, checks size and hash, and moves them into place; a mismatch installs
  nothing (support code L5). Black Ops II files are never listed or hosted.
- **Play:** update if needed, find the folder, `POST /join-ticket`, then start `CodLegacy.exe` in the install
  folder with `CODLEGACY_TICKET`, `CODLEGACY_GAME_DIR` and `CODLEGACY_API` in its environment. The long-lived key
  is never passed to the game. The game reading these is the next step (game-loader integration).
- **Errors** carry the support-page codes L1 to L6.

### Signing, self-update and the installer

- **Signed manifests.** Ed25519. The secret key stays on the owner's PC (never the server, never git);
  the public key is compiled into the launcher from `crates/player_launcher/update_key.pub`. Until a key is set
  (the file says `unset`) the launcher refuses every update, which is the safe default. A compromised web
  host can only serve what was signed. Not covered: replaying an old, validly signed manifest.
  First time: `codlegacy-publish keygen codlegacy.signing-key`, paste the printed public key into
  `update_key.pub`, back the secret file up (losing it means shipping a new launcher by hand).
- **Publishing a build.** `codlegacy-publish sign manifest <build-dir> <version> <base-url> codlegacy.signing-key`
  writes `manifest.json` and `manifest.json.sig`; upload both to `/data/build/` and the files to `<base-url>`.
  For a new launcher use name `launcher` (build dir holds `codlegacy-launcher.exe`, version higher than the
  running one) and upload `launcher.json` and `launcher.json.sig` the same way.
- **Launcher self-update.** On start the launcher fetches `launcher.json`, verifies it, and if the version is
  newer shows "Update and restart". The new program is checked (size, SHA-256), the running one is renamed
  to `.old`, the new one takes its name, and it restarts; the `.old` copy is deleted on the next start.
- **Installer.** `tools/installer/build-installer.cmd` builds the launcher in release mode and packs
  `CodLegacy-Setup.exe` with NSIS (`winget install NSIS.NSIS`). Per-user install to
  `%LOCALAPPDATA%\Programs\CodLegacy` (no administrator prompt), Start menu and desktop shortcuts, an entry in
  Add/Remove Programs, silent mode with `/S`. Uninstall removes only the launcher; the downloaded build, the
  settings (with the launcher key) and Black Ops II stay. The script was compile-checked with a stand-in
  program on Linux; the installer has not been run. It is not code-signed, so Windows SmartScreen will warn
  until a code-signing certificate is bought and applied.

## Games check and what is shipped (decided 5 Nov 2026)

- **Your games card.** The launcher lists the games it copies from (see "Which games are packed" below): found on this PC or not, and whether the folder
  is a Steam install (a Steam `appmanifest` names it). Files on disk show a game is installed here; they do not
  prove who bought it, and the card says so. Only Black Ops II is needed to play.
- **What a CodLegacy download contains.** CodLegacy's own program, launcher, website and licences. It does not
  contain Black Ops II or any other publisher's game files, maps, guns, sounds or textures, even for LAN or
  tournament use: the owner cannot license another company's assets, so the build reads them from the player's own
  installs. "Only the assets it needs" is done on the player's PC (`call-of-claude pack`, run locally from their own
  folders), never as something we upload.
- **Pak/mod protection** (planned, not built): the local trimmed pack gets a manifest of hashes made on the
  player's PC at build time and re-checked at start, and servers refuse clients whose pack hash is not on the
  tournament's allowed list. That stops accidental or casual edits and mismatched packs; it cannot stop someone
  who controls their own PC, so rules that matter stay enforced on the server.

### Games tab: copy what CodLegacy needs, then remove the originals (built 5 Nov 2026)

The launcher's **Games** tab uses `call_of_claude::pack` (the same keep/drop rules as the `call-of-claude pack`
command) on the player's own installs (the list under "Which games are packed"). Infinite Warfare and Battlefield 1 are never copied. The player sees the sizes first (Preview), picks the games
and a destination, and presses Copy. The launcher then:

1. copies each kept file through a temporary name, hashing it as it writes;
2. writes `call_of_claude.manifest.json` (size and SHA-256 of every copied file) and the `call_of_claude.txt` marker;
3. re-reads the whole folder and checks it against the manifest;
4. only if all of that passed, points `game_paths.cfg` at the folder (`call_of_claude::adopt`) and saves it as the
   launcher's `pack_dir`; Play and the games card then use it;
5. only then lists which original game folders can be removed, with their size and a "Show folder" button.

The launcher never deletes anything. Steam games are removed with Steam's Uninstall (frees the space, keeps the
licence). If any file fails to copy or check, the tab says the copy is incomplete and offers no removal advice;
running it again redoes only what is missing. The manifest is also the record the pak-protection check will use.
Not done: a free-disk-space check before copying, and server-side use of the pack hash.

### Which games are packed, and what CodLegacy supplies (5 Nov 2026)

- **Copied from the player's own games** (`call_of_claude::pack::PACKED`): Black Ops II (required), Black Ops,
  World at War, CoD4, Black Ops III, Modern Warfare III (2023), Ghosts and Modern Warfare Remastered. The player
  needs their own copy of MWR; the others are used when found.
- **Supplied by CodLegacy** (`SUPPLIED`), so the player needs none of the original games for them: the community
  MW2 and MW3 remasters and the Advanced Warfare maps (guns, maps, sound effects and so on), and the @leetinservers
  and DLC5 Zombies mods. They ship inside CodLegacy's own signed build under `projects/mw2r`, `projects/mw3r`,
  `projects/aw-maps`, `mods/leetin` and `mods/dlc5`; the launcher points `game_paths` at them after every update.
  The build must actually contain those folders (they are not in the repo). Only ship what the projects' authors
  allow to be redistributed, and nothing that contains Activision's own game files.
- **Never packed or required** (`NOT_PACKED`): Infinite Warfare (parked), Battlefield 1, Black Ops Cold War and
  Vanguard (banners only). The `call-of-claude pack` command skips them and the supplied ones too.
- **Already have a trimmed folder?** The Games tab has "Use this folder": a Call of Claude folder made from the
  player's own copies. It must be a pack with Black Ops II in it; if it has a manifest every file is checked and a
  mismatch refuses it. Games CodLegacy does not use are ignored, not pointed at.
