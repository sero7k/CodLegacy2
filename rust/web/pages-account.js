import { h, api, state, tok, setTok, refreshMe, steamButton, human, toast, phero, wrap, needSignIn, comingSoon, when, dur, num, BASE } from './core.js';

const say = (el, text, ok) => { el.textContent = text; el.className = `note ${ok ? 'okc' : text ? 'err' : ''}`; };

async function entered(j) {
  setTok(j.token);
  await state.onAuth();
  state.go('/account');
}

function login() {
  if (state.me) { queueMicrotask(() => state.go('/account')); return h('div'); }
  const msg = h('p', { class: 'note' });
  const u = h('input', { autocomplete: 'username', required: true, maxlength: '20' });
  const p = h('input', { type: 'password', autocomplete: 'current-password', required: true, maxlength: '128' });
  const creds = () => ({ username: u.value.trim(), password: p.value });
  const form = h('form', { class: 'card', onsubmit: async (e) => { e.preventDefault(); try { await entered(await api('POST', '/login', creds())); } catch (x) { say(msg, human(x)); } } },
    h('h3', null, 'Or use a password'), h('label', null, 'Username', u), h('label', null, 'Password', p),
    h('div', { class: 'btns' }, h('button', { class: 'btn primary', type: 'submit' }, 'Sign in'),
      h('button', { class: 'btn', type: 'button', onclick: async () => { try { await entered(await api('POST', '/register', creds())); } catch (x) { say(msg, human(x)); } } }, 'Create account')), msg);
  return h('div', null, phero('Account', 'Sign in', 'Steam is the easy way: no password, and we never see yours.'),
    wrap(h('section', { class: 's' }, h('div', { class: 'grid g2' },
      h('div', { class: 'card' }, h('h3', null, 'Sign in with Steam'), h('p', { class: 'muted' }, 'You go to Steam, sign in there, and come back signed in. A CodLegacy account is made the first time.'), steamButton()), form))));
}

async function account() {
  if (!state.me) return needSignIn('your account');
  const me = state.me;
  let stats = null;
  try { stats = await api('GET', '/me/stats'); } catch { /* service older than the page */ }
  const play = h('section', { class: 'card' }, h('h2', null, 'Time played'),
    stats ? h('div', null, h('div', { style: 'font-size:2.4rem;font-weight:800' }, dur(stats.playtime_secs)),
      h('p', { class: 'note' }, `${num(stats.matches)} matches recorded · ${num(stats.kills)} kills · ${num(stats.deaths)} deaths`),
      h('p', { class: 'note' }, 'Playtime is counted from match records the game servers report. Short sessions and menus are not included.'))
      : h('p', { class: 'muted' }, 'Stats are not available right now.'),
    h('a', { class: 'btn small', href: '/leaderboard' }, 'View the leaderboard'));

  // Linked accounts
  const lmsg = h('p', { class: 'note' });
  const steamBox = h('section', { class: 'card' }, h('h2', null, 'Linked accounts'),
    h('p', { class: 'muted' }, me.steam_id ? `Steam linked · ${me.steam_id}` : 'Steam is not linked.'),
    me.steam_id
      ? h('button', { class: 'btn small', type: 'button', onclick: async () => { try { await api('DELETE', '/auth/steam'); await state.onAuth(); state.go('/account'); } catch (x) { say(lmsg, human(x)); } } }, 'Unlink Steam')
      : h('button', { class: 'btn steam small', type: 'button', onclick: async () => { try { const j = await api('POST', '/auth/steam/link', { return: location.origin + '/account' }); location.href = j.url; } catch (x) { say(lmsg, human(x)); } } }, 'Link Steam'), lmsg);

  // Launcher key
  const kmsg = h('p', { class: 'note' });
  const keyBox = h('div', { class: 'hidden' });
  const info = h('p', { class: 'note' }, 'Checking…');
  const make = h('button', { class: 'btn primary', type: 'button' }, 'Generate launcher key');
  const revoke = h('button', { class: 'btn', type: 'button' }, 'Revoke');
  const paint = (k) => {
    make.textContent = k.exists ? 'Replace launcher key' : 'Generate launcher key';
    revoke.classList.toggle('hidden', !k.exists);
    info.textContent = k.exists ? `Key made ${when(k.created_at)}${k.last_used ? ` · last used ${when(k.last_used)}` : ' · not used yet'}. The key itself is shown only once.` : 'No key yet.';
  };
  api('GET', '/api-key').then(paint).catch(() => { info.textContent = ''; });
  make.onclick = async () => {
    if (!revoke.classList.contains('hidden') && !confirm('Replace your launcher key? The old one stops working.')) return;
    try {
      const k = await api('POST', '/api-key');
      keyBox.className = '';
      keyBox.replaceChildren(h('div', { class: 'keybox', id: 'the-key' }, k.key),
        h('button', { class: 'btn small', type: 'button', onclick: () => navigator.clipboard.writeText(k.key).then(() => toast('Copied'), () => say(kmsg, 'Select the key and copy it by hand.')) }, 'Copy'),
        h('p', { class: 'note' }, 'Copy it now: it is shown only once. Paste it into the launcher.'));
      paint({ exists: true, created_at: Math.floor(Date.now() / 1000) });
    } catch (x) { say(kmsg, human(x)); }
  };
  revoke.onclick = async () => {
    if (!confirm('Revoke your launcher key? The launcher stops working until you make a new one.')) return;
    try { await api('DELETE', '/api-key'); keyBox.className = 'hidden'; paint({ exists: false }); } catch (x) { say(kmsg, human(x)); }
  };
  const keys = h('section', { class: 'card', id: 'key' }, h('h2', null, 'Player launcher key'),
    h('p', { class: 'muted' }, 'Your launcher key proves to the game that you are you. Anyone holding it can play as you, so keep it private.'),
    info, h('div', { class: 'btns' }, make, revoke), keyBox, kmsg);

  // Password
  const pmsg = h('p', { class: 'note' });
  const cur = h('input', { type: 'password', autocomplete: 'current-password', maxlength: '128' });
  const nw = h('input', { type: 'password', autocomplete: 'new-password', maxlength: '128' });
  const pw = h('form', { class: 'card', onsubmit: async (e) => {
    e.preventDefault();
    try { await api('POST', '/password', { current: cur.value || null, new: nw.value }); cur.value = nw.value = ''; say(pmsg, 'Password saved. Other sessions were signed out.', true); } catch (x) { say(pmsg, human(x)); }
  } }, h('h2', null, 'Password'), h('p', { class: 'muted' }, 'Optional if you use Steam. A password lets you unlink Steam without losing access.'),
    h('label', null, 'Current password (leave empty if none set)', cur), h('label', null, 'New password', nw), h('button', { class: 'btn', type: 'submit' }, 'Save password'), pmsg);

  return h('div', null,
    h('section', { class: 'phero pad' }, h('div', { class: 'wrap btns', style: 'justify-content:space-between' },
      h('div', null, h('span', { class: 'eyebrow' }, 'My account'), h('h1', null, me.callsign), h('p', null, 'Your time in CodLegacy, across all regions.')),
      h('a', { class: 'btn', href: '/inventory' }, 'Open inventory'))),
    wrap(h('section', { class: 's' }, h('div', { class: 'grid g2' }, play, steamBox, keys, pw))));
}

