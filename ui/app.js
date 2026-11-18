// Webloom editor UI.
//
// Model: the source text is always the truth. The design view is the page
// served by the Rust side with `data-dw="<n>"` on every element, where <n> is
// the element's index in the parsed source. Every change made in the design
// view or the inspector is sent to the server as a source edit; the server
// returns the new source and the editor reloads the design view from it.
'use strict';

const TOKEN = location.hash.slice(1);
const $ = (sel) => document.querySelector(sel);

const state = {
  files: [],
  dirty: new Set(),   // paths with unsaved changes
  filePath: null,     // file shown in the code editor
  pagePath: null,     // HTML page shown in the design view
  elements: null,     // parsed elements of filePath (HTML only)
  sel: null,          // selected element index
  draftTimer: null,
  draftPending: null, // promise of the in-flight draft request
  quiet: false,       // true while we change the editor programmatically
  editingNode: null,  // design-view node being edited inline
};

// ---------------------------------------------------------------------------
// Server API
// ---------------------------------------------------------------------------

async function api(name, body = {}) {
  const res = await fetch('/__dw/api/' + name, {
    method: 'POST',
    headers: { 'X-DW-Token': TOKEN, 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

function status(msg, isError = false) {
  const el = $('#status');
  el.textContent = msg;
  el.classList.toggle('error', isError);
}

function fail(err) {
  console.error(err);
  status(String(err.message || err), true);
}

const isHtml = (p) => /\.html?$/i.test(p || '');
const sitePathUrl = (p) => '/' + p.split('/').map(encodeURIComponent).join('/');

// ---------------------------------------------------------------------------
// Code editor
// ---------------------------------------------------------------------------

const cm = CodeMirror($('#code'), {
  lineNumbers: true,
  lineWrapping: false,
  matchTags: { bothTags: true },
  indentUnit: 2,
  tabSize: 2,
  value: '',
});
let codeMark = null;

function modeFor(path) {
  const ext = (path.split('.').pop() || '').toLowerCase();
  if (ext === 'html' || ext === 'htm') return 'htmlmixed';
  if (ext === 'css') return 'css';
  if (ext === 'js' || ext === 'mjs' || ext === 'json') return 'javascript';
  if (ext === 'svg' || ext === 'xml') return 'xml';
  return null;
}

/** Replace the whole document, keeping scroll and (when possible) undo history. */
function setCode(text, { resetHistory = false } = {}) {
  state.quiet = true;
  const scroll = cm.getScrollInfo();
  if (resetHistory) {
    cm.setValue(text);
    cm.clearHistory();
  } else {
    const last = cm.lastLine();
    cm.replaceRange(text, { line: 0, ch: 0 }, { line: last, ch: cm.getLine(last).length });
  }
  cm.scrollTo(scroll.left, scroll.top);
  state.quiet = false;
}

cm.on('change', () => {
  if (state.quiet || !state.filePath) return;
  markDirty(state.filePath, true);
  clearTimeout(state.draftTimer);
  state.draftTimer = setTimeout(() => sendDraft().catch(fail), 350);
});

/** Push the editor content to the server so the design view can render it. */
async function sendDraft() {
  clearTimeout(state.draftTimer);
  state.draftTimer = null;
  const path = state.filePath;
  const content = cm.getValue();
  const p = api('draft', { path, content });
  state.draftPending = p;
  const res = await p;
  if (state.draftPending === p) state.draftPending = null;
  if (path !== state.filePath) return;
  if (res.elements) {
    state.elements = res.elements;
    if (state.sel != null && state.sel >= state.elements.length) state.sel = null;
  }
  reloadDesign();
}

/** Make sure the server has the latest editor content. */
async function flushDraft() {
  if (state.draftTimer) await sendDraft();
  else if (state.draftPending) await state.draftPending;
}

cm.on('cursorActivity', () => {
  if (state.quiet || !state.elements || state.draftTimer || state.draftPending) return;
  const off = cm.indexFromPos(cm.getCursor());
  const idx = elementAt(off);
  if (idx !== state.sel) select(idx, { from: 'code' });
});

/** Innermost element whose range contains the UTF-16 offset. */
function elementAt(off) {
  let best = null;
  state.elements.forEach((e, i) => {
    if (e.start <= off && off < e.end) best = i; // later = deeper (document order)
  });
  return best;
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

async function loadFiles() {
  const res = await api('files');
  state.files = res.files;
  state.dirty = new Set(res.dirty);
  renderTree();
}

function renderTree() {
  const root = { dirs: {}, files: [] };
  for (const f of state.files) {
    const parts = f.split('/');
    let node = root;
    for (const d of parts.slice(0, -1)) node = node.dirs[d] ??= { dirs: {}, files: [] };
    node.files.push(f);
  }
  const build = (node) => {
    const frag = document.createDocumentFragment();
    for (const name of Object.keys(node.dirs).sort()) {
      const det = document.createElement('details');
      det.open = true;
      const sum = document.createElement('summary');
      sum.textContent = name;
      det.append(sum, build(node.dirs[name]));
      frag.append(det);
    }
    for (const f of node.files) {
      const div = document.createElement('div');
      div.className = 'file';
      div.textContent = f.split('/').pop();
      div.title = f;
      div.dataset.path = f;
      div.classList.toggle('active', f === state.filePath);
      div.classList.toggle('dirty', state.dirty.has(f));
      div.onclick = () => openFile(f).catch(fail);
      frag.append(div);
    }
    return frag;
  };
  $('#file-tree').replaceChildren(build(root));
}

function markDirty(path, on) {
  if (on) state.dirty.add(path); else state.dirty.delete(path);
  document.querySelectorAll('.file').forEach((d) => d.classList.toggle('dirty', state.dirty.has(d.dataset.path)));
  $('#file-name').classList.toggle('dirty', state.dirty.has(state.filePath));
}

async function openFile(path) {
  if (state.filePath) await flushDraft();
  const res = await api('open', { path });
  state.filePath = path;
  state.elements = res.elements || null;
  state.sel = null;
  cm.setOption('mode', modeFor(path));
  setCode(res.content, { resetHistory: true });
  $('#file-name').textContent = path;
  markDirty(path, state.dirty.has(path));
  document.querySelectorAll('.file').forEach((d) => d.classList.toggle('active', d.dataset.path === path));
  if (isHtml(path)) {
    state.pagePath = path;
    reloadDesign();
  }
  renderInspector();
  renderTagSelector();
  status('');
}

async function save() {
  if (!state.filePath) return;
  await flushDraft();
  await api('save', { path: state.filePath, content: cm.getValue() });
  markDirty(state.filePath, false);
  status('Saved ' + state.filePath);
}

async function revert() {
  if (!state.filePath) return;
  if (!confirm('Discard unsaved changes to ' + state.filePath + '?')) return;
  clearTimeout(state.draftTimer);
  state.draftTimer = null;
  await api('revert', { path: state.filePath });
  state.dirty.delete(state.filePath);
  await openFile(state.filePath);
}

// ---------------------------------------------------------------------------
// Design view
// ---------------------------------------------------------------------------

const frame = $('#design');

function reloadDesign() {
  $('#design-empty').style.display = state.pagePath ? 'none' : '';
  if (!state.pagePath) return;
  const doc = frame.contentDocument;
  const win = frame.contentWindow;
  // Keep scroll position across reloads of the same page.
  if (doc && win && frame.dataset.path === state.pagePath) {
    frame.dataset.scrollX = win.scrollX;
    frame.dataset.scrollY = win.scrollY;
  } else {
    frame.dataset.scrollX = frame.dataset.scrollY = 0;
  }
  frame.dataset.path = state.pagePath;
  frame.src = sitePathUrl(state.pagePath) + '?__dw=' + Date.now();
}

frame.addEventListener('load', () => {
  const doc = frame.contentDocument;
  if (!doc || !doc.documentElement) return;
  frame.contentWindow.scrollTo(+frame.dataset.scrollX || 0, +frame.dataset.scrollY || 0);
  state.editingNode = null;

  const style = doc.createElement('style');
  style.textContent =
    '[data-dw-hover]{outline:1px dashed #3b82f6 !important;outline-offset:-1px}' +
    '[data-dw-sel]{outline:2px solid #3b82f6 !important;outline-offset:-2px}' +
    '[contenteditable=true]{outline:2px solid #f59e0b !important;cursor:text}';
  (doc.head || doc.documentElement).append(style);

  let hovered = null;
  doc.addEventListener('mouseover', (e) => {
    const el = e.target.closest && e.target.closest('[data-dw]');
    if (hovered) hovered.removeAttribute('data-dw-hover');
    hovered = el;
    if (el) el.setAttribute('data-dw-hover', '');
  });
  doc.addEventListener('mouseleave', () => hovered && hovered.removeAttribute('data-dw-hover'));

  // Capture phase: the page's own handlers never see editor clicks,
  // and links/forms don't navigate.
  doc.addEventListener('click', (e) => {
    if (state.editingNode && state.editingNode.contains(e.target)) return;
    e.preventDefault();
    e.stopPropagation();
    const el = e.target.closest('[data-dw]');
    if (el) selectFromDesign(+el.dataset.dw).catch(fail);
  }, true);
  doc.addEventListener('submit', (e) => e.preventDefault(), true);
  doc.addEventListener('dblclick', (e) => {
    e.preventDefault();
    const el = e.target.closest('[data-dw]');
    if (el) startInlineEdit(el);
  }, true);
  doc.addEventListener('keydown', onDesignKey, true);

  if (state.sel != null && frame.dataset.path === state.filePath) highlightDesign(state.sel, false);
});

async function selectFromDesign(idx) {
  // Clicking the design view while editing a CSS/JS file jumps back to the page.
  if (state.filePath !== state.pagePath) {
    const keep = idx;
    await openFile(state.pagePath);
    await new Promise((r) => frame.addEventListener('load', r, { once: true }));
    idx = keep;
  }
  await flushDraft();
  select(idx, { from: 'design' });
}

function highlightDesign(idx, scroll) {
  const doc = frame.contentDocument;
  if (!doc) return null;
  doc.querySelectorAll('[data-dw-sel]').forEach((n) => n.removeAttribute('data-dw-sel'));
  if (idx == null) return null;
  const node = doc.querySelector(`[data-dw="${idx}"]`);
  if (node) {
    node.setAttribute('data-dw-sel', '');
    if (scroll) node.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }
  return node;
}

function designNode(idx) {
  const doc = frame.contentDocument;
  return doc && idx != null ? doc.querySelector(`[data-dw="${idx}"]`) : null;
}

function startInlineEdit(node) {
  const idx = +node.dataset.dw;
  const e = state.elements && state.elements[idx];
  if (!e || !e.text_only) {
    status('Only elements that contain plain text can be edited in place. Use the code view for the rest.');
    return;
  }
  state.editingNode = node;
  const before = node.textContent;
  node.contentEditable = 'true';
  node.focus();
  const finish = (commit) => {
    node.removeEventListener('blur', onBlur);
    node.removeEventListener('keydown', onKey);
    node.contentEditable = 'false';
    state.editingNode = null;
    const text = node.textContent;
    if (commit && text !== before) applyEdit('text', idx, { text }).catch(fail);
    else if (!commit) node.textContent = before;
  };
  const onBlur = () => finish(true);
  const onKey = (ev) => {
    if (ev.key === 'Enter' && !ev.shiftKey) { ev.preventDefault(); node.blur(); }
    if (ev.key === 'Escape') { ev.preventDefault(); finish(false); }
    ev.stopPropagation();
  };
  node.addEventListener('blur', onBlur);
  node.addEventListener('keydown', onKey);
}

function onDesignKey(ev) {
  if (handleShortcut(ev)) return;
  if (state.editingNode) return;
  if ((ev.key === 'Delete' || ev.key === 'Backspace') && state.sel != null) {
    ev.preventDefault();
    deleteSelected();
  }
}

// ---------------------------------------------------------------------------
// Selection: keeps code, design, inspector and tag selector in sync
// ---------------------------------------------------------------------------

function select(idx, { from } = {}) {
  state.sel = idx;
  if (codeMark) { codeMark.clear(); codeMark = null; }
  const e = idx != null && state.elements ? state.elements[idx] : null;
  if (e) {
    const a = cm.posFromIndex(e.start);
    const b = cm.posFromIndex(e.end);
    codeMark = cm.markText(a, b, { className: 'dw-code-sel' });
    if (from !== 'code') {
      state.quiet = true;
      cm.setSelection(a, cm.posFromIndex(e.open_end));
      state.quiet = false;
      cm.scrollIntoView({ line: a.line, ch: 0 }, 80);
    }
  }
  if (state.filePath === state.pagePath) highlightDesign(idx, from !== 'design');
  renderInspector();
  renderTagSelector();
}

function renderTagSelector() {
  const bar = $('#tag-selector');
  bar.replaceChildren();
  if (!state.elements || state.sel == null) return;
  const chain = [];
  for (let i = state.sel; i != null; i = state.elements[i].parent) chain.unshift(i);
  for (const i of chain) {
    const span = document.createElement('span');
    span.textContent = '<' + label(state.elements[i]) + '>';
    span.classList.toggle('cur', i === state.sel);
    span.onclick = () => select(i, { from: 'tags' });
    bar.append(span);
  }
}

function attr(e, name) {
  const a = e.attrs.find(([n]) => n.toLowerCase() === name);
  return a ? a[1] ?? '' : null;
}

function label(e) {
  let s = e.tag;
  const id = attr(e, 'id');
  const cls = attr(e, 'class');
  if (id) s += '#' + id;
  if (cls) s += '.' + cls.trim().split(/\s+/).join('.');
  return s;
}

// ---------------------------------------------------------------------------
// Edits (all go through the server so the source stays the truth)
// ---------------------------------------------------------------------------

async function applyEdit(op, index, extra = {}) {
  if (state.filePath !== state.pagePath || !state.elements) return;
  await flushDraft();
  const res = await api('edit', { path: state.filePath, content: cm.getValue(), index, op, ...extra });
  setCode(res.content);
  state.elements = res.elements;
  markDirty(state.filePath, true);
  let next = index;
  if (res.select != null) next = res.select;
  else if (op === 'delete') next = null;
  state.sel = next;
  reloadDesign();
  select(next, { from: 'edit' });
}

function deleteSelected() {
  if (state.sel == null) return;
  const e = state.elements[state.sel];
  if (['html', 'head', 'body'].includes(e.tag)) return status('Refusing to delete <' + e.tag + '>.', true);
  const parent = e.parent;
  applyEdit('delete', state.sel).then(() => parent != null && select(parent, { from: 'edit' })).catch(fail);
}

function setAttrs(idx, attrs) {
  return applyEdit('attrs', idx, { attrs }).catch(fail);
}

/** Copy of the element's attribute list with `name` set (null removes it). */
function withAttr(e, name, value) {
  const out = e.attrs.map(([n, v]) => [n, v]);
  const i = out.findIndex(([n]) => n.toLowerCase() === name.toLowerCase());
  if (value === null) { if (i >= 0) out.splice(i, 1); }
  else if (i >= 0) out[i][1] = value;
  else out.push([name, value]);
  return out;
}

// --- inline style helpers ---------------------------------------------------

function parseStyle(s) {
  const out = [];
  for (const part of (s || '').split(';')) {
    const c = part.indexOf(':');
    if (c < 0) continue;
    const k = part.slice(0, c).trim().toLowerCase();
    const v = part.slice(c + 1).trim();
    if (k) out.push([k, v]);
  }
  return out;
}

const styleString = (list) => list.filter(([, v]) => v !== '').map(([k, v]) => `${k}: ${v}`).join('; ');

// ---------------------------------------------------------------------------
// Inspector
// ---------------------------------------------------------------------------

const TAG_ATTRS = {
  a: ['href', 'target', 'title', 'rel'],
  img: ['src', 'alt', 'width', 'height', 'loading'],
  link: ['rel', 'href'],
  script: ['src', 'type', 'defer'],
  input: ['type', 'name', 'value', 'placeholder'],
  textarea: ['name', 'placeholder', 'rows'],
  button: ['type', 'name'],
  form: ['action', 'method'],
  iframe: ['src', 'width', 'height', 'title'],
  video: ['src', 'poster', 'controls', 'autoplay', 'loop', 'muted'],
  audio: ['src', 'controls'],
  source: ['src', 'type', 'srcset', 'media'],
  meta: ['name', 'content', 'charset'],
  label: ['for'],
  td: ['colspan', 'rowspan'],
  th: ['colspan', 'rowspan', 'scope'],
};
const BOOLEAN_ATTRS = new Set(['controls', 'autoplay', 'loop', 'muted', 'defer', 'disabled', 'checked', 'required', 'hidden']);
const STYLE_PROPS = [
  'display', 'position', 'width', 'height', 'margin', 'padding',
  'color', 'background', 'font-family', 'font-size', 'font-weight', 'text-align',
  'border', 'border-radius', 'flex-direction', 'gap', 'justify-content', 'align-items',
];

function el(tag, props = {}, ...children) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k.startsWith('on')) n.addEventListener(k.slice(2), v);
    else if (k in n) n[k] = v;
    else n.setAttribute(k, v);
  }
  n.append(...children);
  return n;
}

function section(title, ...children) {
  return el('div', { className: 'sec' }, el('div', { className: 'sec-title' }, title), ...children);
}

function row(name, input) {
  return el('div', { className: 'row' }, el('label', { title: name }, name), input);
}

function renderInspector() {
  const box = $('#props');
  box.replaceChildren();
  const e = state.sel != null && state.elements ? state.elements[state.sel] : null;
  if (!e) {
    box.className = 'muted';
    box.textContent = state.elements
      ? 'Select an element in the design or code view.'
      : 'The inspector works on HTML files.';
    return;
  }
  box.className = '';
  const idx = state.sel;
  const node = designNode(idx);
  const computed = node ? node.ownerDocument.defaultView.getComputedStyle(node) : null;

  box.append(el('div', { className: 'tag-title' }, '<' + e.tag + '>'));

  // Common attributes
  const attrInput = (name) => {
    if (BOOLEAN_ATTRS.has(name)) {
      const cb = el('input', { type: 'checkbox', checked: attr(e, name) !== null, style: 'width:auto' });
      cb.onchange = () => setAttrs(idx, cb.checked ? withAttrBool(e, name) : withAttr(e, name, null));
      return cb;
    }
    const inp = el('input', { value: attr(e, name) ?? '', spellcheck: false });
    inp.onchange = () => setAttrs(idx, withAttr(e, name, inp.value === '' ? null : inp.value));
    return inp;
  };
  const common = ['id', 'class', ...(TAG_ATTRS[e.tag] || [])];
  box.append(section('Element', ...common.map((n) => row(n, attrInput(n)))));

  // Text
  if (e.text_only) {
    const ta = el('textarea', { value: node ? node.textContent : '' });
    ta.onchange = () => applyEdit('text', idx, { text: ta.value }).catch(fail);
    box.append(section('Text', ta, el('div', { className: 'muted' }, 'Tip: double-click the element in the design view to edit in place.')));
  }

  // Inline style
  const styles = parseStyle(attr(e, 'style'));
  const styleRows = STYLE_PROPS.map((prop) => {
    const cur = styles.find(([k]) => k === prop);
    const inp = el('input', { value: cur ? cur[1] : '', placeholder: computed ? computed.getPropertyValue(prop) : '', spellcheck: false });
    inp.onchange = () => {
      const list = styles.filter(([k]) => k !== prop);
      if (inp.value.trim()) {
        const at = styles.findIndex(([k]) => k === prop);
        list.splice(at >= 0 ? at : list.length, 0, [prop, inp.value.trim()]);
      }
      const s = styleString(list);
      setAttrs(idx, withAttr(e, 'style', s === '' ? null : s));
    };
    return row(prop, inp);
  });
  const extra = styles.filter(([k]) => !STYLE_PROPS.includes(k));
  const extraInp = el('input', { value: styleString(extra), placeholder: 'other: value; …', spellcheck: false });
  extraInp.onchange = () => {
    const list = styles.filter(([k]) => STYLE_PROPS.includes(k)).concat(parseStyle(extraInp.value));
    const s = styleString(list);
    setAttrs(idx, withAttr(e, 'style', s === '' ? null : s));
  };
  box.append(section('Inline style (grey = computed value)', ...styleRows, row('other', extraInp)));

  // All other attributes
  const shown = new Set([...common, 'style']);
  const others = e.attrs.filter(([n]) => !shown.has(n.toLowerCase()));
  const otherRows = others.map(([n, v]) => {
    const val = el('input', { value: v ?? '', spellcheck: false });
    val.onchange = () => setAttrs(idx, withAttr(e, n, val.value));
    const del = el('button', { title: 'Remove attribute', onclick: () => setAttrs(idx, withAttr(e, n, null)) }, '×');
    return el('div', { className: 'attr-row' }, el('label', { title: n }, n), val, del);
  });
  const newName = el('input', { placeholder: 'name', spellcheck: false });
  const newVal = el('input', { placeholder: 'value', spellcheck: false });
  const add = el('button', { title: 'Add attribute', onclick: () => {
    if (newName.value.trim()) setAttrs(idx, withAttr(e, newName.value.trim(), newVal.value));
  } }, '+');
  box.append(section('Attributes', ...otherRows, el('div', { className: 'attr-row' }, newName, newVal, add)));

  // Actions
  const actions = el('div', { className: 'actions' });
  if (e.parent != null) actions.append(el('button', { onclick: () => select(e.parent, { from: 'tags' }) }, 'Select parent'));
  if (!['html', 'head', 'body'].includes(e.tag)) actions.append(el('button', { onclick: deleteSelected }, 'Delete element'));
  box.append(section('Actions', actions));
}

function withAttrBool(e, name) {
  const out = e.attrs.map(([n, v]) => [n, v]);
  if (!out.some(([n]) => n.toLowerCase() === name)) out.push([name, null]);
  return out;
}

// ---------------------------------------------------------------------------
// Insert panel
// ---------------------------------------------------------------------------

const SNIPPETS = [
  ['Div', '<div></div>'],
  ['Section', '<section>\n  <h2>Section title</h2>\n</section>'],
  ['Heading 1', '<h1>Heading</h1>'],
  ['Heading 2', '<h2>Heading</h2>'],
  ['Paragraph', '<p>New paragraph</p>'],
  ['Link', '<a href="#">Link text</a>'],
  ['Image', '<img src="" alt="">'],
  ['Button', '<button type="button">Button</button>'],
  ['List', '<ul>\n  <li>Item</li>\n  <li>Item</li>\n</ul>'],
  ['Table', '<table>\n  <tr><th>Header</th><th>Header</th></tr>\n  <tr><td>Cell</td><td>Cell</td></tr>\n</table>'],
  ['Form', '<form action="" method="post">\n  <label>Name <input type="text" name="name"></label>\n  <button type="submit">Send</button>\n</form>'],
  ['Line', '<hr>'],
];

function renderInsertPanel() {
  const panel = $('#insert-panel');
  for (const [name, html] of SNIPPETS) {
    panel.append(el('button', { onclick: () => insertSnippet(html).catch(fail) }, name));
  }
}

async function insertSnippet(html) {
  if (!state.elements || state.filePath !== state.pagePath) {
    return status('Open an HTML page to insert elements.', true);
  }
  let idx = state.sel;
  let inside = $('#insert-inside').checked;
  if (idx == null || ['html', 'body'].includes(state.elements[idx].tag)) {
    idx = state.elements.findIndex((e) => e.tag === 'body');
    inside = true;
    if (idx < 0) return status('No <body> to insert into.', true);
  }
  if (state.elements[idx].tag === 'head') return status('Select an element inside <body> first.', true);
  await applyEdit('insert', idx, { html, inside });
}

// ---------------------------------------------------------------------------
// Layout, shortcuts, startup
// ---------------------------------------------------------------------------

function setView(view) {
  const ws = $('#workspace');
  ws.classList.remove('code', 'split', 'design');
  ws.classList.add(view);
  document.querySelectorAll('#view-modes button').forEach((b) => b.classList.toggle('on', b.dataset.view === view));
  $('#split-dir').style.display = view === 'split' ? '' : 'none';
  cm.refresh();
  try { localStorage.setItem('webloom.view', view); } catch {}
}

function setSplit(dir) {
  const ws = $('#workspace');
  ws.classList.remove('rows', 'cols');
  ws.classList.add(dir);
  document.querySelectorAll('#split-dir button').forEach((b) => b.classList.toggle('on', b.dataset.dir === dir));
  cm.refresh();
  try { localStorage.setItem('webloom.split', dir); } catch {}
}

function handleShortcut(ev) {
  const mod = ev.ctrlKey || ev.metaKey;
  if (mod && ev.key.toLowerCase() === 's') {
    ev.preventDefault();
    save().catch(fail);
    return true;
  }
  return false;
}

async function start() {
  document.querySelectorAll('#view-modes button').forEach((b) => (b.onclick = () => setView(b.dataset.view)));
  document.querySelectorAll('#split-dir button').forEach((b) => (b.onclick = () => setSplit(b.dataset.dir)));
  $('#btn-save').onclick = () => save().catch(fail);
  $('#btn-revert').onclick = () => revert().catch(fail);
  window.addEventListener('keydown', handleShortcut);
  window.addEventListener('beforeunload', (e) => { if (state.dirty.size) e.preventDefault(); });
  renderInsertPanel();

  let view = 'split', split = 'cols';
  try { view = localStorage.getItem('webloom.view') || view; split = localStorage.getItem('webloom.split') || split; } catch {}
  setView(view);
  setSplit(split);

  const info = await api('info');
  $('#site-name').textContent = info.name;
  document.title = info.name + ' — Webloom';
  await loadFiles();
  const first = state.files.find((f) => /^index\.html?$/i.test(f)) || state.files.find(isHtml);
  if (first) await openFile(first);
}

start().catch(fail);
