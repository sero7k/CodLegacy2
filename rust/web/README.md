# CodLegacy site

Static site: `index.html`, `art/`, `_headers`, `404.html`. No build step.
Preview locally: `python3 -m http.server -d web 8080`.

## Publish (one-time, about 10 minutes, free)
1. Make a free Cloudflare account.
2. Workers & Pages > Create > Pages > Direct Upload, project name `codlegacy`
   (upload any file; the workflow replaces it).
3. My profile > API Tokens > Create Token, template "Edit Cloudflare Workers",
   or custom with "Cloudflare Pages: Edit". Copy the token and your Account ID.
4. In the GitHub repo: Settings > Secrets and variables > Actions, add
   `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID`.
5. Actions > deploy-site > Run workflow. The site is at `codlegacy.pages.dev`.
6. Custom domain: Pages project > Custom domains.

After that, every push to `main` that touches `web/` redeploys.
The sign-in, launcher key and leaderboard on the page need the account service
(`crates/accounts`): without it the page shows the labelled sample leaderboard and sign-in
reports an error. To try everything on your PC run `tools\accounts\run-local.cmd` and open
<http://localhost:8080> (see `docs/HOSTING.md`). To point a Pages deployment at a running
service, set the repository variable `CL_API_URL`. The screenshots are game art owned by their
publishers; remove `art/` and the slideshow if you are unsure about using them.
