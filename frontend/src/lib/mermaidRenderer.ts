let mermaidModulePromise: Promise<typeof import('mermaid')> | null = null;
let panZoomModulePromise: Promise<any> | null = null;
let mermaidInitialized = false;
let mermaidRenderCount = 0;
let activeViewerSvg: SVGSVGElement | null = null;
let viewerTrigger: HTMLButtonElement | null = null;
let viewerRequest = 0;

// GitHub Octicons maximize-16 / screen-full-16 (MIT)。public/licenses/octicons.txt を参照。
const MAXIMIZE_PATH = 'M6.137 8.803a.749.749 0 1 1 1.06 1.06L3.811 13.25H6a.75.75 0 0 1 0 1.5H2a.75.75 0 0 1-.75-.75v-4a.75.75 0 0 1 1.5 0v2.19l3.387-3.387ZM14.75 6a.75.75 0 0 1-1.5 0V3.811L9.863 7.197a.749.749 0 1 1-1.06-1.06L12.19 2.75H10a.75.75 0 0 1 0-1.5h4a.75.75 0 0 1 .75.75v4Z';
const SCREEN_FULL_PATH = 'M1.75 10a.75.75 0 0 1 .75.75v2.5c0 .138.112.25.25.25h2.5a.75.75 0 0 1 0 1.5h-2.5A1.75 1.75 0 0 1 1 13.25v-2.5a.75.75 0 0 1 .75-.75Zm12.5 0a.75.75 0 0 1 .75.75v2.5A1.75 1.75 0 0 1 13.25 15h-2.5a.75.75 0 0 1 0-1.5h2.5a.25.25 0 0 0 .25-.25v-2.5a.75.75 0 0 1 .75-.75ZM2.75 2.5a.25.25 0 0 0-.25.25v2.5a.75.75 0 0 1-1.5 0v-2.5C1 1.784 1.784 1 2.75 1h2.5a.75.75 0 0 1 0 1.5ZM10 1.75a.75.75 0 0 1 .75-.75h2.5c.966 0 1.75.784 1.75 1.75v2.5a.75.75 0 0 1-1.5 0v-2.5a.25.25 0 0 0-.25-.25h-2.5a.75.75 0 0 1-.75-.75Z';

const INLINE_MAX_SCALE_ABS = 2.0;
const INLINE_MAX_FONT_PX = 16;
const INLINE_MAX_HEIGHT_VH_RATIO = 0.6;
const INLINE_MAX_HEIGHT_PX = 480;
const INLINE_MIN_LOGICAL_WIDTH = 480;
const INLINE_TEXT_MEASURE_CORRECTION = 0.50;

interface MermaidViewerModal {
  root: HTMLDivElement;
  panel: HTMLDivElement;
  canvas: HTMLDivElement;
  closeButton: HTMLButtonElement;
}

let viewerModal: MermaidViewerModal | null = null;

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

async function loadMermaid(): Promise<(typeof import('mermaid'))['default']> {
  if (!mermaidModulePromise) {
    mermaidModulePromise = import('mermaid');
  }
  const module = await mermaidModulePromise;
  return module.default;
}

async function loadPanZoom(): Promise<any> {
  if (!panZoomModulePromise) {
    panZoomModulePromise = import('svg-pan-zoom');
  }
  const module = await panZoomModulePromise;
  return module.default ?? module;
}

function ensureViewerModal(): MermaidViewerModal {
  if (viewerModal) {
    return viewerModal;
  }

  const root = document.createElement('div');
  root.className = 'mermaid-viewer-modal hidden';
  root.innerHTML = `
    <div class="mermaid-viewer-backdrop" data-mermaid-close="1"></div>
    <div class="mermaid-viewer-panel">
      <div class="mermaid-viewer-header">
        <span class="mermaid-viewer-title">Mermaid Viewer</span>
        <button type="button" class="mermaid-viewer-close" aria-label="close">閉じる</button>
      </div>
      <div class="mermaid-viewer-canvas"></div>
    </div>
  `;

  const panel = root.querySelector<HTMLDivElement>('.mermaid-viewer-panel');
  const canvas = root.querySelector<HTMLDivElement>('.mermaid-viewer-canvas');
  const closeButton = root.querySelector<HTMLButtonElement>('.mermaid-viewer-close');
  if (!panel || !canvas || !closeButton) {
    throw new Error('failed to initialize mermaid viewer');
  }

  root.addEventListener('click', (event) => {
    const target = event.target as HTMLElement;
    if (target.dataset.mermaidClose === '1' || target.classList.contains('mermaid-viewer-close')) {
      closeViewerModal();
    }
  });

  document.body.appendChild(root);
  viewerModal = {
    root,
    panel,
    canvas,
    closeButton,
  };
  return viewerModal;
}

