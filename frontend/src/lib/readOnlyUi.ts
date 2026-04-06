/**
 * 現在ユーザ属性に ReadOnly が含まれるか判定する。
 *
 * @param attributes ユーザ属性配列
 * @returns ReadOnly 属性を持つ場合は true
 */
export function hasReadOnlyAttribute(
  attributes: readonly string[] | null | undefined,
): boolean {
  return attributes?.includes('ReadOnly') ?? false;
}

/**
 * ReadOnly を含む共通の無効化判定を行う。
 *
 * @param isReadOnlyUser 現在ユーザが ReadOnly か
 * @param disabled 追加の無効化条件
 * @returns 操作を無効化する場合は true
 */
export function isWriteActionDisabled(
  isReadOnlyUser: boolean,
  disabled = false,
): boolean {
  return isReadOnlyUser || disabled;
}

/**
 * ReadOnly を含む共通の実行可否判定を行う。
 *
 * @param isReadOnlyUser 現在ユーザが ReadOnly か
 * @param enabled 追加条件が満たされているか
 * @returns 操作を実行できる場合は true
 */
export function canWriteAction(
  isReadOnlyUser: boolean,
  enabled = true,
): boolean {
  return !isWriteActionDisabled(isReadOnlyUser, !enabled);
}

export interface ViewPageActionAvailabilityInput {
  interactionDisabled: boolean;
  isDeleted: boolean;
  isRootPage: boolean;
  isReadOnlyUser: boolean;
  tabIdReady: boolean;
}

/**
 * 閲覧画面のページ削除可否を判定する。
 */
export function canDeletePageFromView(
  input: ViewPageActionAvailabilityInput,
): boolean {
  return canWriteAction(
    input.isReadOnlyUser,
    !input.interactionDisabled && !input.isDeleted && !input.isRootPage,
  );
}

/**
 * 閲覧画面のページ移動可否を判定する。
 */
export function canMovePageFromView(
  input: ViewPageActionAvailabilityInput,
): boolean {
  return canDeletePageFromView(input);
}

/**
 * 閲覧画面のページ新規作成可否を判定する。
 */
export function canCreatePageFromView(
  input: Pick<ViewPageActionAvailabilityInput, 'interactionDisabled' | 'isReadOnlyUser' | 'tabIdReady'>,
): boolean {
  return canWriteAction(
    input.isReadOnlyUser,
    !input.interactionDisabled && input.tabIdReady,
  );
}

/**
 * 閲覧画面の編集導線可否を判定する。
 */
export function canEditPageFromView(
  isReadOnlyUser: boolean,
  interactionDisabled: boolean,
  isLocking: boolean,
  tabIdReady: boolean,
): boolean {
  return canWriteAction(
    isReadOnlyUser,
    !interactionDisabled && !isLocking && tabIdReady,
  );
}

/**
 * 編集画面の保存可否を判定する。
 */
export function canSaveFromEdit(
  isReadOnlyUser: boolean,
  canSave: boolean,
  isSaving: boolean,
): boolean {
  return canWriteAction(isReadOnlyUser, canSave && !isSaving);
}

/**
 * リビジョン画面の書き込み導線可否を判定する。
 */
export function canExecuteRevisionWriteAction(
  isReadOnlyUser: boolean,
  enabled: boolean,
): boolean {
  return canWriteAction(isReadOnlyUser, enabled);
}
