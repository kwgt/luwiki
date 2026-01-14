import { computed, ref, watch } from 'vue';
import {
  acquirePageLock,
  deletePage,
  fetchParentPage,
  fetchPageMeta,
  fetchPageSource,
  renamePagePath,
  unlockPageLock,
  type PageMetaResponse,
} from '../api/pages';
import {
  deleteAsset,
  fetchAssetMeta,
  fetchPageAssets,
  type PageAsset,
  uploadPageAsset,
} from '../api/assets';
import {
  createMarkdownRenderer,
  extractToc,
  extractTitle,
  formatBytes,
  getMetaContent,
  normalizeWikiPath,
  parseAssetMaxBytes,
  resolvePagePath,
  toErrorMessage,
} from '../lib/pageCommon';
import { tryBuildLockTokenKey } from '../lib/lockToken';
import { buildAmendRefreshKey } from '../lib/amendRefresh';


function copyToClipboard(text: string): void {
  if (!navigator.clipboard || !window.isSecureContext) {
    console.warn('clipboard not available', {
      secureContext: window.isSecureContext,
      hasClipboard: !!navigator.clipboard,
    });
    return;
  }
  console.log('[clipboard] write request', text);
  navigator.clipboard.writeText(text).then(() => {
    console.log('[clipboard] write success');
  }).catch((err) => {
    console.warn('clipboard write failed', err);
  });
}

function isRequestError(err: unknown): err is { status?: number } {
  return typeof err === 'object' && err !== null && 'status' in err;
}

