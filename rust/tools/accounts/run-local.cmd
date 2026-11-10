@echo off
rem Runs the CodLegacy account service and the website on this PC, for testing.
rem Then open http://localhost:8080 in a browser.
rem
rem   - "Sign in with Steam" works here: Steam sends the browser back to localhost.
rem   - Accounts are kept in accounts-data\accounts.sqlite beside the repo.
rem   - Nothing is exposed to the network: it only listens on 127.0.0.1.
rem   - Close this window (or press Ctrl+C) to stop it.
setlocal
cd /d "%~dp0\..\.."
if not exist accounts-data mkdir accounts-data
set ACCOUNTS_BIND=127.0.0.1:8080
set ACCOUNTS_PUBLIC_URL=http://localhost:8080
set ACCOUNTS_DB=%CD%\accounts-data\accounts.sqlite
set ACCOUNTS_STATIC_DIR=%CD%\web
rem A game server key for local tests of /verify and /results (name:key, 24+ characters).
if not defined ACCOUNTS_SERVER_KEYS set ACCOUNTS_SERVER_KEYS=local:local-test-server-key-0123456789
echo Starting the account service (the first build takes a few minutes)...
cargo run -p accounts
echo.
echo The account service stopped.
pause
