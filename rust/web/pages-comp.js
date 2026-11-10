import { h, api, state, human, phero, wrap, needSignIn, num, when, day, toast, steamButton } from './core.js';

const say = (el, text, ok) => { el.textContent = text; el.className = `note ${ok ? 'okc' : text ? 'err' : ''}`; };
const REGIONS = ['EU', 'NA', 'SEA', 'OCE', 'BR'];
const sel = (opts, value, on, label) => {
  const s = h('select', { 'aria-label': label, onchange: () => on(s.value) }, opts.map(([v, t]) => h('option', { value: v, selected: v === value }, t)));
  return s;
};

// ---- leaderboard -----------------------------------------------------------------
async function leaderboard() {
  const f = { region: '', mode: '', season: 'all', q: '' };
  const body = h('tbody'); const top = h('div', { class: 'podium' }); const info = h('p', { class: 'note' });
  const load = async () => {
    const qs = new URLSearchParams({ limit: '100' });
    for (const k of ['region', 'mode', 'season', 'q']) if (f[k]) qs.set(k, f[k]);
    let j;
    try { j = await api('GET', `/leaderboard?${qs}`); } catch (x) { body.replaceChildren(h('tr', null, h('td', { colspan: 9 }, human(x)))); return; }
    const rows = Array.isArray(j) ? j : j.rows;
    info.textContent = `${num(Array.isArray(j) ? rows.length : j.total)} players ranked`;
    top.replaceChildren(...rows.slice(0, 3).map((r, i) => h('div', { class: 'card' }, h('span', { class: 'badge' }, ['Champion', '2nd', '3rd'][i]), h('b', { class: 'big' }, r.callsign), h('span', { class: 'muted' }, `${num(r.score)} points · ${num(r.wins ?? 0)} wins`))));
    body.replaceChildren(...(rows.length ? rows.map((r) => {
      const kd = r.deaths ? (r.kills / r.deaths).toFixed(2) : String(r.kills);
      return h('tr', null, h('td', null, String(r.rank).padStart(2, '0')), h('td', null, r.callsign), h('td', { class: 'n' }, num(r.score)), h('td', { class: 'n' }, num(r.games ?? 0)),
        h('td', { class: 'n' }, num(r.wins ?? 0)), h('td', { class: 'n' }, num(r.kills)), h('td', { class: 'n' }, num(r.deaths)), h('td', { class: 'n' }, kd), h('td', { class: 'n' }, num(r.best ?? 0)));
    }) : [h('tr', null, h('td', { colspan: 9 }, 'No matches recorded for this filter yet.'))]));
  };
  const qbox = h('input', { type: 'search', placeholder: 'Search player', 'aria-label': 'Search player', onchange: () => { f.q = qbox.value.trim(); load(); } });
  const filters = h('div', { class: 'filters' },
    sel([['', 'All regions'], ...REGIONS.map((r) => [r, r])], '', (v) => { f.region = v; load(); }, 'Region'),
    sel([['', 'All modes'], ['tdm', 'Team Deathmatch'], ['ffa', 'Free-for-all'], ['dom', 'Domination'], ['kc', 'Kill Confirmed'], ['hp', 'Hardpoint']], '', (v) => { f.mode = v; load(); }, 'Mode'),
    sel([['all', 'All time'], ['30d', 'Last 30 days'], ['7d', 'Last 7 days']], 'all', (v) => { f.season = v; load(); }, 'Season'), qbox);
  await load();
  return h('div', null, phero('Live ladder', 'Leaderboard', 'Every finished match is scored the moment it ends. Your rank is your score across your best games, so consistency counts, not grinding.'),
    wrap(h('section', { class: 's' }, top, filters, info, h('div', { class: 'tw' }, h('table', null,
      h('thead', null, h('tr', null, ['Rank', 'Player', 'Score', 'Played', 'Wins', 'Kills', 'Deaths', 'K/D', 'Best'].map((c, i) => h('th', { class: i > 1 ? 'n' : null }, c)))), body)))));
}

