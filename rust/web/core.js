// Shared helpers: DOM builder, API client, sign-in state, router hooks.
// Everything dynamic goes in through textContent (h()), never innerHTML.

const meta = document.querySelector('meta[name="cl-api"]');
export const BASE = ((meta && meta.content) || '').replace(/\/+$/, '');
const KEY = 'cl-token';

export const tok = () => { try { return localStorage.getItem(KEY); } catch { return null; } };
export const setTok = (t) => { try { t ? localStorage.setItem(KEY, t) : localStorage.removeItem(KEY); } catch { /* storage blocked */ } };

export const state = { me: null, onAuth: () => {}, go: () => {} };

export function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else el.setAttribute(k, v === true ? '' : v);
  }
  add(el, kids);
  return el;
}
export function add(el, kids) {
  for (const k of kids.flat(Infinity)) {
    if (k == null || k === false) continue;
    el.append(k.nodeType ? k : String(k));
  }
  return el;
}
// Trusted, static markup only (page copy written in this repo).
export function raw(markup) {
  const t = document.createElement('template');
  t.innerHTML = markup;
  return t.content;
}

export const ERR = {
  steam_rejected: 'Steam did not confirm that sign-in. Try again.',
  steam_unreachable: 'Could not reach Steam. Try again in a moment.',
  steam_replay: 'That Steam sign-in was already used. Start again.',
  steam_invalid: 'That Steam sign-in could not be checked. Start again.',
  steam_in_use: 'That Steam account is already linked to another player.',
  banned: 'This account is banned.',
  invalid_credentials: 'Wrong username or password.',
  invalid_username: 'Usernames are 3-20 letters, numbers or underscores.',
  invalid_password: 'Passwords are 8-128 characters.',
  username_taken: 'That username is taken.',
  rate_limited: 'Too many attempts. Wait a few minutes.',
  set_password_first: 'Set a password before unlinking Steam, or you would lose access.',
  unauthorized: 'Please sign in again.',
  name_taken: 'That name is already used.',
  tag_taken: 'That tag is already used.',
  invalid_name: 'Names are 2-32 characters.',
  invalid_tag: 'Tags are 2-5 letters or digits.',
  too_many_teams: 'You are in the maximum number of teams.',
  invalid_input: 'Please check the form and try again.',
  not_found: 'Not found.',
  forbidden: 'You are not allowed to do that.',
};
export const human = (e) => ERR[e && e.code] || `Something went wrong (${(e && e.code) || 'network error'}).`;

export async function api(method, path, body) {
  const headers = {};
  const t = tok();
  if (t) headers.Authorization = `Bearer ${t}`;
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const r = await fetch(BASE + path, { method, headers, body: body !== undefined ? JSON.stringify(body) : undefined });
  if (r.status === 204) return {};
  const j = await r.json().catch(() => ({}));
  if (!r.ok) {
    const e = new Error(j.error || `http_${r.status}`);
    e.code = j.error; e.status = r.status;
    throw e;
  }
  return j;
}

export async function refreshMe() {
  if (!tok()) { state.me = null; return null; }
  try { state.me = await api('GET', '/me'); }
  catch (e) { if (e.status === 401) setTok(null); state.me = null; }
  return state.me;
}

export function toast(text) {
  const t = h('div', { class: 'toast', role: 'status' }, text);
  document.body.append(t);
  setTimeout(() => t.remove(), 2600);
}

export const num = (n) => (typeof n === 'number' ? n.toLocaleString() : '—');
export const when = (ts) => new Date(ts * 1000).toLocaleString();
export const day = (ts) => new Date(ts * 1000).toLocaleDateString(undefined, { day: '2-digit', month: 'short', year: 'numeric' });
export function dur(secs) {
  const m = Math.floor((secs || 0) / 60);
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m`;
}

export function steamStart(ret) {
  location.href = `${BASE}/auth/steam/start?return=${encodeURIComponent(ret || location.origin + '/account')}`;
}
export const steamButton = (label = 'Sign in with Steam') =>
  h('button', { class: 'btn steam', type: 'button', onclick: () => steamStart() }, label);

export function phero(eyebrow, title, text, metas) {
  return h('section', { class: 'phero pad' }, h('div', { class: 'wrap' },
    h('span', { class: 'eyebrow' }, eyebrow), h('h1', null, title), text && h('p', null, text),
    metas && h('div', { class: 'metas' }, metas.map(([k, v]) => h('span', null, k, h('b', null, v))))));
}
export const wrap = (...kids) => h('div', { class: 'wrap pad' }, ...kids);

export function comingSoon(eyebrow, title, line, items) {
  return h('div', { class: 'coming pad' }, h('span', { class: 'eyebrow' }, eyebrow), h('h1', null, title),
    h('div', { class: 'st' }, h('span', { class: 'dot' }), 'Coming soon'), h('p', { class: 'muted' }, line),
    h('ul', null, items.map((i) => h('li', null, h('span', null, i), h('span', { class: 'muted' }, 'Planned')))),
    h('a', { class: 'btn', href: '/updates' }, 'Follow development'));
}

export function needSignIn(what) {
  return h('div', { class: 'wrap pad', style: 'padding-top:4em;padding-bottom:4em;text-align:center' },
    h('h1', null, 'Sign in'), h('p', { class: 'muted' }, `Sign in to see ${what}.`),
    h('div', { class: 'btns', style: 'justify-content:center' }, steamButton(), h('a', { class: 'btn', href: '/login' }, 'Use a password')));
}
