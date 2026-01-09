import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import {
  createPage,
  acquirePageLock,
  extendPageLock,
  fetchDeletedPageCandidates,
  fetchPageLockInfo,
  fetchPageMeta,
  fetchPageSource,
  restorePagePath,
  unlockPageLock,
  updatePageSource,
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
  formatBytes,
  getMetaContent,
  normalizeWikiPath,
  parseAssetMaxBytes,
  toErrorMessage,
} from '../lib/pageCommon';
import { buildLockTokenKey } from '../lib/lockToken';
import { buildAmendRefreshKey } from '../lib/amendRefresh';

const LOCK_EXTEND_INTERVAL = 60 * 1000;
const AMEND_MAX_CHAR_DIFF = 20;
const AMEND_MAX_LINE_DIFF = 2;

function normalizeLineBreaks(text: string): string {
  return text.replace(/\r\n/g, '\n').replace(/\r/g, '\n');
}

function resolveEditPath(): string {
  const raw = window.location.pathname;
  const trimmed = raw.replace(/^\/edit\/?/, '');
  if (!trimmed) {
    return '/';
  }
  const decoded = trimmed
    .split('/')
    .map((segment) => {
      try {
        return decodeURIComponent(segment);
      } catch {
        return segment;
      }
    })
    .join('/');
  return normalizeWikiPath(decoded);
}

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