function closeViewerModal(): void {
  const modal = ensureViewerModal();
  viewerRequest += 1;
  const trigger = viewerTrigger;
  viewerTrigger = null;
  if (activeViewerSvg) {
    const instance = (activeViewerSvg as any).__luwikiViewerPanZoom;
    if (instance && typeof instance.destroy === 'function') {
      instance.destroy();
    }
    delete (activeViewerSvg as any).__luwikiViewerPanZoom;
  }
  modal.root.classList.add('hidden');
  modal.root.classList.remove('is-fullscreen');
  modal.canvas.innerHTML = '';
  activeViewerSvg = null;
  const restoreFocus = () => {
    if (!viewerTrigger && trigger?.isConnected) trigger.focus({ preventScroll: true });
  };
  if (document.fullscreenElement === modal.panel) {
    void document.exitFullscreen().then(restoreFocus, restoreFocus);
  } else {
    restoreFocus();
  }
}

function toNumber(value: string | null): number | null {
  if (!value) {
    return null;
  }
  const parsed = Number.parseFloat(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return null;
  }
  return parsed;
}

function normalizeSvgElement(svg: SVGSVGElement): void {
  if (!svg.getAttribute('viewBox')) {
    const widthAttr = svg.getAttribute('width');
    const heightAttr = svg.getAttribute('height');
    const width = toNumber(widthAttr);
    const height = toNumber(heightAttr);
    if (width && height) {
      svg.setAttribute('viewBox', `0 0 ${width} ${height}`);
    }
    if (!svg.getAttribute('viewBox')) {
      try {
        const bbox = svg.getBBox();
        if (bbox.width > 0 && bbox.height > 0) {
          svg.setAttribute('viewBox', `${bbox.x} ${bbox.y} ${bbox.width} ${bbox.height}`);
        }
      } catch {
        // getBBox may fail before layout stabilization.
      }
    }
  }
  svg.style.width = '100%';
  svg.style.height = 'auto';
  svg.style.maxWidth = '100%';
  svg.style.display = 'block';
  svg.style.overflow = 'visible';
  svg.setAttribute('preserveAspectRatio', 'xMidYMid meet');
}

function getSvgViewBoxSize(svg: SVGSVGElement): { width: number; height: number } | null {
  const viewBox = svg.getAttribute('viewBox');
  if (viewBox) {
    const values = viewBox
      .trim()
      .split(/[\s,]+/)
      .map((value) => Number.parseFloat(value));
    if (values.length === 4) {
      const width = values[2];
      const height = values[3];
      if (Number.isFinite(width) && Number.isFinite(height) && width > 0 && height > 0) {
        return { width, height };
      }
    }
  }

  const width = toNumber(svg.getAttribute('width'));
  const height = toNumber(svg.getAttribute('height'));
  if (width && height) {
    return { width, height };
  }
  return null;
}

function getInlineHeightLimitPx(): number {
  const viewportHeight = Number.isFinite(window.innerHeight) ? window.innerHeight : INLINE_MAX_HEIGHT_PX;
  const computed = Math.min(viewportHeight * INLINE_MAX_HEIGHT_VH_RATIO, INLINE_MAX_HEIGHT_PX);
  return Math.max(240, computed);
}

function clampInlineAspectRatio(width: number, height: number): number {
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    return 1;
  }
  const ratio = width / height;
  return Math.min(1.5, Math.max(2 / 3, ratio));
}

function computeInlineScale(
  containerWidth: number,
  viewBoxWidth: number,
  viewBoxHeight: number,
): number {
  const scaleToContainer = containerWidth / viewBoxWidth;
  const maxScaleByLogicalWidth = containerWidth / INLINE_MIN_LOGICAL_WIDTH;
  const maxScaleByHeight = getInlineHeightLimitPx() / viewBoxHeight;

  return Math.min(
    scaleToContainer,
    INLINE_MAX_SCALE_ABS,
    maxScaleByLogicalWidth,
    maxScaleByHeight,
  );
}

