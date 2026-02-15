let mermaidModulePromise: Promise<typeof import('mermaid')> | null = null;
let panZoomModulePromise: Promise<any> | null = null;
let mermaidInitialized = false;
let mermaidRenderCount = 0;
const panZoomBySvg = new WeakMap<SVGSVGElement, any>();
let activeViewerSvg: SVGSVGElement | null = null;

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
  modal.root.classList.add('hidden');
  modal.root.classList.remove('is-fullscreen');
  modal.canvas.innerHTML = '';
  activeViewerSvg = null;
  if (document.fullscreenElement === modal.panel) {
    void document.exitFullscreen();
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

function applyInlineSvgSizing(svg: SVGSVGElement): void {
  svg.style.setProperty('width', '100%', 'important');
  svg.style.setProperty('height', 'auto', 'important');
  svg.style.setProperty('min-height', '0', 'important');
  svg.style.setProperty('max-width', '100%', 'important');
  svg.style.setProperty('overflow', 'visible', 'important');

  let parent: HTMLElement | null = svg.parentElement;
  while (parent && parent.tagName !== 'PRE') {
    parent.style.setProperty('width', '100%', 'important');
    parent.style.setProperty('height', 'auto', 'important');
    parent.style.setProperty('min-height', '0', 'important');
    parent.style.setProperty('max-height', 'none', 'important');
    parent.style.setProperty('overflow', 'visible', 'important');
    parent = parent.parentElement;
  }

  const block = svg.closest('pre.mermaid');
  if (block) {
    (block as HTMLElement).style.setProperty('height', 'auto', 'important');
    (block as HTMLElement).style.setProperty('max-height', 'none', 'important');
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
  const instance = panZoomBySvg.get(svg);
  if (!instance) {
    return;
  }
  instance.resize();
  if (svg === activeViewerSvg) {
    instance.fit();
    instance.center();
  } else {
    applyInlineSvgSizing(svg);
  }
}

async function initializePanZoom(svg: SVGSVGElement, mode: 'inline' | 'viewer'): Promise<void> {
  const existing = panZoomBySvg.get(svg);
  if (existing) {
    existing.resize();
    if (mode === 'viewer') {
      existing.fit();
      existing.center();
    }
    return;
  }

  const svgPanZoom = await loadPanZoom();
  const instance = svgPanZoom(svg, {
    zoomEnabled: true,
    controlIconsEnabled: mode === 'viewer',
    fit: mode === 'viewer',
    center: mode === 'viewer',
    minZoom: 0.2,
    maxZoom: 20,
    dblClickZoomEnabled: true,
    mouseWheelZoomEnabled: true,
  });
  panZoomBySvg.set(svg, instance);
  if (mode === 'viewer') {
    instance.resize();
    instance.fit();
    instance.center();
  }
}

async function openViewer(source: string, mode: 'expanded' | 'fullscreen'): Promise<void> {
  const modal = ensureViewerModal();
  modal.root.classList.remove('hidden');
  modal.root.classList.toggle('is-fullscreen', mode === 'fullscreen');
  modal.canvas.innerHTML = '<div class="mermaid-viewer-loading">Rendering...</div>';

  const mermaid = await loadMermaid();
  const renderId = `luwiki-mermaid-viewer-${mermaidRenderCount}`;
  mermaidRenderCount += 1;

  try {
    const { svg } = await mermaid.render(renderId, source);
    modal.canvas.innerHTML = svg;
    const svgElement = modal.canvas.querySelector<SVGSVGElement>('svg');
    if (svgElement) {
      normalizeSvgElement(svgElement);
      fitViewBoxToContent(svgElement);
      svgElement.style.height = '100%';
      await initializePanZoom(svgElement, 'viewer');
      activeViewerSvg = svgElement;
      requestAnimationFrame(() => {
        refitPanZoom(svgElement);
      });
    }
    if (mode === 'fullscreen' && modal.panel.requestFullscreen) {
      await modal.panel.requestFullscreen();
      requestAnimationFrame(() => {
        refitPanZoom(svgElement ?? null);
      });
      window.setTimeout(() => {
        refitPanZoom(svgElement ?? null);
      }, 180);
    }
  } catch {
    const escaped = escapeHtml(source);
    modal.canvas.innerHTML = `<code class="text-error">Mermaid render error</code><code>${escaped}</code>`;
  }
}

function decorateBlock(block: HTMLElement, source: string): void {
  block.classList.add('mermaid-block');
  block.setAttribute('data-mermaid-source', source);

  let toolbar = block.querySelector<HTMLElement>('.mermaid-toolbar');
  if (!toolbar) {
    toolbar = document.createElement('div');
    toolbar.className = 'mermaid-toolbar';
    toolbar.innerHTML = `
      <button type="button" class="btn btn-xs btn-outline mermaid-action-expand">拡大表示</button>
      <button type="button" class="btn btn-xs btn-outline mermaid-action-fullscreen">全画面</button>
    `;
    block.prepend(toolbar);
  }

  const expandButton = toolbar.querySelector<HTMLButtonElement>('.mermaid-action-expand');
  const fullscreenButton = toolbar.querySelector<HTMLButtonElement>('.mermaid-action-fullscreen');
  if (expandButton) {
    expandButton.onclick = () => {
      void openViewer(source, 'expanded');
    };
  }
  if (fullscreenButton) {
    fullscreenButton.onclick = () => {
      void openViewer(source, 'fullscreen');
    };
  }
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
      suppressErrorRendering: true,
    });
    mermaidInitialized = true;
  }

  for (const block of blocks) {
    const source = block.textContent ?? '';
    if (source.trim().length === 0) {
      continue;
    }

    const previousSource = block.getAttribute('data-mermaid-source');
    if (previousSource === source && block.querySelector('svg')) {
      continue;
    }

    const renderId = `luwiki-mermaid-${mermaidRenderCount}`;
    mermaidRenderCount += 1;

    try {
      const { svg } = await mermaid.render(renderId, source);
      block.innerHTML = svg;
      decorateBlock(block, source);
      const svgElement = block.querySelector<SVGSVGElement>('svg');
      if (svgElement) {
        normalizeSvgElement(svgElement);
        fitViewBoxToContent(svgElement);
        await initializePanZoom(svgElement, 'inline');
        applyInlineSvgSizing(svgElement);
        requestAnimationFrame(() => {
          fitViewBoxToContent(svgElement);
          applyInlineSvgSizing(svgElement);
          refitPanZoom(svgElement);
        });
      }
    } catch {
      const escaped = escapeHtml(source);
      block.innerHTML = `<code class="text-error">Mermaid render error</code><code>${escaped}</code>`;
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
