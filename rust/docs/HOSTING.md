# Hosting the website and account service

The account service (`crates/accounts`, binary `codlegacy-accounts`) is one program. It
runs the sign-in, the launcher keys, join tickets and the leaderboard, and it can also
serve the website from the same address. You have three ways to run it, from easiest.

## 1. Test on your own PC (no accounts, nothing public)

1. Double-click `tools\accounts\run-local.cmd`. The first run builds it (a few minutes).
2. Open <http://localhost:8080>.
3. Press **Sign in with Steam**, sign in on Steam's page, and you come back signed in.
   Steam sends the browser back to `localhost`, so this works with no public address.
4. In the Account panel press **Generate launcher key** and copy the 32-character key.
   It is shown once. Paste it into the launcher (see `LAUNCHER_ACCOUNTS.md`).

Accounts are stored in `accounts-data\accounts.sqlite` (not committed). Delete that file
to start over. The service listens on `127.0.0.1` only, so nobody else can reach it.

## 2. One small server (everything on one domain)

This is the simplest real setup: one cheap VPS (1 core, 1 GB is plenty to start), a domain,
and Caddy for HTTPS. The service serves the site too, so there is no CORS to set up.

1. Point the domain's DNS A record at the server. Open ports 80 and 443.
2. Build for Linux (`cargo build --release -p accounts` on the server, or on any Linux
   machine) and copy `codlegacy-accounts` and the `web/` folder over.
3. Create the settings file from `deploy/accounts.env.example`
   (`/etc/codlegacy/accounts.env`). Set `ACCOUNTS_PUBLIC_URL=https://your-domain` and one
   long random key per game server in `ACCOUNTS_SERVER_KEYS`.
4. Install `deploy/codlegacy-accounts.service` (the steps are in its first lines) and
   `deploy/Caddyfile` (replace the domain).
5. Back up `/var/lib/codlegacy/accounts.sqlite`. It is the only data.

Steam needs nothing registered for this: sign-in uses Steam's OpenID, which asks only
for the player's Steam ID. No Steam Web API key is required.

## 3. Website on Cloudflare Pages, service on a server

Use this if you want the static site on a CDN. `deploy-site.yml` publishes `web/` to a
Cloudflare Pages project called `codlegacy` (setup steps in `web/README.md`).

1. Set the repository variable `CL_API_URL` to the service's address (for example
   `https://api.your-domain`). The workflow writes it into the page when it deploys.
2. On the server set `ACCOUNTS_PUBLIC_URL=https://api.your-domain` and
   `ACCOUNTS_ALLOWED_ORIGINS=https://codlegacy.pages.dev` (or your custom domain), so the
   site's pages may call the service and Steam sign-in may return to them.

## What has to be done by you

Creating a Cloudflare account, a server and a domain, and setting the repository secrets
and variables, needs your accounts. Everything else is in the repo. GitHub Pages is not
used: the repository is private and Pages for a private repository needs a paid plan,
and the repository holds internal notes that must not be made public.

## Before going public

- The service and site use Call of Duty content and names. A public sign-in service and
  public servers are far more visible than a private build. Get legal advice first, and
  never host game files.
- `web/art/` holds other people's game screenshots. Remove them or replace them with
  your own before the site is public.
- Passwords are argon2id. Session tokens, launcher keys, join tickets and one-time codes
  are stored only as SHA-256 hashes, so a copy of the database signs nobody in.
- The service rate-limits sign-ins by address and by username. Behind a proxy, set
  `ACCOUNTS_TRUST_IP_HEADER` or every player shares the proxy's address.
- Run with `RUST_BACKTRACE` unset and read the service's log (`journalctl -u
  codlegacy-accounts`). A Steam sign-in that fails to confirm with Steam is logged with
  the reason.

## Windows VPS (alternative to Linux)

`deploy/windows/setup.ps1` does the Linux guide's job on a Windows VPS: copies the program and `web/` to
`C:\CodLegacy`, generates the game-server and admin keys (saved in a locked-down `KEYS.txt`), installs Caddy
for HTTPS, runs both as start-at-boot tasks that restart when they stop, and opens ports 80 and 443.

1. On any Windows PC: `cargo build --release -p accounts`, then copy `target\release\codlegacy-accounts.exe`,
   the `web` folder and `deploy\windows\setup.ps1` to the VPS.
2. Point the domain's DNS A record at the VPS.
3. In an Administrator PowerShell on the VPS: `.\setup.ps1 -Domain yourdomain.com -Exe .\codlegacy-accounts.exe -WebDir .\web`
4. To ship a new version later: copy the new files over and run the same command with `-Update` (keys and database are kept).

The script has not been run: it was written without a Windows machine. Windows VPSes cost more than Linux ones
(licence) and want about 4 GB of RAM. Back up `C:\CodLegacy\data\accounts.sqlite` while the service is stopped,
or with SQLite's backup command.