function getInlineContainerWidth(
  block: HTMLElement | null,
  svg: SVGSVGElement,
): number {
  if (block && block.clientWidth > 0) {
    return block.clientWidth;
  }
  const parent = block?.parentElement as HTMLElement | null;
  if (parent && parent.clientWidth > 0) {
    return parent.clientWidth;
  }
  const fallback = (svg.parentElement as HTMLElement | null)?.clientWidth ?? 0;
  return fallback;
}

function measureMaxRenderedTextPx(svg: SVGSVGElement): number {
  const svgSize = getSvgViewBoxSize(svg);
  const svgRect = svg.getBoundingClientRect();
  const renderScale =
    svgSize && svgSize.width > 0 && Number.isFinite(svgRect.width) && svgRect.width > 0
      ? svgRect.width / svgSize.width
      : 0;

  let maxSize = 0;
  const textNodes = Array.from(svg.querySelectorAll<SVGTextElement>('text, tspan'));
  for (const node of textNodes) {
    const rect = node.getBoundingClientRect();
    if (Number.isFinite(rect.height) && rect.height > 0) {
      maxSize = Math.max(maxSize, rect.height);
      continue;
    }
    try {
      const bbox = node.getBBox();
      if (Number.isFinite(bbox.height) && bbox.height > 0 && renderScale > 0) {
        maxSize = Math.max(maxSize, bbox.height * renderScale);
        continue;
      }
    } catch {
      // Ignore and continue fallback.
    }
    const fontSize = toNumber(window.getComputedStyle(node).fontSize);
    if (fontSize && renderScale > 0) {
      maxSize = Math.max(maxSize, fontSize * renderScale);
    }
  }

  const foreignNodes = Array.from(svg.querySelectorAll<HTMLElement>('foreignObject *'));
  for (const node of foreignNodes) {
    if ((node.textContent ?? '').trim().length === 0) {
      continue;
    }
    const rect = node.getBoundingClientRect();
    if (Number.isFinite(rect.height) && rect.height > 0) {
      maxSize = Math.max(maxSize, rect.height);
      continue;
    }
    const fontSize = toNumber(window.getComputedStyle(node).fontSize);
    if (fontSize && renderScale > 0) {
      maxSize = Math.max(maxSize, fontSize * renderScale);
    }
  }

  return maxSize;
}

function applyInlineTargetWidth(
  svg: SVGSVGElement,
  targetWidthPx: number,
): void {
  const targetWidthStyle = `${Math.max(1, Math.floor(targetWidthPx))}px`;
  svg.style.setProperty('width', targetWidthStyle, 'important');
  svg.style.setProperty('max-width', '100%', 'important');
  svg.style.setProperty('margin-left', 'auto', 'important');
  svg.style.setProperty('margin-right', 'auto', 'important');
}

function enforceInlineMaxTextSize(
  svg: SVGSVGElement,
  currentTargetWidth: number,
): void {
  const renderedMaxText = measureMaxRenderedTextPx(svg) * INLINE_TEXT_MEASURE_CORRECTION;
  if (renderedMaxText <= 0 || renderedMaxText <= INLINE_MAX_FONT_PX) {
    return;
  }
  const shrinkRatio = INLINE_MAX_FONT_PX / renderedMaxText;
  const adjustedWidth = currentTargetWidth * shrinkRatio;
  applyInlineTargetWidth(svg, adjustedWidth);
}