// ---- leagues & tournaments --------------------------------------------------------
function competitions(kind) {
  return async () => {
    let list = [];
    try { list = (await api('GET', `/competitions?kind=${kind}`)).competitions; } catch { /* none */ }
    return h('div', null, phero('Competitions', kind === 'league' ? 'Leagues' : 'Tournaments', 'Run by community creators: register your team, follow the games and the standings live.'),
      wrap(h('section', { class: 's' }, list.length
        ? h('div', { class: 'feed' }, list.map((c) => h('div', { class: 'card' }, h('span', { class: 'eyebrow' }, `${c.kind} · ${c.mode} · ${c.region}`), h('h3', null, c.title), h('p', { class: 'muted' }, `By ${c.creator}`), h('span', { class: 'badge' }, c.status))))
        : h('div', { class: 'card' }, h('h3', null, 'No competition here yet'), h('p', { class: 'muted' }, 'Creators can propose one from the Creators page. Teams and standings are ready for when the first one runs.'),
          h('div', { class: 'btns' }, h('a', { class: 'btn', href: '/streamers' }, 'Creators'), h('a', { class: 'btn', href: '/teams' }, 'My teams'))))));
  };
}

// ---- teams ----------------------------------------------------------------------------
async function teams() {
  if (!state.me) return needSignIn('your teams');
  const msg = h('p', { class: 'note' });
  const list = h('div', { class: 'grid g2' });
  const draw = async () => {
    let j;
    try { j = await api('GET', '/teams'); } catch (x) { say(msg, human(x)); return; }
    list.replaceChildren(...(j.teams.length ? j.teams.map((t) => h('div', { class: 'card' }, h('h3', null, `[${t.tag}] ${t.name}`),
      h('p', { class: 'muted' }, `${t.members} member${t.members === 1 ? '' : 's'} · ${t.role}`),
      t.role === 'leader' ? h('button', { class: 'btn small', type: 'button', onclick: async () => { if (!confirm(`Delete ${t.name}?`)) return; try { await api('DELETE', `/teams/${t.id}`); draw(); } catch (x) { say(msg, human(x)); } } }, 'Delete team')
        : h('button', { class: 'btn small', type: 'button', onclick: async () => { try { await api('POST', `/teams/${t.id}/leave`); draw(); } catch (x) { say(msg, human(x)); } } }, 'Leave'))) : [h('p', { class: 'muted' }, 'You are not in a team yet: create one below.')]));
    meta.textContent = `${j.teams.length}/10`;
  };
  const meta = h('b', null, '');
  const name = h('input', { required: true, minlength: '2', maxlength: '32' }); const tag = h('input', { required: true, minlength: '2', maxlength: '5', pattern: '[A-Za-z0-9]{2,5}' });
  const form = h('form', { class: 'card', onsubmit: async (e) => { e.preventDefault(); try { await api('POST', '/teams', { name: name.value.trim(), tag: tag.value.trim() }); name.value = tag.value = ''; say(msg, ''); draw(); } catch (x) { say(msg, human(x)); } } },
    h('h2', null, 'Create a team'), h('div', { class: 'row' }, h('label', null, 'Name', name, h('span', { class: 'note' }, 'Unique on CodLegacy.')), h('label', null, 'Tag', tag, h('span', { class: 'note' }, '2 to 5 letters or digits.'))),
    h('button', { class: 'btn primary', type: 'submit' }, 'Create'), msg);
  await draw();
  return h('div', null, phero('Competitions', 'My teams', 'Create your team, then register it for creators’ leagues and tournaments.', [['Teams', '']]), wrap(h('section', { class: 's' }, list, h('div', { style: 'height:1em' }), form)));
}