export function usePageView() {
  const pageId = ref('');
  const revision = ref<number | null>(null);
  const pagePath = ref('');
  const source = ref('');
  const assets = ref<PageAsset[]>([]);
  const isLoading = ref(false);
  const isUploading = ref(false);
  const uploadProgress = ref<number | null>(null);
  const uploadingFileName = ref('');
  const uploadingIndex = ref(0);
  const uploadingTotal = ref(0);
  const assetMaxBytes = ref<number | null>(null);
  const copyToastVisible = ref(false);
  const copyToastName = ref('');
  const pageMeta = ref<PageMetaResponse | null>(null);
  const pageMetaOpen = ref(false);
  const assetDetails = ref<PageAsset | null>(null);
  const assetMetaDetails = ref<{
    file_name: string;
    mime_type: string;
    size: number;
    timestamp: string;
    username: string;
  } | null>(null);
  const assetDetailsLoading = ref(false);
  const assetDeleteTarget = ref<PageAsset | null>(null);
  const assetDeleteLoading = ref(false);
  const pageDeleteOpen = ref(false);
  const pageDeleteLoading = ref(false);
  const pageDeleteRecursive = ref(false);
  const pageMoveOpen = ref(false);
  const pageMoveLoading = ref(false);
  const pageMoveRecursive = ref(false);
  const pageMoveTarget = ref('');
  const pageMoveError = ref('');
  const errorMessage = ref('');

  const pageTitle = computed(() => extractTitle(source.value, pagePath.value));
  const tocEntries = computed(() => extractToc(source.value));
  const wikiUrl = computed(() => {
    if (!pagePath.value) {
      return '/wiki/';
    }
    return pagePath.value === '/' ? '/wiki/' : `/wiki${pagePath.value}`;
  });
  const interactionDisabled = computed(
    () =>
      isLoading.value ||
      errorMessage.value.length > 0 ||
      pageDeleteLoading.value,
  );
  const assetInteractionDisabled = computed(
    () => interactionDisabled.value || isUploading.value,
  );
  const assetUploadAllowed = computed(
    () => assetMaxBytes.value !== null && assetMaxBytes.value > 0,
  );
  const assetUploadDisabled = computed(
    () => assetInteractionDisabled.value || !assetUploadAllowed.value,
  );
  const pageMoveResolvedTarget = computed(() => {
    const raw = pageMoveTarget.value.trim();
    if (!raw) {
      return null;
    }
    const base = pagePath.value || '/';
    const hasTrailingSlash = raw.endsWith('/');
    const resolved = resolvePagePath(base, raw);
    if (!resolved) {
      return null;
    }
    // 末尾"/"指定はページ名再利用の意図なので、解決後も末尾"/"を保持する
    if (hasTrailingSlash && resolved !== '/') {
      return `${resolved}/`;
    }
    return resolved;
  });
  const pageMovePreviewPath = computed(() => {
    const raw = pageMoveTarget.value.trim();
    if (!raw) {
      return '';
    }
    const base = pagePath.value || '/';
    const hasTrailingSlash = raw.endsWith('/');
    const resolved = resolvePagePath(base, raw);
    if (!resolved) {
      return '';
    }
    if (!hasTrailingSlash) {
      return resolved;
    }
    const normalizedCurrent = normalizeWikiPath(pagePath.value || '/');
    const trimmed = normalizedCurrent.replace(/\/+$/g, '');
    const slashIndex = trimmed.lastIndexOf('/');
    const suffix = slashIndex >= 0 ? trimmed.slice(slashIndex + 1) : '';
    if (!suffix) {
      return resolved;
    }
    if (resolved.endsWith('/')) {
      return `${resolved}${suffix}`;
    }
    return `${resolved}/${suffix}`;
  });
  const pageMoveInputError = computed(() => {
    const preview = pageMovePreviewPath.value;
    if (!preview) {
      return '';
    }
    const normalizedPreview = preview === '/'
      ? '/'
      : preview.replace(/\/+$/g, '');
    const normalizedCurrent = normalizeWikiPath(pagePath.value || '/');
    const normalizedCurrentTrimmed = normalizedCurrent === '/'
      ? '/'
      : normalizedCurrent.replace(/\/+$/g, '');

    if (normalizedPreview === "/") {
      return '移動先がルートページです';
    }

    if (normalizedPreview === normalizedCurrentTrimmed) {
      return '移動先が元のパスと同じです';
    }

    if (normalizedCurrentTrimmed !== '/') {
      const basePrefix = `${normalizedCurrentTrimmed}/`;
      if (normalizedPreview.startsWith(basePrefix)) {
        return '移動先が元のパス配下です';
      }
    }
    return '';
  });

  const renderedHtml = computed(() => {
    if (!pagePath.value) {
      return '';
    }
    const md = createMarkdownRenderer(pagePath.value, '/wiki');
    return md.render(source.value);
  });

  const assetItems = computed(() =>
    [...assets.value]
      .sort((left, right) =>
        left.file_name.localeCompare(right.file_name, undefined, {
          numeric: true,
          sensitivity: 'base',
        }),
      )
      .map((item) => ({
        ...item,
        formattedSize: formatBytes(item.size),
      })),
  );

  function resolveUploadFileName(file: File): string {
    return file.name;
  }

  async function loadPage(): Promise<void> {
    const rawPageId = getMetaContent('wiki-page-id');
    const rawRevision = getMetaContent('wiki-page-revision');
    const rawAssetMaxBytes = getMetaContent('asset-max-bytes');

    if (!rawPageId) {
      errorMessage.value = 'page id not found';
      return;
    }

    pageId.value = rawPageId;
    revision.value = rawRevision ? Number(rawRevision) : null;
    assetMaxBytes.value = parseAssetMaxBytes(rawAssetMaxBytes);
    const refreshKey = buildAmendRefreshKey(pageId.value);
    const noCache = sessionStorage.getItem(refreshKey) === '1';
    if (noCache) {
      sessionStorage.removeItem(refreshKey);
    }

    isLoading.value = true;
    errorMessage.value = '';

    try {
      const [meta, markdown, pageAssets] = await Promise.all([
        fetchPageMeta(pageId.value, revision.value ?? undefined, noCache),
        fetchPageSource(pageId.value, revision.value ?? undefined, noCache),
        fetchPageAssets(pageId.value, Date.now()),
      ]);

      pageMeta.value = meta;
      pagePath.value = normalizeWikiPath(meta.page_info.path.value);
      source.value = markdown;
      assets.value = pageAssets;
    } catch (err: unknown) {
      errorMessage.value = toErrorMessage(err);
    } finally {
      isLoading.value = false;
    }
  }

  async function uploadAssets(files: File[]): Promise<void> {
    if (isUploading.value || files.length === 0) {
      return;
    }
    if (!pageId.value) {
      reportError(new Error('page id not found'));
      return;
    }
    if (!assetUploadAllowed.value) {
      reportError(new Error('アセットの最大サイズが未設定のためアップロードできません'));
      return;
    }
    const limit = assetMaxBytes.value ?? 0;
    const tooLarge = files.find((file) => file.size > limit);
    if (tooLarge) {
      reportError(new Error('ファイルサイズが大きすぎます'));
      return;
    }

    isUploading.value = true;
    let uploaded = false;
    let uploadedName: string | null = null;
    uploadProgress.value = 0;
    uploadingTotal.value = files.length;
    try {
      for (const [index, file] of files.entries()) {
        const fileName = resolveUploadFileName(file);
        if (!fileName) {
          throw new Error('file name not found');
        }
        uploadingIndex.value = index + 1;
        uploadingFileName.value = fileName;
        uploadProgress.value = 0;
        await uploadPageAsset(
          pageId.value,
          fileName,
          file,
          file.type,
          (loaded, total) => {
            if (!total || total <= 0) {
              return;
            }
            const percent = Math.min(100, Math.floor((loaded / total) * 100));
            uploadProgress.value = percent;
          },
          undefined,
        );
        uploadProgress.value = 100;
        uploaded = true;
        uploadedName = fileName;
      }
    } catch (err: unknown) {
      reportError(err);
    } finally {
      isUploading.value = false;
      uploadProgress.value = null;
      uploadingFileName.value = '';
      uploadingIndex.value = 0;
      uploadingTotal.value = 0;
      if (uploaded) {
        try {
          const pageAssets = await fetchPageAssets(pageId.value, Date.now());
          assets.value = pageAssets;
        } catch (err: unknown) {
          if (!errorMessage.value) {
            reportError(err);
          }
        }
        if (files.length === 1 && uploadedName) {
          copyToastName.value = uploadedName;
          copyToastVisible.value = true;
        }
      }
    }
  }

  function dismissError(): void {
    errorMessage.value = '';
  }

  function openPageDeleteConfirm(): void {
    if (!pageId.value) {
      reportError(new Error('page id not found'));
      return;
    }
    pageDeleteRecursive.value = false;
    pageDeleteOpen.value = true;
  }

  function dismissPageDeleteConfirm(): void {
    pageDeleteOpen.value = false;
    pageDeleteRecursive.value = false;
  }

  function buildParentPath(path: string): string {
    const normalized = normalizeWikiPath(path);
    if (normalized === '/') {
      return '/';
    }
    const trimmed = normalized.replace(/\/+$/g, '');
    const slashIndex = trimmed.lastIndexOf('/');
    if (slashIndex <= 0) {
      return '/';
    }
    return `${trimmed.slice(0, slashIndex)}/`;
  }

  function openPageMoveConfirm(): void {
    if (!pageId.value) {
      pageMoveError.value = 'page id not found';
      return;
    }
    const parentPath = buildParentPath(pagePath.value || '/');
    pageMoveTarget.value = parentPath;
    pageMoveRecursive.value = false;
    pageMoveError.value = '';
    pageMoveOpen.value = true;
  }

  function dismissPageMoveConfirm(): void {
    pageMoveOpen.value = false;
    pageMoveError.value = '';
    pageMoveLoading.value = false;
  }

  async function confirmPageMove(): Promise<void> {
    if (!pageId.value || pageMoveLoading.value) {
      return;
    }

    if (pageMoveInputError.value) {
      pageMoveError.value = pageMoveInputError.value;
      return;
    }

    const resolved = pageMoveResolvedTarget.value;
    if (!resolved) {
      pageMoveError.value = '移動先パスが不正です';
      return;
    }
    pageMoveLoading.value = true;
    pageMoveError.value = '';
    try {
      await renamePagePath(
        pageId.value,
        resolved,
        pageMoveRecursive.value,
      );
      const nextUrl = resolved === '/' ? '/wiki/' : `/wiki${resolved}`;
      window.location.replace(nextUrl);
    } catch (err: unknown) {
      pageMoveError.value = toErrorMessage(err);
    } finally {
      pageMoveLoading.value = false;
    }
  }

  async function confirmPageDelete(): Promise<void> {
    if (!pageId.value || pageDeleteLoading.value) {
      return;
    }
    pageDeleteLoading.value = true;
    try {
      let redirectPath = '/';
      try {
        const parent = await fetchParentPage(pageId.value, true);
        redirectPath = parent.path;
      } catch (err: unknown) {
        reportError(err);
      }
      const tokenKey = tryBuildLockTokenKey(pageId.value);
      const lockToken = tokenKey
        ? sessionStorage.getItem(tokenKey) ?? undefined
        : undefined;
      await deletePage(pageId.value, lockToken, pageDeleteRecursive.value);
      const nextUrl = redirectPath === '/' ? '/wiki/' : `/wiki${redirectPath}`;
      window.location.replace(nextUrl);
    } catch (err: unknown) {
      reportError(err);
    } finally {
      pageDeleteLoading.value = false;
      pageDeleteOpen.value = false;
    }
  }

  async function cleanupViewLock(): Promise<void> {
    if (!pageId.value) {
      return;
    }
    const tokenKey = tryBuildLockTokenKey(pageId.value);
    if (!tokenKey) {
      return;
    }
    const lockToken = sessionStorage.getItem(tokenKey);
    if (!lockToken) {
      return;
    }
    try {
      await unlockPageLock(pageId.value, lockToken);
      sessionStorage.removeItem(tokenKey);
    } catch (err: unknown) {
      if (isRequestError(err) && err.status === 404) {
        sessionStorage.removeItem(tokenKey);
        return;
      }
      reportError(err);
    }
  }

  function openPageMeta(): void {
    if (!pageMeta.value) {
      return;
    }
    pageMetaOpen.value = true;
  }

  function dismissPageMeta(): void {
    pageMetaOpen.value = false;
  }

  function dismissCopyToast(): void {
    copyToastVisible.value = false;
  }

  function requestCopyName(name: string): void {
    if (!name) {
      return;
    }
    copyToClipboard(name);
  }

  function dismissAssetDetails(): void {
    assetDetails.value = null;
    assetMetaDetails.value = null;
    assetDetailsLoading.value = false;
  }

  async function openAssetDetails(asset: PageAsset): Promise<void> {
    assetDetails.value = asset;
    assetMetaDetails.value = null;
    assetDetailsLoading.value = true;
    try {
      const meta = await fetchAssetMeta(asset.id);
      assetMetaDetails.value = meta;
    } catch (err: unknown) {
      reportError(err);
    } finally {
      assetDetailsLoading.value = false;
    }
  }

  function openAssetDeleteConfirm(asset: PageAsset): void {
    assetDeleteTarget.value = asset;
  }

  function dismissAssetDeleteConfirm(): void {
    assetDeleteTarget.value = null;
  }

  async function confirmAssetDelete(): Promise<void> {
    if (!assetDeleteTarget.value || assetDeleteLoading.value) {
      return;
    }
    assetDeleteLoading.value = true;
    try {
      await deleteAsset(assetDeleteTarget.value.id);
      assetDeleteTarget.value = null;
      const pageAssets = await fetchPageAssets(pageId.value, Date.now());
      assets.value = pageAssets;
    } catch (err: unknown) {
      reportError(err);
    } finally {
      assetDeleteLoading.value = false;
    }
  }

  async function requestEditLock(): Promise<string> {
    if (!pageId.value) {
      throw new Error('page id not found');
    }
    return acquirePageLock(pageId.value);
  }

  function reportError(err: unknown): void {
    errorMessage.value = toErrorMessage(err);
  }

  watch(pageMoveTarget, () => {
    if (pageMoveError.value) {
      pageMoveError.value = '';
    }
  });
  watch(pageMoveInputError, () => {
    if (pageMoveError.value) {
      pageMoveError.value = '';
    }
  });

  return {
    pageId,
    pageTitle,
    pagePath,
    wikiUrl,
    tocEntries,
    renderedHtml,
    assetItems,
    isLoading,
    isUploading,
    uploadProgress,
    uploadingFileName,
    uploadingIndex,
    uploadingTotal,
    assetMaxBytes,
    assetUploadAllowed,
    assetUploadDisabled,
    copyToastVisible,
    copyToastName,
    pageMeta,
    pageMetaOpen,
    assetDetails,
    assetMetaDetails,
    assetDetailsLoading,
    assetDeleteTarget,
    assetDeleteLoading,
    assetInteractionDisabled,
    interactionDisabled,
    errorMessage,
    pageDeleteOpen,
    pageDeleteLoading,
    pageDeleteRecursive,
    pageMoveOpen,
    pageMoveLoading,
    pageMoveRecursive,
    pageMoveTarget,
    pageMoveResolvedTarget,
    pageMovePreviewPath,
    pageMoveInputError,
    pageMoveError,
    loadPage,
    uploadAssets,
    openPageMeta,
    dismissPageMeta,
    openPageDeleteConfirm,
    dismissPageDeleteConfirm,
    confirmPageDelete,
    openPageMoveConfirm,
    dismissPageMoveConfirm,
    confirmPageMove,
    requestCopyName,
    dismissCopyToast,
    openAssetDetails,
    dismissAssetDetails,
    openAssetDeleteConfirm,
    dismissAssetDeleteConfirm,
    confirmAssetDelete,
    requestEditLock,
    cleanupViewLock,
    reportError,
    dismissError,
  };
}
