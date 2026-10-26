(() => {
  'use strict';

  const $ = (sel, root = document) => root.querySelector(sel);
  const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));
  const html = document.documentElement;
  const clamp = (v, a = 0, b = 1) => Math.min(b, Math.max(a, v));
  const easeInOut = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
  const easeOut = (t) => 1 - Math.pow(1 - t, 3);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  /* ================= 软件界面展示 ================= */

  // 每一页都是 frontend 的真实构建（website/app），用浏览器预览模式的演示数据驱动
  const TOURS = [
    { label: '开始前确认', icon: 'shield', colors: ['#34d399', '#059669'], src: '?step=welcome', title: '开始之前，请先确认',
      text: '首次启动时逐项确认：已解锁 Bootloader、重要资料已备份、刷机包与手机匹配。选择只保存在本机，下次不再询问。' },
    { label: '选择刷机包', icon: 'package', colors: ['#3a3f47', '#111318'], src: '?step=package', title: '选择刷机包',
      text: '拖入 .zip 刷机包或已解压的文件夹，也支持小米官方线刷包目录。点“选择 .zip 刷机包”，体验读取包信息与解压。' },
    { label: '连接手机', icon: 'phone', colors: ['#60a5fa', '#2563eb'], src: '?preview=ready&step=device', title: '连接手机',
      text: '自动识别 ADB / Fastboot 设备，并与刷机包支持的机型代号核对；手机处于开机状态时会自动重启到 Fastboot。' },
    { label: 'USB 驱动', icon: 'usb', colors: ['#94a3b8', '#475569'], src: '?preview=ready&step=device&devices=none&driver=missing', title: '缺少 USB 驱动',
      text: '手机已经插上，但电脑没有 Android USB 驱动时，页面会直接给出一键安装入口。' },
    { label: '选择方式', icon: 'sliders', colors: ['#a78bfa', '#7c3aed'], src: '?preview=ready&step=plan', title: '选择刷入方式',
      text: '完整线刷或保留数据更新。展开高级选项，可以换用外部 ID 补丁、刷入 Root 镜像或打开扩容工具。' },
    { label: '刷入中', icon: 'zap', colors: ['#fbbf24', '#ea580c'], src: '?preview=ready&step=plan', thumb: '?preview=ready&step=review', auto: 'flash', title: '开始刷入',
      text: '自动演示一次“保留数据更新”：预检通过后依次写入固件、启动链和系统分区，分区进度与日志实时更新，最后进入完成页。' },
    { label: 'Super 扩容', icon: 'drive', colors: ['#f472b6', '#db2777'], src: '?preview=ready&step=expand', title: 'Super 扩容 / 恢复',
      text: 'Super 容量不足时扩大分区。扩容前备份 GPT 并校验，之后才修改分区；也可以用备份恢复官方分区表。' },
    { label: 'ID 补丁', icon: 'sparkles', colors: ['#22d3ee', '#0891b2'], src: '?preview=ready&step=idtool', title: '刷入 ID 补丁',
      text: '单独更换 mi_ext 中的 ID，无需重新刷入整个系统。会自动找出刷机包目录里的补丁文件。' },
    { label: '官方线刷包', icon: 'folder', colors: ['#ff9a57', '#ff5a00'], src: '?preview=official&step=package', title: '小米官方线刷包',
      text: '识别包含 flash_all.bat 的官方目录，逐条解析脚本语句执行，遇到无法识别的内容直接拒绝。' },
  ];

  const ICONS = {
    shield: '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/><path d="m9 12 2 2 4-4"/>',
    package: '<path d="M11 21.73a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73z"/><path d="M12 22V12"/><path d="m3.3 7 8.7 5 8.7-5"/>',
    phone: '<rect x="5" y="2" width="14" height="20" rx="2"/><path d="M12 18h.01"/>',
    usb: '<circle cx="10" cy="7" r="1"/><circle cx="4" cy="20" r="1"/><path d="M4.7 19.3 19 5"/><path d="m21 3-3 1 2 2Z"/><path d="M9.26 7.68 5 12l2 5"/><path d="m10 14 5 2 3.5-3.5"/><path d="m18 12 1-1 1 1-1 1Z"/>',
    sliders: '<path d="M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3M14 2v4M8 10v4M16 18v4"/>',
    zap: '<path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/>',
    drive: '<path d="M22 12H2"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/><path d="M6 16h.01M10 16h.01"/>',
    sparkles: '<path d="M9.94 15.5A2 2 0 0 0 8.5 14.06l-6.14-1.58a.5.5 0 0 1 0-.96L8.5 9.94A2 2 0 0 0 9.94 8.5l1.58-6.14a.5.5 0 0 1 .96 0L14.06 8.5A2 2 0 0 0 15.5 9.94l6.14 1.58a.5.5 0 0 1 0 .96L15.5 14.06a2 2 0 0 0-1.44 1.44l-1.58 6.14a.5.5 0 0 1-.96 0z"/>',
    folder: '<path d="M10 10.5 8 13l2 2.5M14 10.5l2 2.5-2 2.5"/><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z"/>',
  };
  const appIcon = (tour) =>
    `<span class="app-icon" style="--c1:${tour.colors[0]};--c2:${tour.colors[1]}"><svg viewBox="0 0 24 24" aria-hidden="true">${ICONS[tour.icon]}</svg></span>`;

  const stage = $('#stage');
  const strip = $('#strip');
  const stageMain = $('#stage-main');
  const tourInner = $('.tour-inner');
  const tourEl = $('#tour');
  const caption = $('#tour-caption');
  const frame = $('#app-frame');
  const wrap = $('#window-wrap');
  const win = $('#window');
  const loading = $('#window-loading');
  const app = (tour) => `app/index.html${tour.src}`;
  const thumbApp = (tour) => `app/index.html${tour.thumb || tour.src}`;
  // 界面区快进入视野时才开始加载：每个缩略图都是一份完整运行的界面，首屏不必等它们
  let live = false;
  let pendingSrc = '';
  let tourToken = 0;
  let current = -1;

  // 左侧倾斜卡片：每个界面一份实时运行的界面，缩小显示、不可点；角上是界面图标
  const items = TOURS.map((tour, i) => {
    const li = document.createElement('div');
    li.className = 'thumb-item';
    const b = document.createElement('button');
    b.className = 'thumb';
    b.type = 'button';
    b.setAttribute('aria-label', `切换到：${tour.label}`);
    b.innerHTML = `<span class="thumb-screen"><iframe tabindex="-1" aria-hidden="true" data-src="${thumbApp(tour)}"></iframe></span>${appIcon(tour)}<span class="thumb-tip"></span>`;
    b.querySelector('.thumb-tip').textContent = tour.label;
    b.addEventListener('click', () => selectTour(i, b.querySelector('.thumb-screen')));
    li.appendChild(b);
    strip.appendChild(li);
    return li;
  });

  function selectTour(i, fromEl) {
    if (i === current) return;
    const tour = TOURS[i];
    const token = ++tourToken;
    const from = fromEl && !matchMedia('(prefers-reduced-motion: reduce)').matches ? fromEl.getBoundingClientRect() : null;
    current = i;
    items.forEach((li, j) => li.classList.toggle('is-active', j === i));
    setTimeout(markStrip, 550);

    caption.classList.add('swap');
    setTimeout(() => {
      $('#tour-title').textContent = tour.title;
      $('#tour-text').textContent = tour.text;
      caption.classList.remove('swap');
    }, from ? 220 : 0);

    // 从被点的卡片位置放大成主窗口
    if (from) {
      const to = wrap.getBoundingClientRect();
      wrap.style.transition = 'none';
      wrap.style.transform = `translate(${from.left - to.left}px, ${from.top - to.top}px) scale(${from.width / to.width}, ${from.height / to.height})`;
      wrap.style.opacity = '0.4';
      void wrap.offsetWidth;
      wrap.style.transition = 'transform .62s cubic-bezier(.2,.85,.25,1), opacity .35s ease';
      wrap.style.transform = '';
      wrap.style.opacity = '';
    }

    loading.classList.add('on');
    frame.onload = () => {
      setTimeout(() => token === tourToken && loading.classList.remove('on'), 120);
      if (tour.auto === 'flash') void autoFlash(token);
    };
    if (live) frame.src = app(tour);
    else pendingSrc = app(tour);
  }

  /** 在 iframe 里等待某个元素出现（同源页面）。 */
  async function waitFor(token, sel, match = () => true, timeout = 6000) {
    const end = performance.now() + timeout;
    while (performance.now() < end) {
      if (token !== tourToken) return null;
      const doc = frame.contentDocument;
      const el = doc && $$(sel, doc).find(match);
      if (el) return el;
      await sleep(60);
    }
    return null;
  }
  const hasText = (t) => (el) => el.textContent.includes(t);

  async function autoFlash(token) {
    const choice = await waitFor(token, '.choice', hasText('保留数据更新'));
    if (!choice) return;
    await sleep(700);
    if (token !== tourToken) return;
    choice.click();
    const next = await waitFor(token, '.page-foot .btn.primary', (b) => !b.disabled);
    await sleep(650);
    if (!next || token !== tourToken) return;
    next.click();
    const start = await waitFor(token, '.page-foot .btn.primary', hasText('开始更新'));
    await sleep(900);
    if (!start || token !== tourToken) return;
    start.click();
  }

  // 窗口按可用空间等比缩放；大屏时一屏放下窗口和左侧卡片栏
  const setThumb = (tw) => {
    stage.style.setProperty('--tw', `${tw}px`);
    stage.style.setProperty('--th', `${(tw * 660) / 980}px`);
    stage.style.setProperty('--ts', String(tw / 980));
  };
  const layout = () => {
    const visible = TOURS.length - 1;
    const gap = 18;
    let s;
    if (window.innerWidth < 900) {
      setThumb(150);
      stageMain.style.width = '';
      tourInner.style.maxWidth = '';
      strip.style.maxHeight = '';
      s = Math.min(1, stageMain.clientWidth / 980);
    } else {
      const head = $('#tour-head').offsetHeight + 22;
      // 一屏内放下：导航 64 + 上下留白 52 + 标题区 + 说明文字（最多两行）92
      const availH = window.innerHeight - 64 - 52 - head - 92;
      const tourCs = getComputedStyle(tourEl);
      const availW = Math.min(1280, tourEl.clientWidth - parseFloat(tourCs.paddingLeft) - parseFloat(tourCs.paddingRight));
      s = Math.min(1, availH / 660);
      let tw = 0;
      for (let k = 0; k < 2; k++) {
        // 卡片不能太小：最小 72px 高，放不下时左侧栏可以上下滚动
        const th = clamp((660 * s + 70 - visible * gap) / visible, 72, 118);
        tw = (th * 980) / 660;
        setThumb(tw);
        s = Math.max(0.35, Math.min(1, availH / 660, (availW - tw * 0.9 - 28 - 40) / 980));
      }
      stageMain.style.width = `${980 * s}px`;
      strip.style.maxHeight = `${660 * s + 70}px`;
      // 标题区与“卡片栏 + 窗口”这一组对齐
      tourInner.style.maxWidth = `${tw * 0.9 + 28 + 40 + 980 * s}px`;
    }
    win.style.transform = `translateX(-50%) scale(${s})`;
    wrap.style.height = `${660 * s}px`;
  };
  // 左侧栏放不下全部卡片时，底部渐隐提示还能往下滚
  const markStrip = () => {
    const scrollable = window.innerWidth >= 900 && strip.scrollHeight > strip.clientHeight + 4;
    strip.classList.toggle('scrollable', scrollable);
    strip.classList.toggle('at-end', strip.scrollTop + strip.clientHeight >= strip.scrollHeight - 4);
  };
  strip.addEventListener('scroll', markStrip, { passive: true });
  new ResizeObserver(() => { layout(); markStrip(); }).observe(tourEl);
  window.addEventListener('resize', () => { layout(); markStrip(); });
  layout();
  setTimeout(markStrip, 600);
  selectTour(1);

  // 当前显示在大窗口里的界面，左侧那份缩略图是隐藏的，等它回到左侧时再加载
  const loadThumbs = () => {
    items.forEach((li, i) => {
      const f = li.querySelector('iframe');
      if (i !== current && f.dataset.src && !f.getAttribute('src')) f.src = f.dataset.src;
    });
  };
  const goLive = () => {
    if (live) return;
    live = true;
    if (pendingSrc) frame.src = pendingSrc;
    loadThumbs();
  };
  new IntersectionObserver((entries, obs) => {
    if (entries.some((e) => e.isIntersecting)) { goLive(); obs.disconnect(); }
  }, { rootMargin: '100% 0px' }).observe(tourEl);
  strip.addEventListener('click', () => setTimeout(loadThumbs, 700));


  /* ================= 页面细节 ================= */

  const nav = $('#nav');
  const onScroll = () => nav.classList.toggle('scrolled', window.scrollY > 8);
  window.addEventListener('scroll', onScroll, { passive: true });
  onScroll();

  // 右侧页码：当前页高亮，点击跳到对应整页
  const pages = $$('.page');
  const dots = $('#dots');
  const dotEls = pages.map((page) => {
    const b = document.createElement('button');
    b.className = 'dot';
    b.type = 'button';
    b.setAttribute('aria-label', page.dataset.page);
    b.innerHTML = '<i></i><span></span>';
    b.querySelector('span').textContent = page.dataset.page;
    b.addEventListener('click', () => page.scrollIntoView({ behavior: 'smooth' }));
    dots.appendChild(b);
    return b;
  });
  const markPage = () => {
    const y = window.scrollY + window.innerHeight * 0.45;
    let idx = 0;
    pages.forEach((page, i) => { if (page.offsetTop <= y) idx = i; });
    dotEls.forEach((d, i) => d.classList.toggle('on', i === idx));
    dots.classList.toggle('on-dark', pages[idx].classList.contains('dark'));
  };
  window.addEventListener('scroll', markPage, { passive: true });
  window.addEventListener('resize', markPage);
  markPage();

  const io = new IntersectionObserver((entries) => {
    entries.forEach((e) => {
      if (!e.isIntersecting) return;
      e.target.classList.add('in');
      io.unobserve(e.target);
    });
  }, { rootMargin: '0px 0px -8% 0px' });
  $$('.reveal').forEach((el) => {
    const siblings = $$(':scope > .reveal', el.parentElement);
    if (siblings.length > 1) el.style.transitionDelay = `${siblings.indexOf(el) * 70}ms`;
    io.observe(el);
  });

  $$('[data-copy]').forEach((btn) => btn.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText($(btn.dataset.copy).textContent);
      btn.textContent = '已复制';
    } catch { btn.textContent = '复制失败'; }
    setTimeout(() => { btn.textContent = '复制'; }, 1500);
  }));

  // 最新版本信息（由 release.ps1 生成 version.json，工具内的检查更新读取同一个文件）
  fetch('version.json', { cache: 'no-cache' })
    .then((r) => (r.ok ? r.json() : null))
    .then((rel) => {
      if (!rel) return;
      if (/^https:\/\//.test(rel.download_url || '')) $('#dl-btn').href = rel.download_url;
      if (rel.version) {
        const meta = [`v${rel.version}`, 'Windows x64', rel.download_label || '123 云盘'];
        if (rel.date) meta.splice(1, 0, rel.date);
        $('#dl-meta').textContent = meta.join(' · ');
      }
      if (!Array.isArray(rel.files) || !rel.files.length) return;
      const list = $('#dl-hashes');
      list.innerHTML = '';
      rel.files.forEach((f) => {
        const li = document.createElement('li');
        li.innerHTML = '<span class="hash-name"></span><code class="hash"></code><button type="button" class="hash-copy">复制</button>';
        li.querySelector('.hash-name').textContent = `${f.label || f.name} · ${f.name}`;
        li.querySelector('.hash').textContent = f.sha256;
        li.querySelector('.hash-copy').addEventListener('click', async (e) => {
          const btn = e.currentTarget;
          try { await navigator.clipboard.writeText(f.sha256); btn.textContent = '已复制'; } catch { btn.textContent = '复制失败'; }
          setTimeout(() => { btn.textContent = '复制'; }, 1500);
        });
        list.appendChild(li);
      });
    })
    .catch(() => undefined);

  $('#replay').addEventListener('click', () => {
    const url = new URL(location.href);
    url.searchParams.set('intro', '1');
    url.hash = '';
    location.href = url.toString();
  });

  /* ================= 开场动画 ================= */

  if (!html.classList.contains('intro')) {
    requestAnimationFrame(() => {
      html.classList.add('ready');
      $('#hero-title').classList.add('landed');
    });
    return;
  }
  try { localStorage.setItem('wrion.intro-seen', 'yes'); } catch { /* 忽略 */ }
  if ('scrollRestoration' in history) history.scrollRestoration = 'manual';
  window.scrollTo(0, 0);
  (document.fonts ? document.fonts.ready : Promise.resolve()).then(runIntro);

  function runIntro() {
    const layer = $('#intro');
    const canvas = $('#intro-canvas');
    const ctx = canvas.getContext('2d');
    const title = $('#hero-title');
    const W = window.innerWidth;
    const H = window.innerHeight;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.round(W * dpr);
    canvas.height = Math.round(H * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    const small = W < 700;
    const CW = small ? 9 : 11;
    const CH = small ? 15 : 18;
    const FS = small ? 11 : 13;
    const cols = Math.ceil(W / CW);
    const rows = Math.ceil(H / CH);
    const N = cols * rows;
    const bits = new Uint8Array(N);
    const base = new Uint8Array(N);
    for (let i = 0; i < N; i++) {
      bits[i] = Math.random() < 0.5 ? 1 : 0;
      const r = Math.random();
      base[i] = r < 0.55 ? 0 : r < 0.88 ? 1 : r < 0.97 ? 2 : 3;
    }
    const mono = getComputedStyle(html).getPropertyValue('--mono').trim() || 'monospace';

    // 预渲染 0 / 1 的各级亮度，逐格 drawImage
    const ALPHAS = [0.09, 0.16, 0.26, 0.42, 0.68, 1];
    const sprites = [0, 1].map((b) => ALPHAS.map((a) => {
      const c = document.createElement('canvas');
      c.width = Math.ceil(CW * dpr);
      c.height = Math.ceil(CH * dpr);
      const g = c.getContext('2d');
      g.scale(dpr, dpr);
      g.font = `500 ${FS}px ${mono}`;
      g.textAlign = 'center';
      g.textBaseline = 'middle';
      g.fillStyle = `rgba(232, 236, 242, ${a})`;
      g.fillText(String(b), CW / 2, CH / 2 + 0.5);
      return c;
    }));

    const particles = sampleTitle();

    const TA = 1050;   // 二进制写满屏幕
    const TB = 1400;   // 汇聚成标题
    const TH = 220;    // 停留
    const TR = 1350;   // 从标题处向外展开，露出页面
    const total = TA + TB + TH + TR;

    const hudAddr = $('#hud-addr');
    const hudPct = $('#hud-pct');
    const hudStatus = $('#hud-status');
    const bar = $('#intro-bar');
    let hudDone = false;
    let readyAdded = false;
    let landed = false;
    let finished = false;
    let t0 = 0;
    let raf = 0;

    layer.style.background = 'transparent';

    // 展开的形状贴合标题：以标题中心为圆心、按标题宽高比拉伸的椭圆
    const tr0 = title.getBoundingClientRect();
    const cx = tr0.left + tr0.width / 2;
    const cy = tr0.top + tr0.height / 2;
    const AX = clamp(tr0.width / Math.max(tr0.height, 1) / 2.4, 1, 3);
    const irisMax = Math.max(...[[0, 0], [W, 0], [0, H], [W, H]].map(([x, y]) => Math.hypot((x - cx) / AX, y - cy)));
    const soft = irisMax * 0.42;
    particles.forEach((p) => { p.d = Math.hypot((p.tx - cx) / AX, p.ty - cy); });

    function drawBackdrop(R) {
      ctx.fillStyle = '#0a0b0d';
      ctx.fillRect(0, 0, W, H);
      if (R <= 0) return;
      ctx.save();
      ctx.globalCompositeOperation = 'destination-out';
      ctx.translate(cx, cy);
      ctx.scale(AX, 1);
      const g = ctx.createRadialGradient(0, 0, Math.max(0, R - soft), 0, 0, R);
      g.addColorStop(0, 'rgba(0, 0, 0, 1)');
      g.addColorStop(1, 'rgba(0, 0, 0, 0)');
      ctx.fillStyle = g;
      ctx.fillRect(-cx / AX - 2, -cy - 2, W / AX + 4, H + 4);
      ctx.restore();
    }

    function drawGrid(headRow, alpha) {
      ctx.globalAlpha = alpha;
      const maxRow = Math.min(rows, Math.ceil(headRow));
      for (let r = 0; r < maxRow; r++) {
        const d = headRow - r;
        const trail = d < 1 ? 5 : d < 2.5 ? 4 : d < 6 ? 3 : d < 12 ? 2 : 0;
        const y = r * CH;
        for (let c = 0; c < cols; c++) {
          const i = r * cols + c;
          const lv = Math.max(base[i], trail);
          ctx.drawImage(sprites[bits[i]][lv], c * CW, y, CW, CH);
        }
      }
      ctx.globalAlpha = 1;
    }

    function flicker(rate) {
      const n = Math.floor(N * rate);
      for (let k = 0; k < n; k++) {
        const i = (Math.random() * N) | 0;
        bits[i] ^= 1;
      }
    }

    function frame(now) {
      if (finished) return;
      if (!t0) t0 = now;
      const t = now - t0;
      ctx.clearRect(0, 0, W, H);

      // 背景：最后阶段从标题处向外柔和展开，露出下面的页面
      const tr = clamp((t - TA - TB - TH) / TR);
      const R = easeInOut(tr) * (irisMax + soft);
      drawBackdrop(R);

      if (t < TA) {
        const p = easeInOut(t / TA);
        const headRow = p * (rows + 2);
        flicker(0.006);
        drawGrid(headRow, 1);
        const pct = Math.floor(p * 100);
        hudPct.textContent = `${pct}%`;
        hudAddr.textContent = `0x${Math.floor(p * 0x1f400000).toString(16).toUpperCase().padStart(8, '0')}`;
        bar.style.width = `${p * 100}%`;
      } else {
        if (!hudDone) {
          hudDone = true;
          bar.style.width = '100%';
          hudStatus.textContent = `OKAY [ ${(TA / 1000).toFixed(3)}s ]`;
          hudStatus.classList.add('ok');
        }
        const tb = (t - TA) / TB;
        if (tb < 0.7) {
          flicker(0.01);
          drawGrid(rows + 20, 1 - easeOut(clamp(tb / 0.7)));
        }
        if (tb > 0.35) layer.classList.add('fading');
        drawParticles(clamp(tb, 0, 1), R, tr, t);
      }

      if (tr > 0.12 && !readyAdded) { readyAdded = true; html.classList.add('ready'); }
      if (tr > 0.3 && !landed) { landed = true; title.classList.add('landed'); }
      if (t >= total) { finish(); return; }
      raf = requestAnimationFrame(frame);
    }

    function drawParticles(tb, R, tr, t) {
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      // 标题实字淡入时，二进制字形同步淡出
      const fade = 1 - easeInOut(clamp((tr - 0.3) / 0.4));
      if (fade <= 0) return;
      let font = '';
      for (const p of particles) {
        const local = clamp((tb - p.delay) / 0.62);
        const e = easeInOut(local);
        const swirl = Math.sin(Math.PI * local) * p.swirl;
        const x = p.sx + (p.tx - p.sx) * e + p.nx * swirl;
        const y = p.sy + (p.ty - p.sy) * e + p.ny * swirl;
        if (local < 1 && Math.random() < 0.12) p.bit ^= 1;
        const f = `700 ${p.size}px ${mono}`;
        if (f !== font) { ctx.font = f; font = f; }
        // 展开的边缘经过时，字从白色过渡到标题颜色
        const mix = easeInOut(clamp((R - p.d) / (soft * 0.55)));
        const r = Math.round(255 + (p.color[0] - 255) * mix);
        const g = Math.round(255 + (p.color[1] - 255) * mix);
        const b = Math.round(255 + (p.color[2] - 255) * mix);
        const alpha = (0.55 + 0.45 * e) * fade;
        // 落位瞬间的一点高光
        const glow = local >= 1 && t < TA + TB + TH ? 1 : 0;
        ctx.fillStyle = `rgba(${r}, ${g}, ${b}, ${alpha})`;
        if (glow) { ctx.shadowColor = 'rgba(255,255,255,0.55)'; ctx.shadowBlur = 8; }
        ctx.fillText(String(p.bit), x, y);
        if (glow) ctx.shadowBlur = 0;
      }
    }

    function sampleTitle() {
      const off = document.createElement('canvas');
      off.width = W;
      off.height = H;
      const g = off.getContext('2d', { willReadFrequently: true });
      const out = [];
      $$('[data-intro]', title).forEach((span) => {
        const rect = span.getBoundingClientRect();
        if (rect.width === 0 || rect.bottom < 0 || rect.top > H) return;
        const cs = getComputedStyle(span);
        const fontSize = parseFloat(cs.fontSize);
        const step = Math.max(3, Math.round(fontSize / 15));
        g.clearRect(0, 0, W, H);
        g.font = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
        if ('letterSpacing' in g) g.letterSpacing = cs.letterSpacing;
        g.textBaseline = 'alphabetic';
        g.fillStyle = '#000';
        const text = span.textContent;
        const m = g.measureText(text);
        const ascent = m.fontBoundingBoxAscent ?? fontSize * 0.9;
        const descent = m.fontBoundingBoxDescent ?? fontSize * 0.25;
        const baseline = rect.top + (rect.height - (ascent + descent)) / 2 + ascent;
        g.fillText(text, rect.left, baseline);
        const x0 = Math.max(0, Math.floor(rect.left - 20));
        const y0 = Math.max(0, Math.floor(rect.top - 20));
        const x1 = Math.min(W, Math.ceil(rect.right + 20));
        const y1 = Math.min(H, Math.ceil(rect.bottom + 20));
        const data = g.getImageData(x0, y0, x1 - x0, y1 - y0).data;
        const color = parseColor(cs.color);
        const rw = x1 - x0;
        for (let y = 0; y < y1 - y0; y += step) {
          for (let x = 0; x < rw; x += step) {
            if (data[(y * rw + x) * 4 + 3] < 140) continue;
            const tx = x0 + x;
            const ty = y0 + y;
            const sc = (Math.random() * cols) | 0;
            const sr = (Math.random() * rows) | 0;
            const sx = sc * CW + CW / 2;
            const sy = sr * CH + CH / 2;
            const dx = tx - sx;
            const dy = ty - sy;
            const len = Math.hypot(dx, dy) || 1;
            out.push({
              sx, sy, tx, ty,
              nx: -dy / len, ny: dx / len,
              swirl: (Math.random() - 0.5) * Math.min(160, len * 0.35),
              delay: Math.random() * 0.26 + (tx / W) * 0.1,
              bit: Math.random() < 0.5 ? 1 : 0,
              size: Math.round(step * 1.45),
              color,
            });
          }
        }
      });
      return out;
    }

    function finish() {
      if (finished) return;
      finished = true;
      cancelAnimationFrame(raf);
      html.classList.add('ready');
      title.classList.add('landed');
      layer.classList.add('gone');
      setTimeout(() => {
        layer.remove();
        html.classList.remove('intro');
      }, 360);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('resize', finish);
    }
    const onKey = (e) => { if (['Escape', 'Enter', ' '].includes(e.key)) finish(); };

    $('#intro-skip').addEventListener('click', finish);
    layer.addEventListener('click', finish);
    window.addEventListener('keydown', onKey);
    window.addEventListener('resize', finish);
    raf = requestAnimationFrame(frame);
  }

  function parseColor(str) {
    const m = str.match(/\d+(\.\d+)?/g);
    return m ? m.slice(0, 3).map(Number) : [17, 19, 24];
  }
})();
