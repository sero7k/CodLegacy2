import { h, raw, api, state, phero, wrap, comingSoon, num, day, steamButton } from './core.js';

const REGIONS = [['EU', 'Europe'], ['NA', 'North America'], ['SEA', 'Asia'], ['OCE', 'Oceania'], ['BR', 'South America']];

async function servers() {
  try { return (await api('GET', '/servers')).servers || []; } catch { return null; }
}

async function home() {
  const list = await servers();
  const online = list ? list.reduce((a, s) => a + s.players, 0) : null;
  const hero = h('section', { class: 'hero' },
    h('img', { src: '/art/shot1.jpg', alt: '' }),
    h('div', { class: 'c pad' },
      h('span', { class: 'eyebrow' }, 'Black Ops II multiplayer · rebuilt in Rust'),
      h('h1', null, 'It plays like ', h('em', null, 'Black Ops II.')),
      h('p', { class: 'lead' }, 'Just with the expansion packs the game deserved. Experience the classic Call of Duty titles with Black Ops II\u2019s game, style and flow.'),
      h('div', { class: 'btns' },
        h('a', { class: 'btn primary', href: '/play' }, 'How to play'),
        h('a', { class: 'btn', href: '/updates' }, 'Follow development'),
        state.me ? null : steamButton())));
  const pop = h('section', { class: 's pad' }, h('div', { class: 'wrap' },
    h('span', { class: 'eyebrow' }, 'The community, right now'),
    h('h2', null, 'Players online'),
    list === null
      ? h('p', { class: 'muted' }, 'The server list is not reachable right now.')
      : h('div', null,
          h('p', null, h('span', { class: 'dot' }), h('b', null, num(online)), ' online across ', h('b', null, list.length), list.length === 1 ? ' server' : ' servers'),
          h('div', { class: 'pop' }, REGIONS.map(([code, name]) => {
            const rs = list.filter((s) => s.region === code);
            return h('article', null, h('h3', null, code, h('span', null, name)),
              h('div', { class: 'tot' }, num(rs.reduce((a, s) => a + s.players, 0))),
              h('dl', null, rs.length ? rs.map((s) => [h('dt', null, s.mode), h('dd', null, `${s.players}/${s.max_players}`)]) : [h('dt', null, 'No servers'), h('dd', null, '—')]));
          })),
          h('p', { class: 'note' }, 'Counts come from the game servers themselves, refreshed every minute. Nothing here is estimated.'))));
  const road = h('section', { class: 's pad' }, h('div', { class: 'wrap' },
    h('span', { class: 'eyebrow' }, '01 · The road back'), h('h2', null, 'Built in the open.'),
    h('p', { class: 'muted', style: 'max-width:60ch' }, 'Every milestone is written up in the updates feed. No invented launch dates and no made-up numbers on this site: where something is not ready it says so.'),
    h('a', { class: 'btn', href: '/updates' }, 'Open the updates feed')));
  const platform = h('section', { class: 's pad' }, h('div', { class: 'wrap' },
    h('span', { class: 'eyebrow' }, '02 · The platform'), h('h2', null, 'Everything on CodLegacy'),
    h('ol', { class: 'ledger' }, [
      ['/play', 'How to play', 'Steam sign-in, launcher key and launcher: the full guide.', 'Available now'],
      ['/leaderboard', 'Leaderboard', 'Every finished match is scored; your rank is your best games.', 'Live'],
      ['/updates', 'Updates', 'Patch notes and development stories.', 'Live'],
      ['/league', 'Leagues & tournaments', 'Community-run competitions with teams and standings.', 'Early'],
      ['/battle-pass', 'Progression', 'Battle Pass, inventory and player identity.', 'Coming soon'],
      ['/media', 'Media', 'Broadcasts, creator streams and highlights.', 'Coming soon'],
    ].map(([p, t, d, s], i) => h('li', null, h('span', null, String(i + 1).padStart(2, '0')),
      h('div', null, h('h3', null, h('a', { href: p, style: 'color:inherit;text-decoration:none' }, t)), h('p', null, d)),
      h('span', { class: `badge ${s === 'Coming soon' ? '' : 'ok'}` }, s))))));
  return h('div', null, hero, pop, road, platform);
}