// ---- support tickets --------------------------------------------------------------------
async function tickets() {
  if (!state.me) return needSignIn('your tickets');
  const msg = h('p', { class: 'note' }); const list = h('div', { class: 'grid' });
  const draw = async () => {
    let j; try { j = await api('GET', '/tickets'); } catch (x) { say(msg, human(x)); return; }
    list.replaceChildren(...(j.tickets.length ? j.tickets.map((t) => h('div', { class: 'card' }, h('div', { class: 'btns', style: 'justify-content:space-between' }, h('b', null, t.subject), h('span', { class: `badge ${t.status === 'open' ? 'warn' : 'ok'}` }, t.status)),
      h('p', { class: 'note' }, `${t.category} · opened ${when(t.created_at)}`), h('p', null, t.body),
      ...(t.reply ? [h('div', { class: 'notice' }, h('div', null, h('b', null, 'Reply from the team: '), t.reply))] : []))) : [h('p', { class: 'muted' }, 'You have not opened a ticket yet.')]));
  };
  const cat = h('select', null, [['launcher', 'Launcher installation'], ['account', 'Account or key'], ['in_game', 'In game'], ['player_report', 'Report a player']].map(([v, t]) => h('option', { value: v }, t)));
  const subject = h('input', { required: true, maxlength: '120', placeholder: 'One line: what is wrong' }); const bodyEl = h('textarea', { required: true, rows: '6', maxlength: '4000', placeholder: 'What happened, when, what you tried. Error codes help.' });
  const form = h('form', { class: 'card', onsubmit: async (e) => { e.preventDefault(); try { await api('POST', '/tickets', { category: cat.value, subject: subject.value.trim(), body: bodyEl.value.trim() }); subject.value = bodyEl.value = ''; say(msg, 'Ticket opened.', true); draw(); } catch (x) { say(msg, human(x)); } } },
    h('h2', null, 'Open a ticket'), h('label', null, 'Category', cat), h('label', null, 'Subject', subject), h('label', null, 'Describe the problem', bodyEl), h('button', { class: 'btn primary', type: 'submit' }, 'Open the ticket'), msg);
  await draw();
  return h('div', null, phero('Support', 'Your tickets.', 'Open a ticket and the team answers here. A closed ticket stays readable as your copy.'), wrap(h('section', { class: 's' }, h('div', { class: 'grid g2' }, h('div', null, h('h2', null, 'Open and past tickets'), list), form))));
}

// ---- creators -----------------------------------------------------------------------------
function creators() {
  const msg = h('p', { class: 'note' });
  const f = {}; const field = (k, label, ph, tag = 'input') => { f[k] = h(tag, { placeholder: ph, maxlength: tag === 'textarea' ? '4000' : '1024', rows: tag === 'textarea' ? '5' : null }); return h('label', null, label, f[k]); };
  const kind = h('select', null, [['', 'Let’s talk / not sure yet'], ['skin', 'Custom camo or emblem'], ['event', 'Community event'], ['league', 'League or tournament'], ['other', 'Something else']].map(([v, t]) => h('option', { value: v }, t)));
  const form = h('form', { class: 'card', id: 'creator-request', onsubmit: async (e) => {
    e.preventDefault();
    try { await api('POST', '/creator-requests', { twitch: f.twitch.value, youtube: f.youtube.value, x: f.x.value, kind: kind.value, title: f.title.value, message: f.message.value }); say(msg, 'Sent. We reply here on the site; please be patient.', true); }
    catch (x) { say(msg, state.me ? human(x) : 'Sign in first so we can answer you.'); }
  } }, h('h2', null, 'Tell us about you'), h('p', { class: 'note' }, 'Every field is optional. Links or @handles both work.'),
    h('div', { class: 'row' }, field('twitch', 'Twitch', 'twitch.tv/your-channel'), field('youtube', 'YouTube', 'youtube.com/@you'), field('x', 'X / Twitter', '@your-handle')),
    h('label', null, 'What would you like to do?', kind), field('title', 'A name for your idea', 'A camo, a community night…'), field('message', 'Anything specific in mind?', 'Your idea, a date, a style, or just hello.', 'textarea'),
    h('button', { class: 'btn primary', type: 'submit' }, 'Send my request'), msg);
  return h('div', null, phero('CodLegacy × Creators', 'You bring the community.', 'The project is better with its streamers and mappers. Tell us what you would like to make together.'),
    wrap(h('section', { class: 's' }, h('div', { class: 'grid g3' },
      h('div', { class: 'card' }, h('h3', null, 'A camo that’s yours'), h('p', { class: 'muted' }, 'A custom look or emblem, earned not sold.')),
      h('div', { class: 'card' }, h('h3', null, 'Bring everyone together'), h('p', { class: 'muted' }, 'A community night, a league or a tournament.')),
      h('div', { class: 'card' }, h('h3', null, 'Something else?'), h('p', { class: 'muted' }, 'A collaboration we have not thought of yet.'))), h('div', { style: 'height:1.4em' }), form)));
}

export const compPages = [
  { path: '/leaderboard', title: 'Leaderboard', render: leaderboard },
  { path: '/league', title: 'Leagues', render: competitions('league') },
  { path: '/tournaments', title: 'Tournaments', render: competitions('tournament') },
  { path: '/teams', title: 'My teams', render: teams },
  { path: '/support/tickets', title: 'Support tickets', render: tickets },
  { path: '/streamers', title: 'Creators', render: creators },
];
