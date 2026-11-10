/* ============================================================
   Tabnest 前端脚本 —— 原生 JavaScript，无构建步骤。
   整个文件包在一个 IIFE 里，避免污染全局命名空间。
   ============================================================ */

(() => {
  'use strict';

  // 选择器简写：$ 取第一个匹配元素，$$ 取全部并转成数组。
  const $ = (selector, root = document) => root.querySelector(selector);
  const $$ = (selector, root = document) => Array.from(root.querySelectorAll(selector));

  // 内联 SVG 图标表，避免额外请求图标字体或图片资源。
  const SVG = {
    plus: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>',
    more: '<svg viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="1.7"/><circle cx="12" cy="12" r="1.7"/><circle cx="19" cy="12" r="1.7"/></svg>',
    pencil:
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.6 3.6a2.2 2.2 0 0 1 3.1 3.1L7 19.4 3 20.5l1.1-4Z"/></svg>',
    trash:
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6h16"/><path d="M9 6V4h6v2"/><path d="M18 6l-1 14H7L6 6"/><path d="M10 11v5M14 11v5"/></svg>',
    external:
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M14 4h6v6"/><path d="M20 4 10 14"/><path d="M20 14v6H4V4h6"/></svg>',
    menu: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M4 7h16M4 12h16M4 17h16"/></svg>',
    x: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="m6 6 12 12M18 6 6 18"/></svg>',
    sun: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="12" cy="12" r="4.2"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M19.1 4.9l-1.4 1.4M6.3 17.7l-1.4 1.4"/></svg>',
    moon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M20.5 14.4A8.6 8.6 0 0 1 9.6 3.5a8.6 8.6 0 1 0 10.9 10.9Z"/></svg>',
    download:
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M4 19h16"/></svg>',
    upload:
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 16V4"/><path d="m7 9 5-5 5 5"/><path d="M4 20h16"/></svg>',
    grid: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="18" rx="2"/><rect x="14" y="3" width="7" height="10" rx="2"/></svg>',
    folder:
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/></svg>',
    check:
      '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.2 4.2L19 6.5"/></svg>',
  };

  // 可选配色；id 会写进 <html data-palette>，由 CSS 变量决定具体颜色。
  const PALETTES = [
    { id: 'clay', name: '陶土' },
    { id: 'moss', name: '苔绿' },
    { id: 'ocean', name: '海蓝' },
    { id: 'plum', name: '紫藤' },
    { id: 'sakura', name: '樱粉' },
    { id: 'amber', name: '琥珀' },
  ];

  // 亮度模式；auto 表示跟随系统的 prefers-color-scheme。
  const MODES = [
    { id: 'light', name: '浅色' },
    { id: 'dark', name: '深色' },
    { id: 'auto', name: '跟随系统' },
  ];

  // 每行分组列数；auto 表示按窗口宽度自动计算。
  const COLUMN_OPTIONS = [
    { id: 'auto', name: '自动' },
    { id: '2', name: '2' },
    { id: '3', name: '3' },
    { id: '4', name: '4' },
    { id: '5', name: '5' },
    { id: '6', name: '6' },
    { id: '7', name: '7' },
    { id: '8', name: '8' },
  ];

  // 全局运行时状态：面板数据、当前选中项、搜索词与外观偏好。
  const state = {
    boards: [],
    activeId: null,
    query: '',
    authRequired: false,
    version: '0.1.0',
    menuEl: null,
    mode: 'auto',
    palette: 'clay',
    columns: 'auto',
    appearanceEl: null,
  };

  // 拖拽过程中的临时状态，dragend / drop 后统一清空。
  const drag = {
    payload: null,
    el: null,
    overCard: null,
    overColumn: null,
    overColumnEl: null,
    overVisualCol: null,
    overBoard: null,
    suppressClickUntil: 0,
    lastX: null,
    lastY: null,
    scrollFrame: 0,
  };

  // 启动时缓存常用 DOM 节点，避免渲染逻辑里反复查询。
  const els = {
    sidebar: $('#sidebar'),
    scrim: $('#scrim'),
    boardList: $('#board-list'),
    search: $('#search-input'),
    mainHead: $('#main-head'),
    boardView: $('#board-view'),
    modalLayer: $('#modal-layer'),
    toasts: $('#toasts'),
    loginLayer: $('#login-layer'),
    loginForm: $('#login-form'),
    loginPassword: $('#login-password'),
    loginError: $('#login-error'),
    loginSubmit: $('#login-submit'),
    version: $('#app-version'),
    themeBtn: $('#btn-theme'),
    logoutBtn: $('#btn-logout'),
  };

  /* ----------------------------------------------------------
     通用工具
     ---------------------------------------------------------- */

  // HTML 转义表：拼进 innerHTML 的所有用户数据都必须先经过 esc()。
  const ESCAPE_MAP = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
  const esc = (value) => String(value ?? '').replace(/[&<>"']/g, (ch) => ESCAPE_MAP[ch]);
  const attr = esc;

  // 取名称首个字母/数字作为占位图标，空名称退化成「·」。
  function initial(text) {
    const cleaned = String(text || '').trim();
    if (!cleaned) return '·';
    const match = cleaned.match(/[\p{L}\p{N}]/u);
    return (match ? match[0] : cleaned[0]).toUpperCase();
  }

  // 由字符串散列出 6 种色块之一，保证同名面板/书签颜色稳定。
  function tint(text) {
    let hash = 0;
    for (const ch of String(text || '')) {
      hash = (hash * 31 + ch.codePointAt(0)) >>> 0;
    }
    return `tint-${hash % 6}`;
  }

  // 由分组名散列出 9 种分类色，用于分组卡片的左侧色条。
  function categoryColor(text) {
    let hash = 0;
    for (const ch of String(text || '')) {
      hash = (hash * 31 + ch.codePointAt(0)) >>> 0;
    }
    return `cat-${hash % 9}`;
  }

  // 从 URL 取主机名并去掉 www. 前缀；解析失败时原样返回。
  function hostOf(url) {
    try {
      return new URL(url).host.replace(/^www\./, '');
    } catch (error) {
      return url;
    }
  }

  // 客户端 URL 规整：缺协议时补 https://，非 http(s) 视为无效。
  function normalizeClient(raw) {
    const value = String(raw || '').trim();
    if (!value) return '';
    if (/^https?:\/\//i.test(value)) return value;
    if (value.includes('://')) return '';
    return `https://${value}`;
  }

  // 判断两个地址是否指向同一 URL（用于确认抓到的图标仍属于该站点）。
  function sameTarget(left, right) {
    try {
      return new URL(normalizeClient(left)).href === new URL(normalizeClient(right)).href;
    } catch (error) {
      return false;
    }
  }

  // 轻量提示条，2.8 秒后自动淡出并移除。
  function toast(message, type = 'ok') {
    const el = document.createElement('div');
    el.className = type === 'error' ? 'toast error' : 'toast';
    el.textContent = message;
    els.toasts.appendChild(el);
    window.setTimeout(() => {
      el.classList.add('leaving');
      window.setTimeout(() => el.remove(), 220);
    }, 2800);
  }

  // 统一请求封装：自动带 Cookie、按 Content-Type 解析，401 时弹出登录层。
  async function api(path, options = {}) {
    const init = {
      method: options.method || 'GET',
      credentials: 'same-origin',
      headers: {},
    };
    if (options.raw !== undefined) {
      init.headers['Content-Type'] = 'text/plain; charset=utf-8';
      init.body = options.raw;
    } else if (options.body !== undefined) {
      init.headers['Content-Type'] = 'application/json';
      init.body = JSON.stringify(options.body);
    }

    const response = await fetch(path, init);
    if (response.status === 401 && !path.startsWith('/api/login')) {
      showLogin();
      throw new Error('请先登录');
    }

    const contentType = response.headers.get('content-type') || '';
    let data = null;
    if (contentType.includes('application/json')) {
      data = await response.json();
    } else {
      data = await response.text();
    }
    if (!response.ok) {
      const message = (data && typeof data === 'object' && data.error) || `请求失败（${response.status}）`;
      throw new Error(message);
    }
    return data;
  }

  /* ----------------------------------------------------------
     启动与会话
     ---------------------------------------------------------- */

  // 启动流程：绑定事件 -> 应用外观 -> 查询会话 -> 同步设置 -> 加载数据。
  async function boot() {
    bindStatic();
    const params = new URLSearchParams(window.location.search);
    const requestedTheme = params.get('theme');
    const requestedPalette = params.get('palette');
    const storedTheme = localStorage.getItem('tabnest.theme');
    const storedPalette = localStorage.getItem('tabnest.palette');
    const storedColumns = localStorage.getItem('tabnest.columns');

    const mode = MODES.some((item) => item.id === requestedTheme)
      ? requestedTheme
      : (MODES.some((item) => item.id === storedTheme) ? storedTheme : 'auto');
    const palette = PALETTES.some((item) => item.id === requestedPalette)
      ? requestedPalette
      : (PALETTES.some((item) => item.id === storedPalette) ? storedPalette : 'clay');

    applyAppearance(mode, palette, !requestedTheme && !requestedPalette);
    applyColumns(
      COLUMN_OPTIONS.some((item) => item.id === storedColumns) ? storedColumns : 'auto',
      false
    );

    try {
      const session = await api('/api/session');
      state.authRequired = Boolean(session.auth_required);
      state.version = session.version || '0.1.0';
      els.version.textContent = `v${state.version}`;
      els.logoutBtn.classList.toggle('hidden', !state.authRequired);

      if (session.auth_required && !session.authenticated) {
        showLogin();
        return;
      }
      await syncSettingsFromServer();
      await loadState();
      $('#boot')?.remove();
    } catch (error) {
      $('#boot')?.remove();
      toast(error.message || '无法连接到服务器', 'error');
    }
  }

  // HTTP 下浏览器不会保存 Secure Cookie，这里显式提示改用 HTTPS。
  function updateLoginSchemeHint() {
    const hint = $('#login-hint');
    if (!hint) return;
    const insecure = window.location.protocol !== 'https:';
    hint.hidden = !insecure;
    if (insecure) {
      hint.textContent =
        '当前通过 HTTP 访问，浏览器会拒绝保存安全登录 Cookie。请改用 HTTPS 域名访问后再登录。';
    }
  }

  // 显示登录遮罩并把焦点放到密码框。
  function showLogin() {
    closeMenu();
    closeAppearance();
    if (!els.modalLayer.hidden) closeModal();
    els.loginLayer.hidden = false;
    updateLoginSchemeHint();
    $('#boot')?.remove();
    document.title = '登录 · Tabnest';
    window.setTimeout(() => els.loginPassword?.focus(), 60);
  }

  // 提交密码；成功后重新同步设置与数据。
  async function onLogin(event) {
    event.preventDefault();
    els.loginError.textContent = '';
    updateLoginSchemeHint();
    const password = els.loginPassword.value;
    if (!password) return;

    els.loginSubmit.disabled = true;
    els.loginSubmit.textContent = '验证中…';
    try {
      await api('/api/login', { method: 'POST', body: { password } });
      els.loginLayer.hidden = true;
      els.loginPassword.value = '';
      await syncSettingsFromServer();
      await loadState();
      $('#boot')?.remove();
    } catch (error) {
      const insecure = window.location.protocol !== 'https:';
      els.loginError.textContent =
        error.message === '请先登录'
          ? insecure
            ? '登录成功，但 HTTP 下无法保存安全 Cookie；请改用 HTTPS 域名。'
            : '登录成功但会话未保存，请确认浏览器允许保存 Cookie，或换一个无痕窗口重试。'
          : error.message;
      els.loginPassword.select();
    } finally {
      els.loginSubmit.disabled = false;
      els.loginSubmit.textContent = '进入';
    }
  }

  // 退出登录：无论请求是否成功，本地界面与状态先清空。
  async function logout() {
    try {
      await api('/api/logout', { method: 'POST' });
    } catch (error) {
      /* 无论请求成功与否，本地状态都要清空 */
    }
    state.boards = [];
    state.activeId = null;
    state.query = '';
    els.search.value = '';
    els.boardList.innerHTML = '';
    els.mainHead.innerHTML = '';
    els.boardView.innerHTML = '';
    showLogin();
    toast('已退出登录');
  }

  /* ----------------------------------------------------------
     外观与偏好
     ---------------------------------------------------------- */

  // 读取系统的深浅色偏好。
  function systemTheme() {
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }

  // 按 id 查配色，找不到时退回第一个（陶土）。
  function paletteById(id) {
    return PALETTES.find((item) => item.id === id) || PALETTES[0];
  }

  // 应用（并可选持久化）外观，同时更新 theme-color 与按钮提示。
  function applyAppearance(mode, palette, persist = true) {
    state.mode = MODES.some((item) => item.id === mode) ? mode : 'auto';
    state.palette = PALETTES.some((item) => item.id === palette) ? palette : 'clay';
    const theme = state.mode === 'auto' ? systemTheme() : state.mode;

    document.documentElement.dataset.theme = theme;
    document.documentElement.dataset.palette = state.palette;

    if (persist) {
      localStorage.setItem('tabnest.theme', state.mode);
      localStorage.setItem('tabnest.palette', state.palette);
      queueSettings({ theme: state.mode, palette: state.palette });
    }

    const meta = document.querySelector('meta[name="theme-color"]');
    if (meta) {
      const background = getComputedStyle(document.documentElement)
        .getPropertyValue('--bg')
        .trim();
      if (background) meta.setAttribute('content', background);
    }

    updateAppearanceButton();
    syncAppearancePanel();
  }

  // 把当前「配色 · 模式 · 列数」写进外观按钮的 title。
  function updateAppearanceButton() {
    if (!els.themeBtn) return;
    const palette = paletteById(state.palette);
    const mode = MODES.find((item) => item.id === state.mode) || MODES[2];
    const columns = state.columns === 'auto' ? '自动列数' : `${state.columns} 列`;
    els.themeBtn.title = `外观：${palette.name} · ${mode.name} · ${columns}`;
  }

  // 应用每行列数；列数变化会触发一次主区域重排。
  function applyColumns(value, persist = true) {
    const valid = COLUMN_OPTIONS.some((item) => item.id === value) ? value : 'auto';
    state.columns = valid;
    document.documentElement.dataset.columns = valid;
    if (persist) {
      localStorage.setItem('tabnest.columns', valid);
      queueSettings({ columns: valid });
    }
    updateAppearanceButton();
    syncAppearancePanel();
    if (state.boards.length && !state.query.trim()) renderMain();
  }

  /* 偏好既存 localStorage（用于首帧即时生效），也存服务端（用于多设备共享）。
     写回做防抖，因为拖拽等操作会短时间内触发大量变更。 */
  let settingsTimer = null;
  let pendingSettings = {};

  // 设置写回服务端做 700ms 防抖，并合并连续修改。
  function queueSettings(partial) {
    Object.assign(pendingSettings, partial);
    clearTimeout(settingsTimer);
    settingsTimer = setTimeout(() => {
      const body = pendingSettings;
      pendingSettings = {};
      api('/api/settings', { method: 'PUT', body }).catch(() => {
        /* 离线或已退出登录时忽略失败：localStorage 里仍有该值 */
      });
    }, 700);
  }

  const pendingLayouts = new Map();
  let layoutTimer = null;

  // 版式写回按面板合并、900ms 防抖，拖拽时不会打出大量请求。
  function queueLayout(boardId, visual) {
    pendingLayouts.set(String(boardId), visual);
    clearTimeout(layoutTimer);
    layoutTimer = setTimeout(() => {
      const entries = Array.from(pendingLayouts.entries());
      pendingLayouts.clear();
      for (const [id, layout] of entries) {
        api(`/api/settings/layouts/${id}`, { method: 'PUT', body: { layout } }).catch(() => {});
      }
    }, 900);
  }

  // 拉取服务端共享偏好并灌进 localStorage，
  // 这样已有的渲染路径（包括首帧前的主题脚本）无需任何改动。
  // 把服务端设置灌进 localStorage，让已有的渲染/预绘制逻辑无需改动。
  async function syncSettingsFromServer() {
    try {
      const settings = await api('/api/settings');

      // 必须在任何渲染发生之前灌入瀑布流版式，
      // 否则首帧会重新推导一套排版，甚至把它缓存回本地。
      for (const [boardId, layout] of Object.entries(settings.layouts || {})) {
        if (Array.isArray(layout)) {
          localStorage.setItem(`tabnest.layout.${boardId}`, JSON.stringify(layout));
        }
      }

      if (settings.theme || settings.palette) {
        applyAppearance(settings.theme || state.mode, settings.palette || state.palette, true);
      }
      if (settings.columns) applyColumns(settings.columns, true);
      const activeBoard = Number(settings.active_board);
      if (Number.isFinite(activeBoard) && activeBoard > 0) {
        localStorage.setItem('tabnest.board', String(activeBoard));
      }

      // 一次性迁移：把只存在于本地的旧偏好补推到服务端，
      // 服务端已有的字段不动。
      const missing = {};
      if (!settings.theme) {
        const value = localStorage.getItem('tabnest.theme');
        if (value) missing.theme = value;
      }
      if (!settings.palette) {
        const value = localStorage.getItem('tabnest.palette');
        if (value) missing.palette = value;
      }
      if (!settings.columns) {
        const value = localStorage.getItem('tabnest.columns');
        if (value) missing.columns = value;
      }
      if (Object.keys(missing).length) queueSettings(missing);

      const knownLayouts = settings.layouts || {};
      for (let index = 0; index < localStorage.length; index += 1) {
        const key = localStorage.key(index);
        if (!key || !key.startsWith('tabnest.layout.')) continue;
        const boardId = key.slice('tabnest.layout.'.length);
        if (knownLayouts[boardId]) continue;
        try {
          const layout = JSON.parse(localStorage.getItem(key));
          if (Array.isArray(layout)) queueLayout(boardId, layout);
        } catch {
          /* 跳过无法解析的脏数据 */
        }
      }
    } catch {
      /* 偏好是可选能力，服务端不可用时本地存储仍然有效 */
    }
  }

  // 打开外观浮层，位置贴近触发按钮且不超出视口。
  function openAppearance(anchor) {
    if (state.appearanceEl) {
      closeAppearance();
      return;
    }
    closeMenu();

    const element = document.createElement('div');
    element.className = 'appearance-pop';
    element.setAttribute('role', 'dialog');
    element.setAttribute('aria-label', '外观设置');
    element.innerHTML = `
      <div class="appearance-head">
        <strong>外观</strong>
        <button type="button" class="icon-btn" data-close-appearance aria-label="关闭">${SVG.x}</button>
      </div>
      <div class="appearance-section">
        <span class="appearance-label">亮度</span>
        <div class="segmented">
          ${MODES.map(
            (mode) =>
              `<button type="button" data-mode="${mode.id}" aria-pressed="false">${esc(mode.name)}</button>`
          ).join('')}
        </div>
      </div>
      <div class="appearance-section">
        <span class="appearance-label">配色</span>
        <div class="palette-grid">
          ${PALETTES.map(
            (palette) => `<button type="button" class="palette" data-palette="${palette.id}" aria-pressed="false" title="${attr(palette.name)}">
              <span class="swatch"></span>
              <span>${esc(palette.name)}</span>
              <span class="palette-check">${SVG.check}</span>
            </button>`
          ).join('')}
        </div>
      </div>
      <div class="appearance-section">
        <span class="appearance-label">列数</span>
        <div class="column-choices">
          ${COLUMN_OPTIONS.map(
            (option) =>
              `<button type="button" data-cols="${option.id}" aria-pressed="false" aria-label="${option.id === 'auto' ? '自动列数' : `${option.id} 列`}">${esc(option.name)}</button>`
          ).join('')}
        </div>
      </div>
      <p class="appearance-hint">外观与列数只保存在当前浏览器</p>`;

    document.body.appendChild(element);
    state.appearanceEl = element;

    const rect = anchor.getBoundingClientRect();
    const width = element.offsetWidth;
    const height = element.offsetHeight;
    let left = Math.max(12, Math.min(rect.left, window.innerWidth - width - 12));
    let top = rect.top - height - 8;
    if (top < 12) top = Math.min(rect.bottom + 8, window.innerHeight - height - 12);
    element.style.left = `${left}px`;
    element.style.top = `${top}px`;

    element.addEventListener('click', (event) => {
      if (event.target.closest('[data-close-appearance]')) {
        closeAppearance();
        return;
      }
      const modeButton = event.target.closest('[data-mode]');
      if (modeButton) {
        applyAppearance(modeButton.dataset.mode, state.palette);
        return;
      }
      const paletteButton = event.target.closest('button[data-palette]');
      if (paletteButton) {
        applyAppearance(state.mode, paletteButton.dataset.palette);
        return;
      }
      const columnsButton = event.target.closest('button[data-cols]');
      if (columnsButton) {
        applyColumns(columnsButton.dataset.cols);
      }
    });

    const outside = (event) => {
      if (element.contains(event.target) || event.target.closest('#btn-theme')) return;
      closeAppearance();
    };
    element._outside = outside;
    window.setTimeout(() => document.addEventListener('click', outside), 0);

    syncAppearancePanel();
  }

  // 关闭外观浮层并移除全局点击监听。
  function closeAppearance() {
    const element = state.appearanceEl;
    if (!element) return;
    if (element._outside) document.removeEventListener('click', element._outside);
    element.remove();
    state.appearanceEl = null;
  }

  // 把当前模式/配色/列数同步到浮层的高亮与 aria-pressed。
  function syncAppearancePanel() {
    const element = state.appearanceEl;
    if (!element) return;

    $$('[data-mode]', element).forEach((button) => {
      const active = button.dataset.mode === state.mode;
      button.classList.toggle('active', active);
      button.setAttribute('aria-pressed', String(active));
    });
    $$('[data-palette]', element).forEach((button) => {
      const active = button.dataset.palette === state.palette;
      button.classList.toggle('active', active);
      button.setAttribute('aria-pressed', String(active));
    });
    $$('[data-cols]', element).forEach((button) => {
      const active = button.dataset.cols === state.columns;
      button.classList.toggle('active', active);
      button.setAttribute('aria-pressed', String(active));
    });
  }

  /* ----------------------------------------------------------
     数据加载
     ---------------------------------------------------------- */

  // 拉取全量数据、恢复上次选中的面板，然后重绘。
  async function loadState() {
    const data = await api('/api/state');
    state.boards = Array.isArray(data.boards) ? data.boards : [];

    const stored = Number(localStorage.getItem('tabnest.board'));
    const hasActive = state.boards.some((board) => board.id === state.activeId);
    if (!hasActive) {
      state.activeId = state.boards.some((board) => board.id === stored)
        ? stored
        : state.boards[0]?.id ?? null;
    }
    persistActive();
    render();
  }

  // 静默刷新：失败时只提示，未登录场景交给登录层处理。
  async function refresh() {
    try {
      await loadState();
    } catch (error) {
      if (!state.authRequired) toast(error.message, 'error');
    }
  }

  // 记住当前面板：本地立即生效，同时异步写回服务端。
  function persistActive() {
    if (state.activeId) {
      localStorage.setItem('tabnest.board', String(state.activeId));
      queueSettings({ active_board: state.activeId });
    }
  }

  // 取当前选中的面板对象。
  function activeBoard() {
    return state.boards.find((board) => board.id === state.activeId) || null;
  }

  // 在全部面板中按 ID 查分组。
  function findColumn(columnId) {
    for (const board of state.boards) {
      for (const column of board.columns) {
        if (column.id === columnId) return { board, column };
      }
    }
    return null;
  }

  // 在全部面板中按 ID 查书签（含所属面板与分组）。
  function findLink(linkId) {
    for (const board of state.boards) {
      for (const column of board.columns) {
        const link = column.links.find((item) => item.id === linkId);
        if (link) return { board, column, link };
      }
    }
    return null;
  }

  /* ----------------------------------------------------------
     渲染
     ---------------------------------------------------------- */

  // 整体重绘：侧边栏 + 主区域。
  function render() {
    renderSidebar();
    renderMain();
  }

  // 渲染左侧面板列表，并显示每个面板的书签总数。
  function renderSidebar() {
    els.boardList.innerHTML = state.boards
      .map((board) => {
        const count = board.columns.reduce((total, column) => total + column.links.length, 0);
        return `<div class="board-item ${board.id === state.activeId ? 'active' : ''}" data-board-id="${board.id}" role="button" tabindex="0" draggable="true" title="${attr(board.name)}" aria-label="${attr(board.name)}">
          <span class="board-avatar ${tint(board.name)}">${esc(initial(board.name))}</span>
          <span class="board-name">${esc(board.name)}</span>
          <span class="board-count">${count}</span>
          <button class="board-more" data-act="board-menu" data-board-id="${board.id}" title="面板菜单" aria-label="面板菜单">${SVG.more}</button>
        </div>`;
      })
      .join('');

    if (!state.boards.length) {
      els.boardList.innerHTML = '<p class="sidebar-empty">还没有面板，点击上方「新建」开始。</p>';
    }
  }

  // 瀑布流目标列宽与列间距，用于把窗口宽度换算成列数。
  const MASONRY_TARGET = 204;
  const MASONRY_GAP = 8;

  // 每个面板的版式独立存在 localStorage 的一个 key 下。
  function layoutStorageKey(boardId) {
    return `tabnest.layout.${boardId}`;
  }

  // 计算实际视觉列数：窄屏固定 1 列，手动设置优先，否则按宽度估算。
  function visualColumnCount() {
    if (window.matchMedia('(max-width: 900px)').matches) return 1;
    if (state.columns !== 'auto') {
      const count = Number(state.columns);
      if (Number.isFinite(count) && count > 0) return count;
    }
    const width = els.boardView?.clientWidth || window.innerWidth;
    return Math.max(1, Math.floor((width + MASONRY_GAP) / (MASONRY_TARGET + MASONRY_GAP)));
  }

  // 估算分组高度，仅用于把新分组放进当前最矮的一列。
  function estimateColumnHeight(column) {
    return 44 + (column?.links?.length || 0) * 25;
  }

  // 读取并校验面板版式；列数变化时按最矮列重新分配，不丢原有顺序。
  function ensureBoardLayout(board) {
    const count = visualColumnCount();
    const byId = new Map(board.columns.map((column) => [column.id, column]));
    let saved = null;
    try {
      const parsed = JSON.parse(localStorage.getItem(layoutStorageKey(board.id)) || 'null');
      if (Array.isArray(parsed)) saved = parsed;
    } catch {
      saved = null;
    }

    let visual;
    if (saved && saved.length === count) {
      const seen = new Set();
      visual = saved.map((list) => {
        if (!Array.isArray(list)) return [];
        return list
          .map(Number)
          .filter((id) => {
            if (!byId.has(id) || seen.has(id)) return false;
            seen.add(id);
            return true;
          });
      });
    } else if (saved) {
      // 保存版式时的列数与当前不同（窗口变窄、缩放、改了列数设置）：
      // 把已保存的分组按最矮列重新分配，而不是丢弃原有排序；
      // 同时保持 localStorage 里的“标准版式”不被这次重排覆盖。
      const flat = saved
        .flat()
        .map(Number)
        .filter((id) => byId.has(id));
      visual = Array.from({ length: count }, () => []);
      for (const id of flat) {
        let target = 0;
        let shortest = Infinity;
        for (let index = 0; index < count; index += 1) {
          const height = visual[index].reduce(
            (sum, columnId) => sum + estimateColumnHeight(byId.get(columnId)),
            0
          );
          if (height < shortest) {
            shortest = height;
            target = index;
          }
        }
        visual[target].push(id);
      }
    } else {
      visual = Array.from({ length: count }, () => []);
    }

    for (const column of board.columns) {
      if (visual.some((list) => list.includes(column.id))) continue;
      let target = 0;
      let shortest = Infinity;
      for (let index = 0; index < count; index += 1) {
        const height = visual[index].reduce(
          (sum, id) => sum + estimateColumnHeight(byId.get(id)),
          0
        );
        if (height < shortest) {
          shortest = height;
          target = index;
        }
      }
      visual[target].push(column.id);
    }

    // 只有列数与保存时一致才写回；重排后的视图不能覆盖标准版式。
    if (!saved || saved.length === count) {
      const serialized = JSON.stringify(visual);
      if (localStorage.getItem(layoutStorageKey(board.id)) !== serialized) {
        localStorage.setItem(layoutStorageKey(board.id), serialized);
      }
    }
    return visual;
  }

  // 渲染主区域：搜索态、空态，或瀑布流面板。
  function renderMain() {
    if (state.query.trim()) {
      renderSearch();
      return;
    }

    const board = activeBoard();
    if (!board) {
      renderWelcome();
      return;
    }

    els.boardView.classList.remove('search-mode');
    const totalLinks = board.columns.reduce((total, column) => total + column.links.length, 0);
    document.title = `${board.name} · Tabnest`;

    els.mainHead.innerHTML = `
      <button class="icon-btn only-mobile" data-act="toggle-sidebar" aria-label="打开导航">${SVG.menu}</button>
      <div class="head-titles">
        <h1 class="board-title">${esc(board.name)}</h1>
        <p class="board-sub"><span>${board.columns.length} 个分组</span><span class="dot-sep"></span><span>${totalLinks} 个书签</span></p>
      </div>
      <div class="head-actions">
        <button class="btn primary" data-act="open-add-link" title="添加书签" aria-label="添加书签">${SVG.plus}<span>添加书签</span></button>
        <button class="btn ghost" data-act="new-column" title="新建分组" aria-label="新建分组">${SVG.grid}<span>新建分组</span></button>
        <button class="icon-btn" data-act="board-menu" data-board-id="${board.id}" aria-label="面板菜单">${SVG.more}</button>
      </div>`;

    if (!board.columns.length) {
      els.boardView.innerHTML = `<div class="empty-board">
        <span class="empty-mark">${SVG.grid}</span>
        <h2>这个面板还是空的</h2>
        <p>先建一个分组，比如「常用」「开发工具」「稍后读」，再往里添加书签。</p>
        <button class="btn primary" data-act="new-column" data-board-id="${board.id}">${SVG.plus}<span>新建分组</span></button>
      </div>`;
      return;
    }

    const visual = ensureBoardLayout(board);
    const byId = new Map(board.columns.map((column) => [column.id, column]));
    const grid = visual
      .map((ids, index) => {
        const cards = ids
          .map((id) => byId.get(id))
          .filter(Boolean)
          .map(renderColumn)
          .join('');
        return `<div class="masonry-col ${cards ? '' : 'is-empty'}" data-visual-col="${index}">${
          cards || '<div class="masonry-empty">拖到这里</div>'
        }</div>`;
      })
      .join('');
    els.boardView.innerHTML = `<div class="masonry-grid">${grid}</div>
      <button class="column-add" data-act="new-column" data-board-id="${board.id}">${SVG.plus}<span>新建分组</span></button>`;
  }

  // 渲染单个分组卡片（标题栏 + 书签列表 + 添加按钮）。
  function renderColumn(column) {
    return `<section class="column ${categoryColor(column.name)}" data-column-id="${column.id}">
      <header class="column-head" draggable="true" data-column-id="${column.id}" title="拖动排序" aria-label="拖动排序：${attr(column.name)}">
        <h2 class="column-title">${esc(column.name)}</h2>
        <span class="column-count">${column.links.length}</span>
        <div class="column-tools">
          <button class="icon-btn" data-act="add-link" data-column-id="${column.id}" title="添加书签" aria-label="添加书签">${SVG.plus}</button>
          <button class="icon-btn" data-act="column-menu" data-column-id="${column.id}" title="分组菜单" aria-label="分组菜单">${SVG.more}</button>
        </div>
      </header>
      <div class="column-body" data-column-id="${column.id}" data-drop-zone>
        ${column.links.map((link) => renderLink(link)).join('')}
        ${column.links.length === 0 ? '<p class="column-empty">把书签拖到这里，或点击下方按钮</p>' : ''}
        <button class="add-link" data-act="add-link" data-column-id="${column.id}">${SVG.plus}<span>添加书签</span></button>
      </div>
    </section>`;
  }

  // 渲染书签卡片；搜索结果里传 options.draggable=false 关闭拖拽。
  function renderLink(link, options = {}) {
    const draggable = options.draggable !== false;
    const letter = initial(link.title || link.url);
    // 导入的书签没有图标地址，退回用页面 URL：
    // 服务端会去解析 <link rel="icon">（或 /favicon.ico）并缓存结果。
    const iconSrc = link.icon || link.url;
    const icon = iconSrc
      ? `<img src="/api/favicon?u=${encodeURIComponent(iconSrc)}" alt="" loading="lazy">`
      : '';
    return `<article class="link-card" ${draggable ? 'draggable="true"' : ''} data-link-id="${link.id}" data-column-id="${link.column_id}" title="${attr(link.note || link.url)}">
      <a class="link-main" href="${attr(link.url)}" target="_blank" rel="noopener noreferrer" draggable="false">
        <span class="favicon ${tint(link.title)}" data-letter="${attr(letter)}">
          <span class="fallback">${esc(letter)}</span>${icon}
        </span>
        <span class="link-text">
          <span class="link-title">${esc(link.title)}</span>
          <span class="link-host">${esc(hostOf(link.url))}</span>
          ${link.note ? `<span class="link-note">${esc(link.note)}</span>` : ''}
        </span>
      </a>
      <div class="link-tools">
        <button type="button" data-act="edit-link" data-link-id="${link.id}" title="编辑">${SVG.pencil}</button>
        <button type="button" data-act="delete-link" data-link-id="${link.id}" title="删除">${SVG.trash}</button>
      </div>
    </article>`;
  }

  // 全量模糊搜索：标题/网址/备注/分组名/面板名都参与匹配，结果按面板分组。
  function renderSearch() {
    const rawQuery = state.query.trim();
    const query = rawQuery.toLowerCase();
    const results = [];
    for (const board of state.boards) {
      for (const column of board.columns) {
        for (const link of column.links) {
          const haystack = `${link.title} ${link.url} ${link.note} ${column.name} ${board.name}`.toLowerCase();
          if (haystack.includes(query)) results.push({ board, column, link });
        }
      }
    }

    document.title = `搜索 · Tabnest`;
    els.mainHead.innerHTML = `
      <button class="icon-btn only-mobile" data-act="toggle-sidebar" aria-label="打开导航">${SVG.menu}</button>
      <div class="head-titles">
        <h1 class="board-title">搜索 “${esc(rawQuery)}”</h1>
        <p class="board-sub">${results.length} 个结果</p>
      </div>
      <div class="head-actions">
        <button class="btn ghost" data-act="clear-search">清除搜索</button>
      </div>`;
    els.boardView.classList.add('search-mode');

    if (!results.length) {
      els.boardView.innerHTML = `<div class="empty-state"><strong>没有找到匹配的书签</strong><span>换个关键词，或者按 Esc 清除搜索。</span></div>`;
      return;
    }

    const groups = new Map();
    for (const result of results) {
      if (!groups.has(result.board.id)) groups.set(result.board.id, { board: result.board, items: [] });
      groups.get(result.board.id).items.push(result);
    }

    els.boardView.innerHTML = `<div class="search-view">${Array.from(groups.values())
      .map(
        (group) => `<section class="result-board">
          <h2 class="result-board-head">
            <span class="board-avatar ${tint(group.board.name)}">${esc(initial(group.board.name))}</span>
            <span>${esc(group.board.name)}</span>
            <span class="result-count">${group.items.length}</span>
          </h2>
          <div class="result-grid">
            ${group.items
              .map(
                ({ column, link }) => `<div class="result-cell">
                  <span class="result-column">${esc(column.name)}</span>
                  ${renderLink(link, { draggable: false })}
                </div>`
              )
              .join('')}
          </div>
        </section>`
      )
      .join('')}</div>`;
  }

  // 一个面板都没有时的欢迎页。
  function renderWelcome() {
    document.title = 'Tabnest · 个人书签面板';
    els.boardView.classList.remove('search-mode');
    els.mainHead.innerHTML = `
      <button class="icon-btn only-mobile" data-act="toggle-sidebar" aria-label="打开导航">${SVG.menu}</button>
      <div class="head-titles">
        <h1 class="board-title">欢迎使用 Tabnest</h1>
        <p class="board-sub">一个只属于你的书签起始页</p>
      </div>`;
    els.boardView.innerHTML = `<div class="empty-home">
      <span class="empty-art">${SVG.grid}</span>
      <h2>创建你的第一块面板</h2>
      <p>按主题建面板、按分组排书签。<br>所有数据都保存在自己服务器的 SQLite 文件里。</p>
      <button class="btn primary" data-act="new-board">${SVG.plus}<span>新建面板</span></button>
    </div>`;
  }

  /* ----------------------------------------------------------
     导航
     ---------------------------------------------------------- */

  // 切换面板：清空搜索、记住选择、关闭移动端侧栏。
  function selectBoard(boardId) {
    if (!boardId) return;
    state.activeId = boardId;
    state.query = '';
    els.search.value = '';
    persistActive();
    closeSidebar();
    render();
  }

  // 清空搜索并回到普通视图。
  function clearSearch() {
    state.query = '';
    els.search.value = '';
    renderMain();
  }

  // 开关移动端侧边栏（传入 force 可强制指定状态）。
  function toggleSidebar(force) {
    const open = typeof force === 'boolean' ? force : !els.sidebar.classList.contains('open');
    els.sidebar.classList.toggle('open', open);
    els.scrim.classList.toggle('show', open);
  }

  // 关闭移动端侧边栏及其遮罩。
  function closeSidebar() {
    els.sidebar.classList.remove('open');
    els.scrim.classList.remove('show');
  }

  /* ----------------------------------------------------------
     新建表单
     ---------------------------------------------------------- */

  // 新建面板：弹窗取名 -> 调接口 -> 刷新并切到新面板。
  async function startNewBoard() {
    const name = await promptDialog({
      title: '新建面板',
      label: '面板名称',
      confirmText: '创建',
    });
    if (!name) return;
    try {
      const result = await api('/api/boards', { method: 'POST', body: { name } });
      state.activeId = result.id;
      persistActive();
      await refresh();
      toast('面板已创建');
    } catch (error) {
      toast(error.message, 'error');
    }
  }

  // 在指定面板下新建分组。
  async function startNewColumn(boardId) {
    const board = state.boards.find((item) => item.id === boardId);
    if (!board) return;
    const name = await promptDialog({
      title: '新建分组',
      label: '分组名称',
      confirmText: '创建',
    });
    if (!name) return;
    try {
      await api('/api/columns', { method: 'POST', body: { board_id: board.id, name } });
      await refresh();
      toast('分组已创建');
    } catch (error) {
      toast(error.message, 'error');
    }
  }

  /* ----------------------------------------------------------
     菜单
     ---------------------------------------------------------- */

  // 关闭当前打开的上下文菜单。
  function closeMenu() {
    if (state.menuEl) {
      if (state.menuEl._outside) document.removeEventListener('click', state.menuEl._outside);
      state.menuEl.remove();
      state.menuEl = null;
    }
    $$('[data-menu-open]').forEach((el) => delete el.dataset.menuOpen);
  }

  // 通用上下文菜单：按锚点定位，点击外部自动关闭。
  function openMenu(anchor, items) {
    const wasOpen = anchor.dataset.menuOpen === '1';
    closeAppearance();
    closeMenu();
    if (wasOpen) return;

    const menu = document.createElement('div');
    menu.className = 'menu';
    menu.setAttribute('role', 'menu');
    menu.innerHTML = items
      .map(
        (item, index) =>
          `<button type="button" data-index="${index}" class="${item.danger ? 'danger' : ''}" role="menuitem">${item.icon || ''}<span>${esc(item.label)}</span></button>`
      )
      .join('');

    menu.addEventListener('click', async (event) => {
      const button = event.target.closest('button[data-index]');
      if (!button) return;
      const item = items[Number(button.dataset.index)];
      closeMenu();
      if (!item) return;
      try {
        await item.onClick();
      } catch (error) {
        toast(error.message || '操作失败', 'error');
      }
    });

    document.body.appendChild(menu);
    anchor.dataset.menuOpen = '1';
    state.menuEl = menu;

    const rect = anchor.getBoundingClientRect();
    const width = menu.offsetWidth;
    const height = menu.offsetHeight;
    let left = Math.max(10, Math.min(rect.right - width, window.innerWidth - width - 10));
    let top = rect.bottom + 6;
    if (top + height > window.innerHeight - 10) top = Math.max(10, rect.top - height - 6);
    menu.style.left = `${left}px`;
    menu.style.top = `${top}px`;

    const outside = (event) => {
      if (menu.contains(event.target)) return;
      if (event.target.closest && event.target.closest('[data-act$="-menu"]')) return;
      closeMenu();
    };
    menu._outside = outside;
    window.setTimeout(() => document.addEventListener('click', outside), 0);
  }

  // 面板菜单：重命名 / 删除。
  function openBoardMenu(anchor, boardId) {
    const board = state.boards.find((item) => item.id === boardId);
    if (!board) return;
    openMenu(anchor, [
      {
        label: '重命名面板',
        icon: SVG.pencil,
        onClick: async () => {
          const name = await promptDialog({
            title: '重命名面板',
            label: '面板名称',
            value: board.name,
          });
          if (!name) return;
          await api(`/api/boards/${board.id}`, { method: 'PATCH', body: { name } });
          await refresh();
          toast('面板已重命名');
        },
      },
      {
        label: '删除面板',
        icon: SVG.trash,
        danger: true,
        onClick: async () => {
          const total = board.columns.reduce((sum, column) => sum + column.links.length, 0);
          const ok = await confirmDialog({
            title: '删除这个面板？',
            message: `“${board.name}”里的 ${board.columns.length} 个分组、${total} 个书签都会被删除，且无法恢复。`,
            confirmText: '删除面板',
          });
          if (!ok) return;
          await api(`/api/boards/${board.id}`, { method: 'DELETE' });
          if (state.activeId === board.id) {
            state.activeId = null;
            localStorage.removeItem('tabnest.board');
          }
          await refresh();
          toast('面板已删除');
        },
      },
    ]);
  }

  // 分组菜单：添加书签 / 重命名 / 打开全部 / 删除。
  function openColumnMenu(anchor, columnId) {
    const board = activeBoard();
    const column = board?.columns.find((item) => item.id === columnId);
    if (!column) return;
    openMenu(anchor, [
      {
        label: '添加书签',
        icon: SVG.plus,
        onClick: () => openLinkDialog({ columnId: column.id }),
      },
      {
        label: '重命名分组',
        icon: SVG.pencil,
        onClick: async () => {
          const name = await promptDialog({
            title: '重命名分组',
            label: '分组名称',
            value: column.name,
          });
          if (!name) return;
          await api(`/api/columns/${column.id}`, { method: 'PATCH', body: { name } });
          await refresh();
          toast('分组已重命名');
        },
      },
      {
        label: '打开全部书签',
        icon: SVG.external,
        onClick: () => openAll(column),
      },
      {
        label: '删除分组',
        icon: SVG.trash,
        danger: true,
        onClick: async () => {
          const ok = await confirmDialog({
            title: '删除这个分组？',
            message: `“${column.name}”以及其中的 ${column.links.length} 个书签都会被删除。`,
            confirmText: '删除分组',
          });
          if (!ok) return;
          await api(`/api/columns/${column.id}`, { method: 'DELETE' });
          await refresh();
          toast('分组已删除');
        },
      },
    ]);
  }

  // 打开分组内全部书签；数量多时先确认，避免被浏览器拦截。
  async function openAll(column) {
    if (!column.links.length) {
      toast('这个分组里还没有书签');
      return;
    }
    if (column.links.length > 6) {
      const ok = await confirmDialog({
        title: '打开全部分组书签？',
        message: `将尝试打开 ${column.links.length} 个标签页，浏览器可能会拦截一部分弹窗。`,
        confirmText: '打开',
      });
      if (!ok) return;
    }
    for (const link of column.links) {
      window.open(link.url, '_blank', 'noopener');
    }
  }

  /* ----------------------------------------------------------
     弹窗
     ---------------------------------------------------------- */

  // 把弹窗 HTML 挂到遮罩层，并先关掉菜单与外观浮层。
  function mountModal(html) {
    els.modalLayer.innerHTML = html;
    els.modalLayer.hidden = false;
    closeMenu();
    closeAppearance();
  }

  // 关闭弹窗并清空内容。
  function closeModal() {
    els.modalLayer.hidden = true;
    els.modalLayer.innerHTML = '';
  }

  // 给弹窗内所有 [data-close] 与遮罩绑定关闭行为。
  function wireModalClose() {
    $$('[data-close]', els.modalLayer).forEach((button) => {
      button.addEventListener('click', closeModal);
    });
    els.modalLayer.addEventListener('click', (event) => {
      if (event.target === els.modalLayer) closeModal();
    });
  }

  // 单输入框弹窗，返回输入值；取消时返回 null。
  function promptDialog({ title, label, value = '', hint = '', confirmText = '保存' }) {
    return new Promise((resolve) => {
      mountModal(`<form class="modal" role="dialog" aria-modal="true">
        <div class="modal-head">
          <div><h2>${esc(title)}</h2>${hint ? `<p>${esc(hint)}</p>` : ''}</div>
          <button type="button" class="icon-btn" data-close aria-label="关闭">${SVG.x}</button>
        </div>
        <div class="modal-body">
          <label class="field"><span>${esc(label)}</span>
            <input name="value" value="${attr(value)}" maxlength="60" required>
          </label>
        </div>
        <div class="modal-foot">
          <button type="button" class="btn ghost" data-close>取消</button>
          <button class="btn primary" type="submit">${esc(confirmText)}</button>
        </div>
      </form>`);

      const form = els.modalLayer.querySelector('form');
      const input = form.querySelector('input[name="value"]');
      input.focus();
      input.select();

      let finished = false;
      const finish = (result) => {
        if (finished) return;
        finished = true;
        closeModal();
        resolve(result);
      };

      form.addEventListener('submit', (event) => {
        event.preventDefault();
        const next = input.value.trim();
        if (next) finish(next);
      });
      $$('[data-close]', form).forEach((button) => button.addEventListener('click', () => finish(null)));
      els.modalLayer.addEventListener('click', (event) => {
        if (event.target === els.modalLayer) finish(null);
      });
    });
  }

  // 确认弹窗，返回布尔值。
  function confirmDialog({ title, message, confirmText = '确认', danger = true }) {
    return new Promise((resolve) => {
      mountModal(`<div class="modal" role="dialog" aria-modal="true">
        <div class="modal-head">
          <div><h2>${esc(title)}</h2></div>
          <button type="button" class="icon-btn" data-close aria-label="关闭">${SVG.x}</button>
        </div>
        <div class="modal-body"><p class="confirm-text">${esc(message)}</p></div>
        <div class="modal-foot">
          <button type="button" class="btn ghost" data-close>取消</button>
          <button type="button" class="btn ${danger ? 'danger' : 'primary'}" data-confirm>${esc(confirmText)}</button>
        </div>
      </div>`);

      let finished = false;
      const finish = (result) => {
        if (finished) return;
        finished = true;
        closeModal();
        resolve(result);
      };
      $$('[data-close]', els.modalLayer).forEach((button) =>
        button.addEventListener('click', () => finish(false))
      );
      els.modalLayer.querySelector('[data-confirm]').addEventListener('click', () => finish(true));
      els.modalLayer.addEventListener('click', (event) => {
        if (event.target === els.modalLayer) finish(false);
      });
    });
  }

  // 生成「面板 -> 分组」两级下拉选项，供书签弹窗选择归属。
  function columnOptions(selectedId) {
    return state.boards
      .filter((board) => board.columns.length)
      .map(
        (board) => `<optgroup label="${attr(board.name)}">${board.columns
          .map(
            (column) =>
              `<option value="${column.id}" ${column.id === selectedId ? 'selected' : ''}>${esc(column.name)}</option>`
          )
          .join('')}</optgroup>`
      )
      .join('');
  }

  // 添加/编辑书签弹窗：失焦自动抓取标题与图标，保存时提交整份表单。
  function openLinkDialog(options = {}) {
    const found = options.linkId ? findLink(options.linkId) : null;
    if (options.linkId && !found) {
      toast('书签不存在');
      return;
    }
    const existing = found?.link || null;
    const editing = Boolean(existing);
    const allColumns = state.boards.flatMap((board) => board.columns);
    if (!allColumns.length) {
      toast('先新建一个分组，再添加书签');
      return;
    }
    const defaultColumnId =
      options.columnId || existing?.column_id || activeBoard()?.columns[0]?.id || allColumns[0].id;

    mountModal(`<form class="modal link-form" role="dialog" aria-modal="true">
      <div class="modal-head">
        <div>
          <h2>${editing ? '编辑书签' : '添加书签'}</h2>
          <p>${editing ? '可以修改网址、标题和所属分组。' : '粘贴网址即可，标题可以自动抓取。'}</p>
        </div>
        <button type="button" class="icon-btn" data-close aria-label="关闭">${SVG.x}</button>
      </div>
      <div class="modal-body">
        <label class="field"><span>网址 <i class="required">*</i></span>
          <div class="url-row">
            <span class="favicon no-icon" id="link-preview" data-letter="?"><span class="fallback">·</span></span>
            <input id="link-url" type="text" inputmode="url" autocomplete="off" spellcheck="false"
              placeholder="https://example.com" value="${attr(existing?.url || '')}" required>
          </div>
        </label>
        <label class="field"><span>标题</span>
          <input id="link-title" maxlength="300" placeholder="留空则自动获取" value="${attr(existing?.title || '')}">
        </label>
        <label class="field"><span>备注</span>
          <textarea id="link-note" maxlength="2000" placeholder="可选，例如：每周必看">${esc(existing?.note || '')}</textarea>
        </label>
        <label class="field"><span>分组</span>
          <select id="link-column">${columnOptions(defaultColumnId)}</select>
        </label>
        <p class="form-error" id="link-error"></p>
      </div>
      <div class="modal-foot">
        <button type="button" class="btn ghost" data-close>取消</button>
        <button class="btn primary" type="submit" id="link-submit">${editing ? '保存修改' : '添加书签'}</button>
      </div>
    </form>`);

    wireModalClose();

    const form = els.modalLayer.querySelector('form');
    const urlInput = $('#link-url');
    const titleInput = $('#link-title');
    const noteInput = $('#link-note');
    const columnSelect = $('#link-column');
    const errorEl = $('#link-error');
    const submit = $('#link-submit');

    let iconState = {
      icon: existing?.icon || null,
      url: existing?.url || '',
    };
    let metaSeq = 0;

    function updatePreview(icon, label, pageUrl) {
      const preview = $('#link-preview');
      if (!preview) return;
      const letter = initial(label || '?');
      const src = icon || pageUrl;
      preview.className = `favicon ${tint(label || '')}`;
      preview.dataset.letter = letter;
      preview.innerHTML = `<span class="fallback">${esc(letter)}</span>${
        src ? `<img src="/api/favicon?u=${encodeURIComponent(src)}" alt="">` : ''
      }`;
    }

    async function hydrateMeta() {
      const raw = urlInput.value.trim();
      if (!raw) return;
      const normalized = normalizeClient(raw);
      if (!normalized) return;
      const seq = ++metaSeq;
      try {
        const meta = await api(`/api/meta?url=${encodeURIComponent(normalized)}`);
        if (seq !== metaSeq || urlInput.value.trim() !== raw) return;
        if (!titleInput.value.trim() && meta.title) titleInput.value = meta.title;
        iconState = { icon: meta.icon || null, url: meta.url || normalized };
        updatePreview(iconState.icon, meta.title || hostOf(normalized), iconState.url);
      } catch (error) {
        /* 元数据抓取失败不影响流程，保存时服务端会再试一次 */
      }
    }

    updatePreview(existing?.icon || null, existing?.title || hostOf(existing?.url || ''), existing?.url);
    urlInput.addEventListener('blur', () => {
      void hydrateMeta();
    });

    $$('[data-close]', els.modalLayer).forEach((button) =>
      button.addEventListener('click', closeModal)
    );

    form.addEventListener('submit', async (event) => {
      event.preventDefault();
      errorEl.textContent = '';
      const url = urlInput.value.trim();
      if (!url) {
        errorEl.textContent = '请输入网址';
        urlInput.focus();
        return;
      }
      const payload = {
        column_id: Number(columnSelect.value),
        url,
        title: titleInput.value.trim(),
        note: noteInput.value.trim(),
      };
      if (iconState.icon && sameTarget(iconState.url, url)) {
        payload.icon = iconState.icon;
      }

      submit.disabled = true;
      submit.textContent = '保存中…';
      try {
        if (editing) {
          await api(`/api/links/${existing.id}`, { method: 'PATCH', body: payload });
        } else {
          await api('/api/links', { method: 'POST', body: payload });
        }
        closeModal();
        await refresh();
        toast(editing ? '书签已更新' : '书签已添加');
      } catch (error) {
        errorEl.textContent = error.message;
        submit.disabled = false;
        submit.textContent = editing ? '保存修改' : '添加书签';
      }
    });
  }

  // 删除书签，先二次确认。
  async function deleteLink(linkId) {
    const found = findLink(linkId);
    if (!found) return;
    const ok = await confirmDialog({
      title: '删除这个书签？',
      message: `将删除“${found.link.title}”，此操作无法撤销。`,
      confirmText: '删除书签',
    });
    if (!ok) return;
    await api(`/api/links/${linkId}`, { method: 'DELETE' });
    await refresh();
    toast('书签已删除');
  }

  // 重命名分组（菜单与快捷键共用）。
  async function renameColumn(columnId) {
    const found = findColumn(columnId);
    if (!found) return;
    const name = await promptDialog({
      title: '重命名分组',
      label: '分组名称',
      value: found.column.name,
    });
    if (!name) return;
    await api(`/api/columns/${columnId}`, { method: 'PATCH', body: { name } });
    await refresh();
    toast('分组已重命名');
  }

  /* ----------------------------------------------------------
     导入 / 导出
     ---------------------------------------------------------- */

  // 导入/导出弹窗：两个下载链接 + 文件选择/拖放区域。
  function openImportDialog() {
    mountModal(`<div class="modal" role="dialog" aria-modal="true">
      <div class="modal-head">
        <div><h2>导入 / 导出</h2><p>所有书签都只保存在自己的 SQLite 数据库里。</p></div>
        <button type="button" class="icon-btn" data-close aria-label="关闭">${SVG.x}</button>
      </div>
      <div class="modal-body">
        <section class="io-block">
          <h3>导出</h3>
          <p>JSON 备份包含面板、分组、书签，以及主题 / 配色 / 每行列数和每个面板的瀑布流排版；HTML 可直接导入 Chrome / Edge / Firefox。</p>
          <div class="io-actions">
            <a class="btn ghost" href="/api/export/json" download>${SVG.download} 导出 JSON</a>
            <a class="btn ghost" href="/api/export/html" download>${SVG.download} 导出 HTML</a>
          </div>
        </section>
        <section class="io-block">
          <h3>导入</h3>
          <p>支持 Tabnest JSON 备份或浏览器导出的 HTML 书签文件。重复的网址会自动跳过。</p>
          <label class="drop-file" id="import-drop">
            ${SVG.upload}
            <span>选择文件，或拖放到这里</span>
            <input type="file" id="import-file" accept=".json,.html,.htm,application/json,text/html" hidden>
          </label>
          <p class="form-error" id="import-error"></p>
        </section>
      </div>
      <div class="modal-foot"><button type="button" class="btn ghost" data-close>关闭</button></div>
    </div>`);

    wireModalClose();

    const drop = $('#import-drop');
    const fileInput = $('#import-file');

    fileInput.addEventListener('change', () => {
      const file = fileInput.files?.[0];
      if (file) void importFile(file);
    });
    ['dragenter', 'dragover'].forEach((type) =>
      drop.addEventListener(type, (event) => {
        event.preventDefault();
        drop.classList.add('dragging');
      })
    );
    ['dragleave', 'drop'].forEach((type) =>
      drop.addEventListener(type, (event) => {
        event.preventDefault();
        drop.classList.remove('dragging');
      })
    );
    drop.addEventListener('drop', (event) => {
      const file = event.dataTransfer?.files?.[0];
      if (file) void importFile(file);
    });
  }

  // 读取本地文件并发给后端；JSON 与 HTML 走不同接口。
  async function importFile(file) {
    const errorEl = $('#import-error');
    const drop = $('#import-drop');
    const isJson = file.name.toLowerCase().endsWith('.json') || file.type.includes('json');
    const endpoint = isJson ? '/api/import/json' : '/api/import/html';

    if (errorEl) errorEl.textContent = '';
    drop?.classList.add('loading');
    try {
      const text = await file.text();
      const result = await api(endpoint, { method: 'POST', raw: text });
      const stats = result.stats || {};
      closeModal();
      // 设置（含版式）必须在首次渲染前灌好，
      // 否则刚导入的面板会先按自动排列画一遍。
      await syncSettingsFromServer();
      await refresh();
      // 自动切到第一个导入的面板，让恢复出来的版式立刻可见。
      const firstImported = Array.isArray(result.boards) ? result.boards[0] : null;
      if (firstImported && state.boards.some((board) => board.id === firstImported)) {
        state.activeId = firstImported;
        persistActive();
        render();
      }
      const skipped = stats.skipped ? `，跳过 ${stats.skipped} 个重复或无效网址` : '';
      toast(`导入完成：${stats.boards} 个面板、${stats.columns} 个分组、${stats.links} 个书签${skipped}`);
    } catch (error) {
      if (errorEl) errorEl.textContent = error.message;
      else toast(error.message, 'error');
    } finally {
      drop?.classList.remove('loading');
    }
  }

  /* ----------------------------------------------------------
     拖放
     ---------------------------------------------------------- */

  // 清除所有拖放高亮类。
  function clearIndicators() {
    $$('.drop-before, .drop-after, .drop-active, .drop-left, .drop-right, .drop-empty').forEach((el) => {
      el.classList.remove('drop-before', 'drop-after', 'drop-active', 'drop-left', 'drop-right', 'drop-empty');
    });
  }

  // 拖拽结束后的统一清理：停自动滚动、清高亮、清状态。
  function cleanupDrag() {
    stopAutoScroll();
    clearIndicators();
    $$('.is-dragging').forEach((el) => el.classList.remove('is-dragging'));
    drag.payload = null;
    drag.el = null;
    drag.overCard = null;
    drag.overColumn = null;
    drag.overColumnEl = null;
    drag.overVisualCol = null;
    drag.overBoard = null;
    drag.lastX = null;
    drag.lastY = null;
  }

  // 停止拖拽时的自动滚动动画帧。
  function stopAutoScroll() {
    if (drag.scrollFrame) {
      window.cancelAnimationFrame(drag.scrollFrame);
      drag.scrollFrame = 0;
    }
  }

  // 拖拽书签/分组时启动边缘自动滚动（面板排序不需要）。
  function startAutoScroll() {
    if (drag.scrollFrame || !drag.payload || drag.payload.type === 'board') return;
    drag.scrollFrame = window.requestAnimationFrame(autoScrollTick);
  }

  // 每帧根据指针离容器边缘的距离计算滚动速度。
  function autoScrollTick() {
    drag.scrollFrame = 0;
    if (!drag.payload || drag.lastY === null) return;
    const scroller = document.querySelector('.main');
    if (!scroller) return;
    const rect = scroller.getBoundingClientRect();
    if (drag.lastX < rect.left || drag.lastX > rect.right) {
      drag.scrollFrame = window.requestAnimationFrame(autoScrollTick);
      return;
    }
    const edge = Math.max(56, Math.min(110, rect.height * 0.14));
    let speed = 0;
    if (drag.lastY < rect.top + edge) {
      speed = -Math.ceil(((rect.top + edge - drag.lastY) / edge) * 20);
    } else if (drag.lastY > rect.bottom - edge) {
      speed = Math.ceil(((drag.lastY - (rect.bottom - edge)) / edge) * 20);
    }
    if (speed !== 0) {
      const before = scroller.scrollTop;
      scroller.scrollTop += speed;
      if (scroller.scrollTop !== before) {
        refreshDragTargetAtPoint(drag.lastX, drag.lastY);
      }
    }
    if (drag.payload) {
      drag.scrollFrame = window.requestAnimationFrame(autoScrollTick);
    }
  }

  // 自动滚动后指针下的元素会变，这里按坐标重新判定落点。
  function refreshDragTargetAtPoint(x, y) {
    if (!drag.payload || drag.payload.type === 'board') return;
    const node = document.elementFromPoint(x, y);
    if (!node) return;

    if (drag.payload.type === 'column') {
      const column = node.closest('.column');
      if (column && column !== drag.el && els.boardView.contains(column)) {
        clearIndicators();
        const rect = column.getBoundingClientRect();
        column.classList.add(
          y < rect.top + rect.height / 2 ? 'drop-before' : 'drop-after'
        );
        drag.overColumnEl = column;
        drag.overVisualCol = null;
        return;
      }
      const wrapper = node.closest('.masonry-col');
      if (wrapper && !wrapper.querySelector('.column')) {
        clearIndicators();
        wrapper.classList.add('drop-empty');
        drag.overColumnEl = null;
        drag.overVisualCol = wrapper;
      }
      return;
    }

    const card = node.closest('.link-card');
    const body = node.closest('.column-body');
    if (card && card !== drag.el) {
      clearIndicators();
      const rect = card.getBoundingClientRect();
      card.classList.add(y < rect.top + rect.height / 2 ? 'drop-before' : 'drop-after');
      drag.overCard = card;
      drag.overColumn = null;
    } else if (body) {
      clearIndicators();
      body.classList.add('drop-active');
      drag.overCard = null;
      drag.overColumn = body;
    }
  }

  // 根据指针 Y 坐标算出应插入到第几张卡片之前。
  function insertionIndex(body, clientY) {
    const cards = Array.from(body.querySelectorAll('.link-card')).filter((card) => card !== drag.el);
    for (let index = 0; index < cards.length; index += 1) {
      const rect = cards[index].getBoundingClientRect();
      if (clientY < rect.top + rect.height / 2) return index;
    }
    return cards.length;
  }

  // 拖拽开始：区分书签 / 分组 / 面板三种载荷。
  function onDragStart(event) {
    const card = event.target.closest('.link-card');
    const head = event.target.closest('.column-head');
    const boardItem = event.target.closest('.board-item');

    if (card) {
      if (card.closest('.search-view')) {
        event.preventDefault();
        return;
      }
      drag.payload = { type: 'link', id: Number(card.dataset.linkId), columnId: Number(card.dataset.columnId) };
      drag.el = card;
      drag.suppressClickUntil = Date.now() + 400;
      drag.lastX = event.clientX;
      drag.lastY = event.clientY;
      event.dataTransfer.effectAllowed = 'move';
      event.dataTransfer.setData('text/plain', `link:${drag.payload.id}`);
      window.requestAnimationFrame(() => card.classList.add('is-dragging'));
      startAutoScroll();
      return;
    }

    if (head) {
      if (event.target.closest('button')) {
        event.preventDefault();
        return;
      }
      const column = head.closest('.column');
      if (!column) return;
      drag.payload = { type: 'column', id: Number(column.dataset.columnId) };
      drag.el = column;
      drag.lastX = event.clientX;
      drag.lastY = event.clientY;
      event.dataTransfer.effectAllowed = 'move';
      event.dataTransfer.setData('text/plain', `column:${drag.payload.id}`);
      window.requestAnimationFrame(() => column.classList.add('is-dragging'));
      try {
        event.dataTransfer.setDragImage(column, 24, 18);
      } catch (error) {
        /* 部分浏览器不支持 setDragImage，忽略即可 */
      }
      startAutoScroll();
      return;
    }

    if (boardItem) {
      if (event.target.closest('[data-act]')) {
        event.preventDefault();
        return;
      }
      drag.payload = { type: 'board', id: Number(boardItem.dataset.boardId) };
      drag.el = boardItem;
      event.dataTransfer.effectAllowed = 'move';
      event.dataTransfer.setData('text/plain', `board:${drag.payload.id}`);
      window.requestAnimationFrame(() => boardItem.classList.add('is-dragging'));
    }
  }

  // 拖拽经过：阻止默认行为并画出插入位置提示。
  function onDragOver(event) {
    if (!drag.payload) return;
    drag.lastX = event.clientX;
    drag.lastY = event.clientY;
    startAutoScroll();

    if (drag.payload.type === 'link') {
      const card = event.target.closest('.link-card');
      const body = event.target.closest('.column-body');
      if (card && card !== drag.el) {
        event.preventDefault();
        event.dataTransfer.dropEffect = 'move';
        clearIndicators();
        const rect = card.getBoundingClientRect();
        card.classList.add(event.clientY < rect.top + rect.height / 2 ? 'drop-before' : 'drop-after');
        drag.overCard = card;
        drag.overColumn = null;
      } else if (body) {
        event.preventDefault();
        event.dataTransfer.dropEffect = 'move';
        clearIndicators();
        body.classList.add('drop-active');
        drag.overCard = null;
        drag.overColumn = body;
      }
      return;
    }

    if (drag.payload.type === 'column') {
      const column = event.target.closest('.column');
      if (column && column !== drag.el && els.boardView.contains(column)) {
        event.preventDefault();
        event.dataTransfer.dropEffect = 'move';
        clearIndicators();
        const rect = column.getBoundingClientRect();
        column.classList.add(
          event.clientY < rect.top + rect.height / 2 ? 'drop-before' : 'drop-after'
        );
        drag.overColumnEl = column;
        drag.overVisualCol = null;
        return;
      }
      const wrapper = event.target.closest('.masonry-col');
      if (wrapper && els.boardView.contains(wrapper) && !wrapper.querySelector('.column')) {
        event.preventDefault();
        event.dataTransfer.dropEffect = 'move';
        clearIndicators();
        wrapper.classList.add('drop-empty');
        drag.overColumnEl = null;
        drag.overVisualCol = wrapper;
      }
      return;
    }

    if (drag.payload.type === 'board') {
      const item = event.target.closest('.board-item');
      if (item && item !== drag.el) {
        event.preventDefault();
        event.dataTransfer.dropEffect = 'move';
        clearIndicators();
        const rect = item.getBoundingClientRect();
        item.classList.add(event.clientY < rect.top + rect.height / 2 ? 'drop-before' : 'drop-after');
        drag.overBoard = item;
      }
    }
  }

  // 放下：先本地重排保证即时反馈，再写回服务端。
  async function onDrop(event) {
    if (!drag.payload) return;
    event.preventDefault();
    stopAutoScroll();
    const payload = drag.payload;

    try {
      if (payload.type === 'link') {
        const point = document.elementFromPoint(event.clientX, event.clientY) || event.target;
        const pointCard = point.closest('.link-card');
        const pointBody = point.closest('.column-body');
        const targetCard =
          pointCard && pointCard !== drag.el
            ? pointCard
            : drag.overCard && drag.overCard !== drag.el
              ? drag.overCard
              : null;
        const body =
          targetCard?.closest('.column-body') ||
          pointBody ||
          drag.overColumn ||
          drag.overCard?.closest('.column-body');
        if (!body) return;
        const targetColumnId = Number(body.dataset.columnId);
        let index = insertionIndex(body, event.clientY);
        if (targetCard) {
          const cards = Array.from(body.querySelectorAll('.link-card')).filter((card) => card !== drag.el);
          const cardIndex = cards.indexOf(targetCard);
          if (cardIndex >= 0) {
            const rect = targetCard.getBoundingClientRect();
            index = event.clientY < rect.top + rect.height / 2 ? cardIndex : cardIndex + 1;
          }
        }
        await moveLink(payload.id, payload.columnId, targetColumnId, index);
      } else if (payload.type === 'column') {
        if (drag.overVisualCol) {
          await moveColumnToVisual(payload.id, Number(drag.overVisualCol.dataset.visualCol));
          return;
        }
        const point = document.elementFromPoint(event.clientX, event.clientY) || event.target;
        const targetColumn =
          point.closest('.column') || event.target.closest('.column') || drag.overColumnEl;
        if (!targetColumn || targetColumn === drag.el) return;
        const rect = targetColumn.getBoundingClientRect();
        const before = event.clientY < rect.top + rect.height / 2;
        await moveColumn(payload.id, Number(targetColumn.dataset.columnId), before);
      } else if (payload.type === 'board') {
        if (!drag.overBoard) return;
        const rect = drag.overBoard.getBoundingClientRect();
        const before = event.clientY < rect.top + rect.height / 2;
        await moveBoard(payload.id, Number(drag.overBoard.dataset.boardId), before);
      }
    } catch (error) {
      toast(error.message || '移动失败', 'error');
      await refresh();
    } finally {
      cleanupDrag();
    }
  }

  // 移动书签：改内存数据 -> 立即重绘 -> 提交两个分组的完整顺序。
  async function moveLink(linkId, fromColumnId, toColumnId, index) {
    const source = findColumn(fromColumnId)?.column;
    const target = findColumn(toColumnId)?.column;
    if (!source || !target) return;

    const fromIndex = source.links.findIndex((link) => link.id === linkId);
    if (fromIndex === -1) return;
    const [link] = source.links.splice(fromIndex, 1);
    const clamped = Math.max(0, Math.min(index, target.links.length));
    target.links.splice(clamped, 0, link);
    link.column_id = toColumnId;

    render();

    const affected = new Map();
    affected.set(fromColumnId, source);
    affected.set(toColumnId, target);
    const columns = Array.from(affected.values()).map((column) => ({
      column_id: column.id,
      link_ids: column.links.map((item) => item.id),
    }));
    await api('/api/links/arrange', { method: 'PUT', body: { columns } });
  }

  // 移动分组到目标分组前/后：先改版式，再按扁平顺序写回 position。
  async function moveColumn(columnId, targetId, before) {
    const board = activeBoard();
    if (!board || columnId === targetId) return;
    const visual = ensureBoardLayout(board);
    const source = visual.findIndex((list) => list.includes(columnId));
    const target = visual.findIndex((list) => list.includes(targetId));
    if (source === -1 || target === -1) return;

    visual[source] = visual[source].filter((id) => id !== columnId);
    const list = visual[target];
    let index = list.indexOf(targetId);
    if (index === -1) index = list.length;
    else if (!before) index += 1;
    list.splice(Math.max(0, Math.min(index, list.length)), 0, columnId);

    localStorage.setItem(layoutStorageKey(board.id), JSON.stringify(visual));
    queueLayout(board.id, visual);
    const flat = visual.flat();
    const order = new Map(flat.map((id, position) => [id, position]));
    board.columns.sort((a, b) => (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0));

    renderMain();
    await api('/api/columns/order', {
      method: 'PUT',
      body: { board_id: board.id, ids: flat },
    });
  }

  // 把分组拖到某个视觉列顶部（空列也接受拖入）。
  async function moveColumnToVisual(columnId, visualIndex) {
    const board = activeBoard();
    if (!board) return;
    const visual = ensureBoardLayout(board);
    const source = visual.findIndex((list) => list.includes(columnId));
    if (source === -1) return;
    visual[source] = visual[source].filter((id) => id !== columnId);
    const target = Math.max(0, Math.min(Number(visualIndex), visual.length - 1));
    visual[target].unshift(columnId);

    localStorage.setItem(layoutStorageKey(board.id), JSON.stringify(visual));
    queueLayout(board.id, visual);
    const flat = visual.flat();
    const order = new Map(flat.map((id, position) => [id, position]));
    board.columns.sort((a, b) => (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0));

    renderMain();
    await api('/api/columns/order', {
      method: 'PUT',
      body: { board_id: board.id, ids: flat },
    });
  }

  // 重排面板：本地 splice 后提交完整 id 列表。
  async function moveBoard(boardId, targetId, before) {
    const fromIndex = state.boards.findIndex((board) => board.id === boardId);
    if (fromIndex === -1) return;
    const [board] = state.boards.splice(fromIndex, 1);
    let targetIndex = state.boards.findIndex((item) => item.id === targetId);
    if (targetIndex === -1) {
      state.boards.splice(fromIndex, 0, board);
      return;
    }
    if (!before) targetIndex += 1;
    state.boards.splice(targetIndex, 0, board);

    render();
    await api('/api/boards/order', {
      method: 'PUT',
      body: { ids: state.boards.map((item) => item.id) },
    });
  }

  /* ----------------------------------------------------------
     动作与事件绑定
     ---------------------------------------------------------- */

  // 统一处理带 data-act 的按钮点击。
  async function handleAction(target) {
    const action = target.dataset.act;
    if (!action) return;

    switch (action) {
      case 'toggle-sidebar':
        toggleSidebar();
        break;
      case 'new-board':
        startNewBoard();
        break;
      case 'new-column':
        startNewColumn(Number(target.dataset.boardId || state.activeId));
        break;
      case 'open-add-link':
        openLinkDialog();
        break;
      case 'add-link':
        openLinkDialog({ columnId: Number(target.dataset.columnId) });
        break;
      case 'edit-link':
        openLinkDialog({ linkId: Number(target.dataset.linkId) });
        break;
      case 'delete-link':
        await deleteLink(Number(target.dataset.linkId));
        break;
      case 'board-menu':
        openBoardMenu(target, Number(target.dataset.boardId || state.activeId));
        break;
      case 'column-menu':
        openColumnMenu(target, Number(target.dataset.columnId));
        break;
      case 'clear-search':
        clearSearch();
        break;
      default:
        break;
    }
  }

  // 绑定所有静态事件：登录、快捷键、搜索、拖拽、图片兜底等。
  function bindStatic() {
    els.loginForm.addEventListener('submit', onLogin);
    $('#btn-new-board').addEventListener('click', startNewBoard);
    $('#btn-import').addEventListener('click', openImportDialog);
    els.themeBtn.addEventListener('click', () => openAppearance(els.themeBtn));
    els.logoutBtn.addEventListener('click', logout);
    els.scrim.addEventListener('click', closeSidebar);

    const scheme = window.matchMedia('(prefers-color-scheme: dark)');
    scheme.addEventListener('change', () => {
      if (state.mode === 'auto') applyAppearance('auto', state.palette, false);
    });

    const mainScroller = document.querySelector('.main');
    const topbar = document.querySelector('.topbar');
    if (mainScroller && topbar) {
      mainScroller.addEventListener(
        'scroll',
        () => topbar.classList.toggle('is-scrolled', mainScroller.scrollTop > 6),
        { passive: true }
      );
    }

    let layoutResizeTimer = null;
    window.addEventListener('resize', () => {
      window.clearTimeout(layoutResizeTimer);
      layoutResizeTimer = window.setTimeout(() => {
        if (state.boards.length && !state.query.trim()) renderMain();
      }, 160);
    });

    let searchTimer = null;
    els.search.addEventListener('input', () => {
      window.clearTimeout(searchTimer);
      const value = els.search.value;
      searchTimer = window.setTimeout(() => {
        state.query = value;
        renderMain();
      }, 90);
    });

    document.addEventListener('click', (event) => {
      const actionTarget = event.target.closest('[data-act]');
      if (actionTarget) {
        event.preventDefault();
        void handleAction(actionTarget);
        return;
      }
      if (!event.target.closest('.menu')) closeMenu();
      const boardItem = event.target.closest('.board-item');
      if (boardItem) {
        selectBoard(Number(boardItem.dataset.boardId));
      }
    });

    els.boardList.addEventListener('keydown', (event) => {
      if ((event.key === 'Enter' || event.key === ' ') && event.target.matches('.board-item')) {
        event.preventDefault();
        selectBoard(Number(event.target.dataset.boardId));
      }
    });

    document.addEventListener('click', (event) => {
      if (Date.now() < drag.suppressClickUntil && event.target.closest('.link-card')) {
        event.preventDefault();
        event.stopPropagation();
      }
    }, true);

    document.addEventListener('dragstart', onDragStart);
    document.addEventListener('dragover', onDragOver);
    document.addEventListener('drop', onDrop);
    document.addEventListener('dragend', cleanupDrag);
    window.addEventListener('blur', cleanupDrag);

    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        if (state.appearanceEl) {
          closeAppearance();
          return;
        }
        if (state.menuEl) {
          closeMenu();
          return;
        }
        if (!els.modalLayer.hidden) {
          closeModal();
          return;
        }
        if (state.query) {
          clearSearch();
          return;
        }
        if (els.sidebar.classList.contains('open')) {
          closeSidebar();
          return;
        }
      }

      const tag = document.activeElement?.tagName || '';
      const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(tag) || document.activeElement?.isContentEditable;
      if (typing || !els.modalLayer.hidden || !els.loginLayer.hidden) return;

      if (event.key === '/' || ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k')) {
        event.preventDefault();
        els.search.focus();
        els.search.select();
        return;
      }
      if (!event.metaKey && !event.ctrlKey && !event.altKey && event.key.toLowerCase() === 'n') {
        event.preventDefault();
        if (activeBoard()?.columns.length) openLinkDialog();
        else toast('先新建一个分组');
      }
    });

    document.addEventListener('error', (event) => {
      const image = event.target;
      if (image.tagName === 'IMG' && image.closest('.favicon')) {
        image.closest('.favicon').classList.add('no-icon');
        image.remove();
      }
    }, true);

    document.addEventListener('scroll', () => {
      closeMenu();
      closeAppearance();
    }, true);
    window.addEventListener('resize', () => {
      closeMenu();
      closeAppearance();
    });
  }

  // DOM 就绪后再启动；脚本在 body 末尾时直接执行。
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', boot);
  } else {
    boot();
  }
})();