function applyInlineSvgSizing(svg: SVGSVGElement): void {
  const size = getSvgViewBoxSize(svg);
  const block = svg.closest('pre.mermaid') as HTMLElement | null;
  const containerWidth = getInlineContainerWidth(block, svg);
  const maxHeightStyle = `${Math.ceil(getInlineHeightLimitPx())}px`;

  svg.style.setProperty('width', '100%', 'important');
  svg.style.setProperty('height', '100%', 'important');
  svg.style.setProperty('min-height', '0', 'important');
  svg.style.setProperty('max-width', '100%', 'important');
  svg.style.setProperty('max-height', '100%', 'important');
  svg.style.setProperty('overflow', 'hidden', 'important');

  if (block) {
    block.style.setProperty('height', 'auto', 'important');
    block.style.setProperty('max-height', maxHeightStyle, 'important');
    block.style.setProperty('overflow', 'hidden', 'important');
    block.style.setProperty('display', 'flex', 'important');
    block.style.setProperty('align-items', 'center', 'important');
    block.style.setProperty('justify-content', 'center', 'important');
    block.style.setProperty('width', '100%', 'important');
    block.style.setProperty('max-width', '100%', 'important');
    block.style.setProperty('margin-left', '0', 'important');
    block.style.setProperty('margin-right', '0', 'important');
    if (size) {
      const aspectRatio = clampInlineAspectRatio(size.width, size.height);
      block.style.setProperty('aspect-ratio', `${aspectRatio}`, 'important');
    } else {
      block.style.setProperty('aspect-ratio', '1', 'important');
    }

    if (size && containerWidth > 0) {
      const scale = computeInlineScale(containerWidth, size.width, size.height);
      const safeScale = Number.isFinite(scale) && scale > 0 ? scale : 1;
      const targetWidth = Math.min(containerWidth, size.width * safeScale);
      applyInlineTargetWidth(svg, targetWidth);
      enforceInlineMaxTextSize(svg, targetWidth);
    } else {
      svg.style.setProperty('width', '100%', 'important');
    }
  }
}

function fitViewBoxToContent(svg: SVGSVGElement): void {
  try {
    const bbox = svg.getBBox();
    if (bbox.width <= 0 || bbox.height <= 0) {
      return;
    }
    const padding = 8;
    const minX = bbox.x - padding;
    const minY = bbox.y - padding;
    const width = bbox.width + padding * 2;
    const height = bbox.height + padding * 2;
    svg.setAttribute('viewBox', `${minX} ${minY} ${width} ${height}`);
  } catch {
    // getBBox can fail when SVG is not yet fully laid out.
  }
}

function refitPanZoom(svg: SVGSVGElement | null): void {
  if (!svg) {
    return;
  }
  const instance = (svg as any).__luwikiViewerPanZoom;
  if (!instance) {
    return;
  }
  instance.resize();
  instance.fit();
  instance.center();
}

async function initializeViewerPanZoom(svg: SVGSVGElement): Promise<void> {
  const existing = (svg as any).__luwikiViewerPanZoom;
  if (existing) {
    existing.resize();
    existing.fit();
    existing.center();
    return;
  }

  const svgPanZoom = await loadPanZoom();
  if (!svg.isConnected) return;
  const instance = svgPanZoom(svg, {
    zoomEnabled: true,
    controlIconsEnabled: true,
    fit: true,
    center: true,
    minZoom: 0.2,
    maxZoom: 20,
    dblClickZoomEnabled: true,
    mouseWheelZoomEnabled: true,
  });
  (svg as any).__luwikiViewerPanZoom = instance;
  instance.resize();
  instance.fit();
  instance.center();
}

async function openViewer(
  source: string, mode: 'expanded' | 'fullscreen', trigger: HTMLButtonElement,
): Promise<void> {
  const modal = ensureViewerModal();
  const request = ++viewerRequest;
  viewerTrigger = trigger;
  modal.root.classList.remove('hidden');
  modal.root.classList.toggle('is-fullscreen', mode === 'fullscreen');
  modal.canvas.innerHTML = '<div class="mermaid-viewer-loading">Rendering...</div>';
  modal.closeButton.focus({ preventScroll: true });

  // Fullscreen の要求は操作時のユーザーアクティベーションが有効な間に行う。
  if (mode === 'fullscreen' && modal.panel.requestFullscreen) {
    void modal.panel.requestFullscreen().then(() => {
      if (request !== viewerRequest && !viewerTrigger) {
        return document.exitFullscreen();
      }
      refitPanZoom(activeViewerSvg);
    }).catch(() => { /* 全画面を利用できない場合もビューアーを表示する。 */ });
  }

  try {
    const mermaid = await loadMermaid();
    const renderId = `luwiki-mermaid-viewer-${mermaidRenderCount++}`;
    const { svg } = await mermaid.render(renderId, source);
    if (request !== viewerRequest) return;
    modal.canvas.innerHTML = svg;
    const svgElement = modal.canvas.querySelector<SVGSVGElement>('svg');
    if (!svgElement) return;
    normalizeSvgElement(svgElement);
    fitViewBoxToContent(svgElement);
    svgElement.style.height = '100%';
    await initializeViewerPanZoom(svgElement);
    if (request !== viewerRequest) return;
    activeViewerSvg = svgElement;
    requestAnimationFrame(() => refitPanZoom(activeViewerSvg));
  } catch {
    if (request !== viewerRequest) return;
    const escaped = escapeHtml(source);
    modal.canvas.innerHTML = `<code class="text-error">Mermaid render error</code><code>${escaped}</code>`;
  }
}