function inventory() {
  if (!state.me) return needSignIn('your inventory');
  return h('div', null, phero('Account', 'Inventory', 'Your appearance and loadout, laid out like the in-game Customize screen.'),
    wrap(h('section', { class: 's' }, h('div', { class: 'card' }, h('span', { class: 'badge warn' }, 'pending'), h('h3', { style: 'margin-top:.6em' }, 'First inventory snapshot is pending'),
      h('p', { class: 'muted' }, 'Your inventory is published by the game server after you play. Launch CodLegacy with this account, then check back.'),
      h('a', { class: 'btn', href: '/play' }, 'How to play')))));
}

function wallet() { return state.me ? comingSoon('Account', 'Wallet', 'Nothing can be bought in CodLegacy, so there is nothing to hold yet.', ['Earned currency', 'Reward history']) : needSignIn('your wallet'); }

// ---- stream overlay ---------------------------------------------------------
const STATS = [
  ['rank', 'Rank', 'Your place on the ladder.'], ['score', 'Score', 'Your ten best games, added up.'], ['kills', 'Kills', 'Kills this season.'], ['wins', 'Wins', 'Matches won.'],
  ['kd', 'K/D', 'Kills per death.'], ['winrate', 'Winrate', 'Wins over games played.'], ['games', 'Games', 'Games played.'], ['deaths', 'Deaths', 'Times eliminated.'], ['best', 'Best score', 'Your highest single game.'],
];
async function overlay() {
  if (!state.me) return needSignIn('your stream overlay');
  let cfg;
  try { cfg = await api('GET', '/overlay'); } catch (x) { return wrap(h('section', { class: 's' }, h('p', { class: 'err' }, human(x)))); }
  let fields = cfg.fields.slice();
  const list = h('ol', { class: 'ovl-list' }); const add = h('div', { class: 'btns' }); const url = h('input', { readonly: true, 'aria-label': 'Overlay URL' });
  const status = h('p', { class: 'note' }, 'Up to date');
  const preview = h('div', { class: 'stream' });
  const link = () => `${location.origin}/overlay/${cfg.token}`;
  const save = async () => { status.textContent = 'Saving…'; try { await api('PUT', '/overlay', { fields }); status.textContent = 'Up to date'; } catch (x) { say(status, human(x)); } };
  const draw = () => {
    list.replaceChildren(...fields.map((f, i) => {
      const [, name, hint] = STATS.find((s) => s[0] === f);
      return h('li', null, h('b', null, String(i + 1).padStart(2, '0')), h('div', null, h('strong', null, name), h('div', { class: 'note' }, hint)),
        h('button', { class: 'btn small', type: 'button', 'aria-label': `Move ${name} up`, disabled: i === 0, onclick: () => { [fields[i - 1], fields[i]] = [fields[i], fields[i - 1]]; draw(); save(); } }, '↑'),
        h('button', { class: 'btn small', type: 'button', 'aria-label': `Move ${name} down`, disabled: i === fields.length - 1, onclick: () => { [fields[i + 1], fields[i]] = [fields[i], fields[i + 1]]; draw(); save(); } }, '↓'),
        h('button', { class: 'btn small', type: 'button', 'aria-label': `Remove ${name}`, onclick: () => { fields.splice(i, 1); draw(); save(); } }, '✕'));
    }));
    add.replaceChildren(...STATS.filter((s) => !fields.includes(s[0])).map(([k, n]) => h('button', { class: 'btn small', type: 'button', disabled: fields.length >= 5, onclick: () => { fields.push(k); draw(); save(); } }, `+ ${n}`)));
    preview.replaceChildren(h('div', { class: 'id' }, h('b', null, state.me.callsign), h('small', null, 'CodLegacy')),
      h('div', { class: 'st' }, fields.map((f) => h('div', null, h('b', null, cfg.sample[f] ?? '—'), h('span', null, STATS.find((s) => s[0] === f)[1])))));
    url.value = link();
  };
  draw();
  return h('div', null, phero('Stream overlay', 'On air', 'Show your stats on stream. Pick up to five numbers, put them in order, and paste one link into OBS as a Browser source.', [['Size', '600 × 110'], ['Refresh', 'every 5 min']]),
    wrap(h('section', { class: 's' }, h('div', { class: 'grid g2' },
      h('div', { class: 'card' }, h('span', { class: 'eyebrow' }, '01 · Stats on the card'), h('h2', null, 'Choose and order'), list, h('p', { class: 'note' }, `Add a stat · ${fields.length}/5`), add, status),
      h('div', { class: 'grid' }, h('div', { class: 'card' }, h('span', { class: 'eyebrow' }, '02 · Live preview'), h('h2', null, 'What the stream sees'), preview),
        h('div', { class: 'card' }, h('span', { class: 'eyebrow' }, '03 · Your overlay link'), h('h2', null, 'Paste into OBS'),
          h('p', { class: 'muted' }, 'Sources → + → Browser. Paste the link, set width 600 and height 110.'), url,
          h('p', { class: 'note' }, 'Anyone with this link can show your public stats, the same ones the leaderboard shows, and nothing else. If it leaks, regenerate it: the old link stops working at once.'),
          h('div', { class: 'btns' }, h('button', { class: 'btn small', type: 'button', onclick: () => navigator.clipboard.writeText(link()).then(() => toast('Copied')) }, 'Copy'),
            h('button', { class: 'btn small', type: 'button', onclick: async () => { if (!confirm('Regenerate the link? The old one stops working.')) return; try { cfg.token = (await api('POST', '/overlay/regenerate')).token; draw(); toast('New link made'); } catch (x) { say(status, human(x)); } } }, 'Regenerate link'))))))));
}

