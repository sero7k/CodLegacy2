import { h, api, state, tok, setTok, refreshMe, steamButton, toast, human } from './core.js';
import { mainPages } from './pages-main.js';
import { accountPages } from './pages-account.js';
import { compPages } from './pages-comp.js';

const routes = [...mainPages, ...accountPages, ...compPages].map((r) => {
  const keys = [];
  const re = new RegExp('^' + r.path.replace(/:([a-z]+)/g, (_, k) => (keys.push(k), '([^/]+)')) + '/?$');
  return { ...r, re, keys };
});

const NAV = [['/', 'Home'], ['/play', 'How to play'], ['/servers', 'Servers'], ['/updates', 'Updates'], ['/store', 'Store'], ['/streamers', 'Creators']];
const EXPLORE = [['/leaderboard', 'Leaderboard'], ['/league', 'Leagues'], ['/tournaments', 'Tournaments'], ['/teams', 'My teams'], ['/project', 'About CodLegacy'], ['/media', 'Media'], ['/support', 'Support & error codes']];
const FOOT = [
  ['Play', [['/play', 'How to play'], ['/servers', 'Servers'], ['/play#launcher', 'Download the launcher'], ['/support', 'Support & error codes']]],
  ['Competition', [['/leaderboard', 'Leaderboard'], ['/league', 'Leagues'], ['/tournaments', 'Tournaments'], ['/streamers', 'Creators'], ['/teams', 'My teams']]],
  ['Player', [['/account', 'Account'], ['/inventory', 'Inventory'], ['/wallet', 'Wallet'], ['/settings/overlay', 'Stream overlay'], ['/battle-pass', 'Battle Pass']]],
  ['Project', [['/project', 'About CodLegacy'], ['/updates', 'Updates'], ['/media', 'Media'], ['/privacy', 'Privacy']]],
];

const root = document.getElementById('app');
const hdr = document.getElementById('hdr');
const ftr = document.getElementById('ftr');
let current = location.pathname;

function drawHeader() {
  const me = state.me;
  const here = location.pathname;
  const link = ([p, t]) => h('a', { href: p, class: here === p || (p !== '/' && here.startsWith(p + '/')) ? 'on' : null }, t);
  let explore;
  const exploreBtn = h('button', { type: 'button', 'aria-expanded': 'false', onclick: (e) => { e.stopPropagation(); toggle(explore, exploreBtn); } }, 'Explore ▾');
  explore = h('div', { class: 'menu hidden', style: 'left:0;right:auto' }, EXPLORE.map(link));
  let acct;
  const who = me
    ? h('button', { class: 'who', type: 'button', 'aria-haspopup': 'menu', onclick: (e) => { e.stopPropagation(); toggle(acct, who); } },
        h('span', { class: 'av', 'aria-hidden': 'true' }, me.callsign.slice(0, 1).toUpperCase()),
        h('span', { class: 't' }, h('small', null, 'Player profile'), h('b', null, me.callsign)))
    : null;
  acct = me && h('div', { class: 'menu hidden' },
    h('a', { href: '/account' }, 'Account & launcher key'), h('a', { href: '/inventory' }, 'Inventory'), h('a', { href: '/wallet' }, 'Wallet'),
    h('a', { href: '/settings/overlay' }, 'Stream overlay'), h('a', { href: '/teams' }, 'My teams'), h('a', { href: '/support/tickets' }, 'Support tickets'), h('hr'),
    h('button', { type: 'button', onclick: signOut }, 'Sign out'));
  hdr.className = 'hdr';
  hdr.replaceChildren(h('div', { class: 'in wrap pad', style: 'position:relative' },
    h('a', { class: 'brand', href: '/', 'aria-label': 'CodLegacy home' }, h('b', null, 'COD', h('i', null, 'LEGACY')), h('small', null, 'Black Ops II multiplayer')),
    h('nav', { class: 'nav', 'aria-label': 'Primary' }, NAV.map(link), h('span', { class: 'menuwrap' }, exploreBtn, explore)),
    h('span', { class: 'sp' }),
    me ? h('span', { class: 'menuwrap' }, who, acct) : steamButton('Sign in with Steam'),
    h('button', { class: 'btn small burger', type: 'button', 'aria-label': 'Menu', onclick: () => hdr.classList.toggle('open') }, '☰')));
}
function toggle(menu, btn) {
  const open = menu.classList.contains('hidden');
  document.querySelectorAll('.menu').forEach((m) => m.classList.add('hidden'));
  if (open) { menu.classList.remove('hidden'); btn.setAttribute('aria-expanded', 'true'); }
}
document.addEventListener('click', () => document.querySelectorAll('.menu').forEach((m) => m.classList.add('hidden')));
document.addEventListener('keydown', (e) => { if (e.key === 'Escape') document.querySelectorAll('.menu').forEach((m) => m.classList.add('hidden')); });