function play() {
  return h('div', null,
    phero('Installation guide', 'How to play.', 'CodLegacy runs from the Black Ops II copy you already own. Sign in with Steam, make your launcher key, install the launcher.', [['Setup', 'about 3 minutes'], ['Sign-in', 'Steam only'], ['Price', 'free']]),
    wrap(h('section', { class: 's', id: 'launcher' },
      h('span', { class: 'eyebrow' }, 'Step by step'), h('h2', null, 'Three steps to the lobby.'),
      h('ol', { class: 'steps' },
        h('li', null, h('div', null, h('h3', null, 'Sign in with Steam'),
          h('p', { class: 'muted' }, 'Steam is the sign-in. No email address and no password are needed, and we never see your Steam password. Your CodLegacy account is created the first time you sign in.'),
          state.me ? h('span', { class: 'badge ok' }, `Signed in as ${state.me.callsign}`) : steamButton())),
        h('li', null, h('div', null, h('h3', null, 'Generate your launcher key'),
          h('p', { class: 'muted' }, 'Open your account and press Generate key. It is 32 characters long.'),
          h('div', { class: 'notice' }, h('span', null, '⚠'), h('div', null, h('b', null, 'The key is shown only once. '), 'Copy it somewhere safe. If you lose it, make a new one: that cancels the old one.')),
          h('a', { class: 'btn', href: '/account' }, 'Open my account'))),
        h('li', null, h('div', null, h('h3', null, 'Install the launcher'),
          h('p', { class: 'muted' }, 'The launcher finds your Black Ops II files, checks them, and signs you in with your key. Paste the key into the launcher once.'),
          h('p', null, h('span', { class: 'badge warn' }, 'Launcher build not released yet'), ' The first build is in development; this page will link it the day it exists.'),
          h('a', { class: 'btn off', href: '#launcher', 'aria-disabled': 'true' }, 'Download the launcher')))),
      ), h('section', { class: 's' },
      h('span', { class: 'eyebrow' }, 'Your files'), h('h2', null, 'We never host the game.'),
      h('p', { class: 'muted prose' }, 'CodLegacy does not download or distribute any Call of Duty files. The launcher only works with the copy you already own, checks that it is intact, and never changes anything in it without telling you.'),
      h('a', { class: 'btn', href: '/project' }, 'What this project is, and is not')),
      h('section', { class: 's' }, h('h2', null, 'Something went wrong?'),
        h('p', { class: 'muted' }, 'Every launcher and game error has a code with an explanation.'),
        h('div', { class: 'btns' }, h('a', { class: 'btn', href: '/support' }, 'Error codes'), h('a', { class: 'btn', href: '/support/tickets' }, 'Open a ticket')))));
}

async function serversPage() {
  const list = await servers();
  const body = list === null ? h('p', { class: 'muted' }, 'The server list is not reachable right now.')
    : !list.length ? h('div', { class: 'card' }, h('h3', null, 'No servers online yet'), h('p', { class: 'muted' }, 'Servers appear here the moment they start reporting. When there is one, the launcher lists it too.'))
    : h('div', { class: 'tw' }, h('table', null, h('thead', null, h('tr', null, ['Server', 'Region', 'Mode', 'Map', 'Players'].map((c, i) => h('th', { class: i === 4 ? 'n' : null }, c)))),
        h('tbody', null, list.map((s) => h('tr', null, h('td', null, s.name), h('td', null, s.region), h('td', null, s.mode), h('td', null, s.map || '—'), h('td', { class: 'n' }, `${s.players}/${s.max_players}`))))));
  return h('div', null, phero('Play', 'Servers', 'Live from the servers themselves. A server that stops reporting disappears after two minutes.'),
    wrap(h('section', { class: 's' }, body,
      h('h2', { style: 'margin-top:1.6em' }, 'Ping guide'),
      h('p', { class: 'muted prose' }, 'Pick the server closest to you: under 50 ms feels like a LAN, 50–100 ms is fine, and past 150 ms you will notice it. The launcher shows your ping to each server before you press Play.'))));
}