// Public page loaded inside OBS: no sign-in, no chrome.
async function overlayPublic({ params }) {
  document.getElementById('hdr').replaceChildren(); document.getElementById('ftr').replaceChildren();
  document.body.style.background = 'transparent';
  const card = h('div', { class: 'stream' });
  const draw = async () => {
    try {
      const j = await api('GET', `/overlay/${params.token}`);
      card.replaceChildren(h('div', { class: 'id' }, h('b', null, j.callsign), h('small', null, 'CodLegacy')), h('div', { class: 'st' }, j.fields.map((f) => h('div', null, h('b', null, j.values[f] ?? '—'), h('span', null, (STATS.find((s) => s[0] === f) || [0, f])[1])))));
    } catch { card.replaceChildren(h('b', null, 'Overlay link not valid')); }
  };
  await draw(); setInterval(draw, 300000);
  return h('div', { style: 'padding:0' }, card);
}

export const accountPages = [
  { path: '/login', title: 'Sign in', render: login },
  { path: '/account', title: 'Account', render: account },
  { path: '/inventory', title: 'Inventory', render: inventory },
  { path: '/wallet', title: 'Wallet', render: wallet },
  { path: '/settings/overlay', title: 'Stream overlay', render: overlay },
  { path: '/overlay/:token', title: 'Overlay', render: overlayPublic },
];
