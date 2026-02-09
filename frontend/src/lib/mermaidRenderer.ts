let mermaidModulePromise: Promise<typeof import('mermaid')> | null = null;
let mermaidInitialized = false;
let mermaidRenderCount = 0;

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
      block.setAttribute('data-mermaid-source', source);
    } catch {
      const escaped = escapeHtml(source);
      block.innerHTML = `<code class="text-error">Mermaid render error</code><code>${escaped}</code>`;
    }
  }
}