function drawFooter() {
  ftr.className = 'ftr';
  ftr.replaceChildren(h('div', { class: 'cols wrap pad' }, FOOT.map(([t, ls]) => h('div', null, h('h4', null, t), ls.map(([p, l]) => h('a', { href: p }, l))))),
    h('div', { class: 'bot wrap pad' }, 'CodLegacy is an independent, non-commercial fan project. Not affiliated with or endorsed by Activision. Call of Duty and Black Ops are trademarks of their owners. No game files are hosted here.'));
}

async function signOut() {
  try { await api('POST', '/logout'); } catch { /* already out */ }
  setTok(null); state.me = null; drawHeader(); go('/');
}

async function render() {
  current = location.pathname;
  const q = new URLSearchParams(location.search);
  let hit = null, params = {};
  for (const r of routes) {
    const m = r.re.exec(current);
    if (m) { hit = r; r.keys.forEach((k, i) => (params[k] = decodeURIComponent(m[i + 1]))); break; }
  }
  drawHeader();
  hdr.classList.remove('open');
  let node, title;
  if (!hit) {
    title = 'Not found';
    node = h('div', { class: 'coming pad' }, h('h1', null, 'Not found'), h('p', { class: 'muted' }, 'That page does not exist.'), h('a', { class: 'btn primary', href: '/' }, 'Back to CodLegacy'));
  } else {
    title = hit.title;
    try { node = await hit.render({ params, query: q }); }
    catch (e) { node = h('div', { class: 'wrap pad', style: 'padding:3em 0' }, h('h1', null, 'Something broke'), h('p', { class: 'err' }, human(e))); }
  }
  document.title = title ? `${title} · CodLegacy` : 'CodLegacy';
  root.replaceChildren(node);
  const hash = location.hash && document.getElementById(location.hash.slice(1));
  if (hash) hash.scrollIntoView(); else window.scrollTo(0, 0);
  root.focus({ preventScroll: true });
}

export function go(path) {
  history.pushState(null, '', path);
  render();
}
state.go = go;
state.onAuth = async () => { await refreshMe(); drawHeader(); };

document.addEventListener('click', (e) => {
  const a = e.target.closest && e.target.closest('a[href]');
  if (!a || e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  if (a.target || a.hasAttribute('download')) return;
  const u = new URL(a.href, location.href);
  if (u.origin !== location.origin) return;
  e.preventDefault();
  if (u.pathname + u.search === location.pathname + location.search) {
    if (u.hash) { history.pushState(null, '', u.pathname + u.search + u.hash); const t = document.getElementById(u.hash.slice(1)); if (t) t.scrollIntoView(); }
    return;
  }
  go(u.pathname + u.search + u.hash);
});
window.addEventListener('popstate', render);

// Coming back from Steam: ?code= (sign-in), ?linked=1, ?error=
async function boot() {
  const q = new URLSearchParams(location.search);
  let landing = null;
  if (q.has('code') || q.has('error') || q.has('linked')) {
    const code = q.get('code'), error = q.get('error'), linked = q.get('linked');
    history.replaceState(null, '', location.pathname + location.hash);
    if (code) {
      try { const j = await api('POST', '/auth/exchange', { code }); setTok(j.token); }
      catch (e) { landing = human(e); }
    } else if (error) landing = human({ code: error });
    else if (linked) landing = 'Steam linked.';
  }
  await refreshMe();
  drawFooter();
  await render();
  if (landing) toast(landing);
}
boot();