const ERROR_CODES = [
  ['L1', 'Launcher key missing or malformed', 'The launcher key must be exactly 32 characters, 0-9 and a-f. Copy it again from your account page.'],
  ['L2', 'Launcher key cancelled', 'You made a newer key or revoked this one. Make a new key on your account page and paste it in.'],
  ['L3', 'Could not reach the account service', 'The launcher could not contact CodLegacy. Check your connection; if the site is up and this stays, a firewall is blocking the launcher.'],
  ['L4', 'Game files not found', 'The launcher could not find a Black Ops II install. Pick the folder yourself.'],
  ['L5', 'Game files failed the check', 'A file does not match the expected checksum. Verify your copy through Steam, then try again.'],
  ['L6', 'Join ticket expired or already used', 'Tickets last one minute and work once. Press Play again to get a fresh one.'],
  ['G1', 'Server rejected your ticket', 'The game server could not confirm your ticket with the account service. Press Play again.'],
  ['G2', 'Account banned', 'This account was removed following a moderation decision. Open a ticket if you think that is wrong.'],
  ['G3', 'Lost connection to the game server', 'The connection dropped after you joined. This is a network path problem between you and the server.'],
  ['G4', 'Version mismatch', 'The launcher and server run different builds. Update the launcher.'],
];
function support() {
  return h('div', null, phero('Support', 'Need a hand?', 'Error codes, connection problems and where to ask. CodLegacy is a community project: tickets are answered by people, so give it a little time.'),
    wrap(h('section', { class: 's' }, h('h2', null, 'Get help'),
      h('p', { class: 'muted' }, 'Open a ticket with your Steam account: pick a category, describe the problem, and the team replies on the site. You keep a copy once it is closed.'),
      h('a', { class: 'btn primary', href: '/support/tickets' }, 'Your tickets')),
      h('section', { class: 's' }, h('h2', null, 'Error codes'),
        h('p', { class: 'muted' }, 'When the launcher or game cannot continue it shows a code. These are the codes and what they mean.'),
        h('dl', { class: 'codes' }, ERROR_CODES.map(([c, t, d]) => h('div', { id: c }, h('dt', null, h('b', null, c), t), h('dd', null, d)))))));
}

function project() {
  return h('div', null, phero('Independent fan project', 'The project.', 'We rebuild the game. We do not own it.'),
    wrap(h('section', { class: 's prose' },
      raw(`<h2>What CodLegacy is</h2>
<p>CodLegacy is an independent, non-commercial recreation of Black Ops II multiplayer written in Rust, with maps, weapons and effects ported from other classic Call of Duty games. The standard we hold it to is simple: it has to feel like Black Ops II.</p>
<h2>What we build</h2>
<p>The engine, the game rules, the account system and this website. The accounts service signs you in with Steam and hands the launcher short-lived tickets; game servers confirm those tickets and report finished matches.</p>
<h2>What we do not have</h2>
<p>CodLegacy does not own or claim Call of Duty, Black Ops or any of their original code, art, audio, names, logos or trademarks. Those belong to their rights holders. No game files are hosted on this site or by the launcher, and the project is not affiliated with or endorsed by Activision.</p>
<h2>Our promise</h2>
<ul><li>No paid access.</li><li>No paid cosmetics or gameplay advantages.</li><li>No profit motive. If donations are ever taken they only cover servers and development, and never unlock anything.</li></ul>`),
      h('div', { class: 'btns' }, h('a', { class: 'btn primary', href: '/updates' }, 'Follow development')))));
}

