import test from 'node:test';
import assert from 'node:assert/strict';

import {
  canCreatePageFromView,
  canDeletePageFromView,
  canEditPageFromView,
  canExecuteRevisionWriteAction,
  canMovePageFromView,
  canSaveFromEdit,
  canWriteAction,
  hasReadOnlyAttribute,
  isWriteActionDisabled,
} from './readOnlyUi.ts';

test('hasReadOnlyAttribute は ReadOnly 属性を検出する', () => {
  assert.equal(hasReadOnlyAttribute(['NoBasicAuth', 'ReadOnly']), true);
  assert.equal(hasReadOnlyAttribute(['NoBasicAuth']), false);
  assert.equal(hasReadOnlyAttribute(undefined), false);
});

test('isWriteActionDisabled と canWriteAction は ReadOnly を優先する', () => {
  assert.equal(isWriteActionDisabled(true, false), true);
  assert.equal(isWriteActionDisabled(false, true), true);
  assert.equal(isWriteActionDisabled(false, false), false);

  assert.equal(canWriteAction(true, true), false);
  assert.equal(canWriteAction(false, false), false);
  assert.equal(canWriteAction(false, true), true);
});

test('閲覧画面のページ操作は ReadOnly で無効化される', () => {
  const enabledInput = {
    interactionDisabled: false,
    isDeleted: false,
    isReadOnlyUser: false,
    isRootPage: false,
    tabIdReady: true,
  };

  assert.equal(canDeletePageFromView(enabledInput), true);
  assert.equal(canMovePageFromView(enabledInput), true);
  assert.equal(canCreatePageFromView(enabledInput), true);
  assert.equal(canEditPageFromView(false, false, false, true), true);

  assert.equal(canDeletePageFromView({ ...enabledInput, isReadOnlyUser: true }), false);
  assert.equal(canMovePageFromView({ ...enabledInput, isReadOnlyUser: true }), false);
  assert.equal(canCreatePageFromView({ ...enabledInput, isReadOnlyUser: true }), false);
  assert.equal(canEditPageFromView(true, false, false, true), false);
});

test('閲覧画面のページ操作は追加条件でも無効化される', () => {
  assert.equal(canDeletePageFromView({
    interactionDisabled: true,
    isDeleted: false,
    isReadOnlyUser: false,
    isRootPage: false,
    tabIdReady: true,
  }), false);

  assert.equal(canMovePageFromView({
    interactionDisabled: false,
    isDeleted: true,
    isReadOnlyUser: false,
    isRootPage: false,
    tabIdReady: true,
  }), false);

  assert.equal(canCreatePageFromView({
    interactionDisabled: false,
    isReadOnlyUser: false,
    tabIdReady: false,
  }), false);

  assert.equal(canEditPageFromView(false, false, true, true), false);
});

test('編集画面とリビジョン画面の書き込み導線は ReadOnly で無効化される', () => {
  assert.equal(canSaveFromEdit(false, true, false), true);
  assert.equal(canSaveFromEdit(true, true, false), false);
  assert.equal(canSaveFromEdit(false, true, true), false);

  assert.equal(canExecuteRevisionWriteAction(false, true), true);
  assert.equal(canExecuteRevisionWriteAction(true, true), false);
  assert.equal(canExecuteRevisionWriteAction(false, false), false);
});