/** 描画した図の操作をコピーと共通のボタン領域へ追加する。 */
function decorateBlock(block: HTMLElement, source: string): void {
  block.classList.add('mermaid-block');
  const panel = block.closest('.luwiki-code-block');
  const toolbar = panel?.querySelector('.code-copy-toolbar');
  if (!toolbar) return;
  const copyButton = toolbar.querySelector('[data-code-copy-id]');

  for (const [mode, label, icon] of [
    ['expanded', '拡大表示', MAXIMIZE_PATH],
    ['fullscreen', '全画面', SCREEN_FULL_PATH],
  ] as const) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'code-copy-button';
    button.setAttribute('aria-label', label);
    button.innerHTML = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"
      viewBox="0 0 16 16" fill="currentColor" aria-hidden="true" focusable="false">
      <path d="${icon}"/></svg>`;
    button.addEventListener('click', () => { void openViewer(source, mode, button); });
    toolbar.insertBefore(button, copyButton);
  }
  panel?.classList.add('has-mermaid-actions');
}

export async function renderMermaidBlocks(container: HTMLElement): Promise<void> {
  const blocks = Array.from(container.querySelectorAll<HTMLElement>('pre.mermaid'));
  if (blocks.length === 0) {
    return;
  }

  const mermaid = await loadMermaid();
  if (!mermaidInitialized) {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: 'strict',
    });
    mermaidInitialized = true;
  }

  for (const block of blocks) {
    if (block.classList.contains('mermaid-block')) continue;
    const source = block.textContent ?? '';
    if (source.trim().length === 0) continue;

    const renderId = `luwiki-mermaid-${mermaidRenderCount}`;
    mermaidRenderCount += 1;

    if (await mermaid.parse(source, {suppressErrors: true})) {
      try {
        const { svg } = await mermaid.render(renderId, source);
        if (!block.isConnected || block.classList.contains('mermaid-block')) continue;
        block.innerHTML = svg;
        decorateBlock(block, source);
        const svgElement = block.querySelector<SVGSVGElement>('svg');
        if (svgElement) {
          normalizeSvgElement(svgElement);
          fitViewBoxToContent(svgElement);
          applyInlineSvgSizing(svgElement);
          requestAnimationFrame(() => {
            fitViewBoxToContent(svgElement);
            applyInlineSvgSizing(svgElement);
          });
        }
      } catch {
        if (!block.isConnected || block.classList.contains('mermaid-block')) continue;
        const escaped = escapeHtml(source);
        block.innerHTML = `<code class="text-error">Mermaid render error</code><code>${escaped}</code>`;
      }
    }
  }
}

document.addEventListener('fullscreenchange', () => {
  if (viewerModal && !viewerModal.root.classList.contains('hidden')) {
    const fullscreenActive = document.fullscreenElement === viewerModal.panel;
    if (!fullscreenActive && viewerModal.root.classList.contains('is-fullscreen')) {
      viewerModal.closeButton.click();
      return;
    }
  }
  refitPanZoom(activeViewerSvg);
});

document.addEventListener('keydown', (event) => {
  if (event.key !== 'Escape') {
    return;
  }
  if (!viewerModal || viewerModal.root.classList.contains('hidden')) {
    return;
  }
  viewerModal.closeButton.click();
});

window.addEventListener('resize', () => {
  window.requestAnimationFrame(() => {
    const inlineSvgs = document.querySelectorAll<SVGSVGElement>('pre.mermaid svg');
    inlineSvgs.forEach((svg) => {
      applyInlineSvgSizing(svg);
    });
    refitPanZoom(activeViewerSvg);
  });
});