function privacy() {
  return h('div', null, phero('Privacy', 'What we keep, and why.', null),
    wrap(h('section', { class: 's prose' }, raw(`<h2>Sign-in</h2>
<p>Steam sign-in tells us your 64-bit Steam ID and nothing else. We never see your Steam password. We do not ask for or store an email address.</p>
<h2>What is stored</h2>
<ul><li>Your callsign, your Steam ID, and an optional password hash if you set a password.</li>
<li>A hash of each sign-in session, launcher key and join ticket. The secrets themselves are never stored.</li>
<li>Match results your servers report: kills, deaths, score, mode, region and when.</li>
<li>Support tickets and creator requests you send, and your teams.</li></ul>
<h2>Your launcher key</h2>
<p>Treat it like a password: anyone holding it can play as you. You can replace or revoke it on your account page at any time.</p>
<h2>Deleting your data</h2>
<p>Open a support ticket and ask. Match results are removed with the account.</p>`))));
}

async function updates() {
  let items = [];
  try { items = (await (await fetch('/data/updates.json', { cache: 'no-cache' })).json()).updates; } catch { /* leave empty */ }
  let kind = 'all';
  const grid = h('div', { class: 'feed' });
  const draw = () => {
    grid.replaceChildren(...items.filter((u) => kind === 'all' || u.kind === kind).map((u) =>
      h('a', { class: 'card', href: `/updates/${u.slug}` }, h('div', { class: 'meta' }, h('span', null, u.kind === 'patch' ? 'Patch notes' : 'Dev update'), h('span', null, day(Date.parse(u.date) / 1000))),
        h('h3', null, u.title), h('p', { class: 'muted' }, u.summary))));
    if (!grid.children.length) grid.append(h('p', { class: 'muted' }, 'Nothing here yet.'));
  };
  const tab = (k, t) => { const b = h('button', { type: 'button', class: k === kind ? 'on' : null, onclick: () => { kind = k; tabs.querySelectorAll('button').forEach((x) => x.classList.toggle('on', x === b)); draw(); } }, t); return b; };
  const tabs = h('div', { class: 'tabs' }, tab('all', 'All'), tab('patch', 'Patch notes'), tab('dev', 'Dev updates'));
  draw();
  return h('div', null, phero('Development archive', 'Updates', 'Every meaningful change, written down.', [[`${items.length}`, ' entries']]), wrap(h('section', { class: 's' }, tabs, h('div', { style: 'height:1em' }), grid)));
}
async function updateDetail({ params }) {
  let items = [];
  try { items = (await (await fetch('/data/updates.json', { cache: 'no-cache' })).json()).updates; } catch { /* empty */ }
  const u = items.find((x) => x.slug === params.slug);
  if (!u) return wrap(h('section', { class: 's' }, h('h1', null, 'Not found'), h('a', { class: 'btn', href: '/updates' }, 'All updates')));
  return h('div', null, phero(u.kind === 'patch' ? 'Patch notes' : 'Dev update', u.title, null, [['Date', day(Date.parse(u.date) / 1000)]]),
    wrap(h('section', { class: 's prose' }, u.body.map((p) => (p.startsWith('# ') ? h('h2', null, p.slice(2)) : h('p', null, p))), h('a', { class: 'btn', href: '/updates' }, '← All updates'))));
}

export const mainPages = [
  { path: '/', title: '', render: home },
  { path: '/play', title: 'How to play', render: play },
  { path: '/servers', title: 'Servers', render: serversPage },
  { path: '/support', title: 'Support', render: support },
  { path: '/project', title: 'About', render: project },
  { path: '/privacy', title: 'Privacy', render: privacy },
  { path: '/updates', title: 'Updates', render: updates },
  { path: '/updates/:slug', title: 'Updates', render: updateDetail },
  { path: '/store', title: 'Store', render: () => comingSoon('Cosmetics', 'Store', 'Skins and camos you earn in play. Nothing here is for sale.', ['Earned cosmetics', 'Camos and emblems', 'No paid items']) },
  { path: '/media', title: 'Media', render: () => comingSoon('Broadcast', 'Media', 'The signal is warming up.', ['Official broadcasts', 'Creator streams', 'Highlights and replays']) },
  { path: '/battle-pass', title: 'Battle Pass', render: () => comingSoon('Progression', 'Battle Pass', 'Seasons of levels and rewards, free to everyone.', ['Season levels', 'Game-synchronised XP', 'Cosmetic rewards']) },
];