export function usePageEdit() {
  const pageId = ref('');
  const revision = ref<number | null>(null);
  const pagePath = ref(resolveEditPath());
  const source = ref('');
  const initialSource = ref('');
  const assets = ref<PageAsset[]>([]);
  const isLoading = ref(false);
  const isSaving = ref(false);
  const isUploading = ref(false);
  const uploadProgress = ref<number | null>(null);
  const uploadingFileName = ref('');
  const uploadingIndex = ref(0);
  const uploadingTotal = ref(0);
  const assetMaxBytes = ref<number | null>(null);
  const copyToastVisible = ref(false);
  const copyToastName = ref('');
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
  const errorMessage = ref('');
  const lockToken = ref('');
  const lockExtendTimer = ref<number | null>(null);
  const draftCreated = ref(false);
  const restoreCandidates = ref<string[]>([]);
  const restoreCandidateId = ref('');
  const restoreCandidateSource = ref('');
  const restoreCandidateLoading = ref(false);
  const restoreInProgress = ref(false);
  const restoreRecursive = ref(false);
  const amendChecked = ref(true);
  const amendUserTouched = ref(false);
  const suppressAutoAmend = ref(false);
  const lastRevisionUsername = ref<string | null>(null);
  const lockUsername = ref<string | null>(null);
  const newPageToastVisible = ref(false);
  const newPageToastMessage = ref('');

  const wikiUrl = computed(() => {
    if (!pagePath.value) {
      return '/wiki/';
    }
    return pagePath.value === '/' ? '/wiki/' : `/wiki${pagePath.value}`;
  });
  const isNewPage = computed(() => !pageId.value);
  const interactionDisabled = computed(
    () =>
      isLoading.value ||
      isSaving.value ||
      errorMessage.value.length > 0 ||
      restoreCandidates.value.length > 0,
  );
  const assetInteractionDisabled = computed(
    () => interactionDisabled.value || isUploading.value,
  );
  const assetUploadAllowed = computed(
    () => !isNewPage.value && assetMaxBytes.value !== null && assetMaxBytes.value > 0,
  );
  const assetUploadDisabled = computed(
    () => assetInteractionDisabled.value || !assetUploadAllowed.value,
  );
  const isDraftPage = computed(() => draftCreated.value);
  const normalizedSource = computed(() => normalizeLineBreaks(source.value));
  const normalizedInitialSource = computed(() => normalizeLineBreaks(initialSource.value));
  const isDirty = computed(() => normalizedSource.value !== normalizedInitialSource.value);
  const sourceCharDiff = computed(
    () => Math.abs(normalizedSource.value.length - normalizedInitialSource.value.length),
  );
  const sourceLineDiff = computed(() => {
    const currentLines = normalizedSource.value.split('\n').length;
    const baseLines = normalizedInitialSource.value.split('\n').length;
    return Math.abs(currentLines - baseLines);
  });
  const amendAutoAllowed = computed(
    () => sourceCharDiff.value <= AMEND_MAX_CHAR_DIFF
      && sourceLineDiff.value <= AMEND_MAX_LINE_DIFF,
  );
  const amendLocked = computed(() => {
    if (!lastRevisionUsername.value || !lockUsername.value) {
      return false;
    }
    return lastRevisionUsername.value !== lockUsername.value;
  });

  const renderedHtml = ref('');

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

  const canSave = computed(() => !interactionDisabled.value && isDirty.value);
  const restorePromptVisible = computed(() => restoreCandidates.value.length > 0);

  function resolveUploadFileName(file: File): string {
    return file.name;
  }

  function applyEditorValue(editor: { setValue: (value: string) => void }) {
    editor.setValue(source.value);
  }

  function applySourceSnapshot(markdown: string): void {
    suppressAutoAmend.value = true;
    source.value = markdown;
    initialSource.value = markdown;
    suppressAutoAmend.value = false;
    resetAmendState();
    applyAutoAmendState();
  }

  function resetAmendState(): void {
    amendUserTouched.value = false;
    amendChecked.value = true;
  }

  function setAmendChecked(value: boolean): void {
    if (amendLocked.value) {
      return;
    }
    amendUserTouched.value = true;
    amendChecked.value = value;
  }

  function shouldSuppressNewPageToast(): boolean {
    const params = new URLSearchParams(window.location.search);
    const value = params.get('create');
    return value === 'true' || value === '1';
  }

  function showNewPageToast(message: string): void {
    newPageToastMessage.value = message;
    newPageToastVisible.value = true;
  }

  function dismissNewPageToast(): void {
    newPageToastVisible.value = false;
  }

  function resolveLockTokenKey(): string | null {
    if (!pageId.value) {
      return null;
    }
    return buildLockTokenKey(pageId.value);
  }

  async function createDraftPage(showToast: boolean): Promise<void> {
    const result = await createPage(pagePath.value);
    pageId.value = result.id;
    lockToken.value = result.lockToken;
    draftCreated.value = true;
    const tokenKey = resolveLockTokenKey();
    if (tokenKey) {
      sessionStorage.setItem(tokenKey, result.lockToken);
    }
    if (showToast && !shouldSuppressNewPageToast()) {
      showNewPageToast('リンク先のページが存在しないため新規にページを作成しました。');
    }
    startLockExtend();
  }

  async function loadRestoreCandidate(pageIdValue: string): Promise<void> {
    restoreCandidateSource.value = '';
    if (!pageIdValue) {
      return;
    }
    restoreCandidateLoading.value = true;
    try {
      const markdown = await fetchPageSource(pageIdValue);
      restoreCandidateSource.value = markdown;
    } catch (err: unknown) {
      reportError(err);
    } finally {
      restoreCandidateLoading.value = false;
    }
  }

  async function selectRestoreCandidate(pageIdValue: string): Promise<void> {
    restoreCandidateId.value = pageIdValue;
    await loadRestoreCandidate(pageIdValue);
  }

  async function confirmRestoreCandidate(): Promise<void> {
    if (!restoreCandidateId.value || !pagePath.value) {
      return;
    }
    restoreInProgress.value = true;
    isLoading.value = true;
    errorMessage.value = '';
    try {
      await restorePagePath(
        restoreCandidateId.value,
        pagePath.value,
        restoreRecursive.value,
      );
      const token = await acquirePageLock(restoreCandidateId.value);
      pageId.value = restoreCandidateId.value;
      lockToken.value = token;
      draftCreated.value = false;
      const tokenKey = resolveLockTokenKey();
      if (tokenKey) {
        sessionStorage.setItem(tokenKey, token);
      }
      const [meta, markdown, pageAssets] = await Promise.all([
        fetchPageMeta(pageId.value, revision.value ?? undefined),
        fetchPageSource(pageId.value, revision.value ?? undefined),
        fetchPageAssets(pageId.value, Date.now()),
      ]);
      applyPageMeta(meta);
      applySourceSnapshot(markdown);
      assets.value = pageAssets;
      await refreshLockInfo();
      startLockExtend();
      restoreCandidates.value = [];
      restoreCandidateId.value = '';
      restoreCandidateSource.value = '';
      restoreRecursive.value = false;
    } catch (err: unknown) {
      reportError(err);
    } finally {
      restoreInProgress.value = false;
      isLoading.value = false;
    }
  }

  async function skipRestoreCandidates(): Promise<void> {
    if (!pagePath.value) {
      return;
    }
    dismissNewPageToast();
    restoreCandidates.value = [];
    restoreCandidateId.value = '';
    restoreCandidateSource.value = '';
    restoreRecursive.value = false;
    isLoading.value = true;
    errorMessage.value = '';
    try {
      await createDraftPage(false);
    } catch (err: unknown) {
      reportError(err);
    } finally {
      isLoading.value = false;
    }
  }

  async function loadPage(applyEditor?: (value: string) => void): Promise<void> {
    const rawPageId = getMetaContent('wiki-page-id');
    const rawRevision = getMetaContent('wiki-page-revision');
    const rawAssetMaxBytes = getMetaContent('asset-max-bytes');

    pagePath.value = resolveEditPath();
    revision.value = rawRevision ? Number(rawRevision) : null;
    assetMaxBytes.value = parseAssetMaxBytes(rawAssetMaxBytes);
    dismissNewPageToast();

    if (!rawPageId) {
      pageId.value = '';
      lastRevisionUsername.value = null;
      lockUsername.value = null;
      applySourceSnapshot('');
      assets.value = [];
      draftCreated.value = false;
      if (applyEditor) {
        applyEditor(source.value);
      }
      if (!pagePath.value) {
        return;
      }

      isLoading.value = true;
      errorMessage.value = '';
      try {
        const candidates = await fetchDeletedPageCandidates(pagePath.value);
        if (candidates.length > 0) {
          restoreCandidates.value = candidates;
          restoreCandidateId.value = candidates[0] ?? '';
          restoreRecursive.value = false;
          await loadRestoreCandidate(restoreCandidateId.value);
          return;
        }
        await createDraftPage(true);
      } catch (err: unknown) {
        errorMessage.value = toErrorMessage(err);
      } finally {
        isLoading.value = false;
      }
      return;
    }

    pageId.value = rawPageId;
    draftCreated.value = false;
    const tokenKey = resolveLockTokenKey();
    if (!tokenKey) {
      errorMessage.value = 'lock token not found';
      return;
    }

    const storedToken = sessionStorage.getItem(tokenKey);
    if (!storedToken) {
      errorMessage.value = 'lock token not found';
      return;
    }

    lockToken.value = storedToken;

    isLoading.value = true;
    errorMessage.value = '';

    try {
      const [meta, markdown, pageAssets] = await Promise.all([
        fetchPageMeta(pageId.value, revision.value ?? undefined),
        fetchPageSource(pageId.value, revision.value ?? undefined),
        fetchPageAssets(pageId.value, Date.now()),
      ]);

      applyPageMeta(meta);
      applySourceSnapshot(markdown);
      assets.value = pageAssets;
      if (applyEditor) {
        applyEditor(source.value);
      }
      await refreshLockInfo();
      startLockExtend();
    } catch (err: unknown) {
      errorMessage.value = toErrorMessage(err);
    } finally {
      isLoading.value = false;
    }
  }

  function applyPageMeta(meta: PageMetaResponse): void {
    pagePath.value = normalizeWikiPath(meta.page_info.path.value);
    lastRevisionUsername.value = meta.revision_info?.username ?? null;
  }

  async function refreshLockInfo(): Promise<void> {
    if (!pageId.value) {
      lockUsername.value = null;
      return;
    }
    try {
      const info = await fetchPageLockInfo(pageId.value);
      lockUsername.value = info.username;
    } catch {
      lockUsername.value = null;
    } finally {
      applyAmendLockState();
    }
  }

  function applyAmendLockState(): void {
    if (!amendLocked.value) {
      return;
    }
    amendUserTouched.value = false;
    amendChecked.value = false;
  }

  async function savePage(content?: string): Promise<void> {
    if (interactionDisabled.value || !isDirty.value) {
      return;
    }
    if (!pagePath.value) {
      reportError(new Error('page path not found'));
      return;
    }

    const editorSource = content ?? source.value;
    isSaving.value = true;
    try {
      if (isNewPage.value) {
        const result = await createPage(pagePath.value);
        pageId.value = result.id;
        lockToken.value = result.lockToken;
        const tokenKey = resolveLockTokenKey();
        if (tokenKey) {
          sessionStorage.setItem(tokenKey, result.lockToken);
        }
        await updatePageSource(pageId.value, editorSource, lockToken.value);
      } else {
        const amend = !isDraftPage.value && !amendLocked.value && amendChecked.value;
        await updatePageSource(pageId.value, editorSource, lockToken.value, amend);
        if (amend) {
          const refreshKey = buildAmendRefreshKey(pageId.value);
          sessionStorage.setItem(refreshKey, '1');
        }
      }
      clearLockToken();
      window.location.replace(wikiUrl.value);
    } catch (err: unknown) {
      reportError(err);
    } finally {
      isSaving.value = false;
    }
  }

  async function cancelEdit(): Promise<void> {
    if (interactionDisabled.value) {
      return;
    }
    if (pageId.value && lockToken.value) {
      try {
        await unlockPageLock(pageId.value, lockToken.value);
      } catch (err: unknown) {
        reportError(err);
        return;
      } finally {
        clearLockToken();
      }
    }

    if (draftCreated.value) {
      window.history.back();
      return;
    }
    window.location.replace(wikiUrl.value);
  }

  async function extendLock(): Promise<void> {
    if (!pageId.value || !lockToken.value) {
      return;
    }
    try {
      const nextToken = await extendPageLock(pageId.value, lockToken.value);
      lockToken.value = nextToken;
      const tokenKey = resolveLockTokenKey();
      if (tokenKey) {
        sessionStorage.setItem(tokenKey, nextToken);
      }
    } catch (err: unknown) {
      reportError(err);
    }
  }

  function startLockExtend(): void {
    if (lockExtendTimer.value !== null) {
      return;
    }
    if (!pageId.value || !lockToken.value) {
      return;
    }
    lockExtendTimer.value = window.setInterval(() => {
      void extendLock();
    }, LOCK_EXTEND_INTERVAL);
  }

  function stopLockExtend(): void {
    if (lockExtendTimer.value === null) {
      return;
    }
    window.clearInterval(lockExtendTimer.value);
    lockExtendTimer.value = null;
  }

  function clearLockToken(): void {
    stopLockExtend();
    const tokenKey = resolveLockTokenKey();
    if (tokenKey) {
      sessionStorage.removeItem(tokenKey);
    }
    lockToken.value = '';
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
          lockToken.value,
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

  function reportError(err: unknown): void {
    errorMessage.value = toErrorMessage(err);
  }

  function applyAutoAmendState(): void {
    if (amendLocked.value) {
      amendChecked.value = false;
      return;
    }
    if (amendUserTouched.value || suppressAutoAmend.value || isLoading.value) {
      return;
    }
    amendChecked.value = amendAutoAllowed.value;
  }

  watch(source, () => {
    applyAutoAmendState();
  });

  watch(initialSource, () => {
    applyAutoAmendState();
  });

  function setupUnloadHandler(): void {
    const handler = () => {
      if (!pageId.value || !lockToken.value) {
        return;
      }
      void unlockPageLock(pageId.value, lockToken.value);
    };
    window.addEventListener('beforeunload', handler);
    onBeforeUnmount(() => {
      window.removeEventListener('beforeunload', handler);
    });
  }

  onMounted(() => {
    setupUnloadHandler();
  });

  onBeforeUnmount(() => {
    stopLockExtend();
  });

  return {
    pageId,
    pagePath,
    wikiUrl,
    source,
    isDirty,
    renderedHtml,
    assetItems,
    isLoading,
    isSaving,
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
    newPageToastVisible,
    newPageToastMessage,
    restoreCandidates,
    restoreCandidateId,
    restoreCandidateSource,
    restoreCandidateLoading,
    restorePromptVisible,
    restoreInProgress,
    restoreRecursive,
    assetDetails,
    assetMetaDetails,
    assetDetailsLoading,
    assetDeleteTarget,
    assetDeleteLoading,
    assetInteractionDisabled,
    interactionDisabled,
    errorMessage,
    isNewPage,
    isDraftPage,
    canSave,
    amendChecked,
    amendLocked,
    applyEditorValue,
    loadPage,
    savePage,
    cancelEdit,
    uploadAssets,
    selectRestoreCandidate,
    confirmRestoreCandidate,
    skipRestoreCandidates,
    requestCopyName,
    dismissCopyToast,
    dismissNewPageToast,
    openAssetDetails,
    dismissAssetDetails,
    openAssetDeleteConfirm,
    dismissAssetDeleteConfirm,
    confirmAssetDelete,
    reportError,
    dismissError,
    setAmendChecked,
  };
}
