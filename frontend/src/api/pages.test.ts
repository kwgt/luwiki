import test from 'node:test';
import assert from 'node:assert/strict';

import { apiClient } from './client';
import { searchPages } from './pages';

test('searchPages は front_matter を含む対象指定を REST API 形式で送る', async () => {
  const originalGet = apiClient.get;
  const captured: Array<unknown> = [];

  apiClient.get = (async (...args: unknown[]) => {
    captured.push(...args);
    return {
      status: 200,
      data: [],
    };
  }) as typeof apiClient.get;

  try {
    await searchPages({
      expression: 'token',
      targets: ['headings', 'body', 'code', 'front_matter'],
      withDeleted: true,
      allRevision: false,
    });
  } finally {
    apiClient.get = originalGet;
  }

  assert.equal(captured[0], '/pages/search');
  const requestOptions = captured[1] as {
    params: Record<string, unknown>;
    validateStatus?: unknown;
  };
  assert.deepEqual(requestOptions.params, {
    expr: 'token',
    target: 'headings,body,code,front_matter',
    with_deleted: true,
    all_revision: false,
  });
  assert.equal(typeof requestOptions.validateStatus, 'function');
});

test('searchPages は対象配列が空でも body を既定で送る', async () => {
  const originalGet = apiClient.get;
  const captured: Array<unknown> = [];

  apiClient.get = (async (...args: unknown[]) => {
    captured.push(...args);
    return {
      status: 200,
      data: [],
    };
  }) as typeof apiClient.get;

  try {
    await searchPages({
      expression: 'token',
      targets: [],
      withDeleted: false,
      allRevision: true,
    });
  } finally {
    apiClient.get = originalGet;
  }

  assert.equal(captured[0], '/pages/search');
  const requestOptions = captured[1] as {
    params: Record<string, unknown>;
    validateStatus?: unknown;
  };
  assert.deepEqual(requestOptions.params, {
    expr: 'token',
    target: 'body',
    with_deleted: false,
    all_revision: true,
  });
  assert.equal(typeof requestOptions.validateStatus, 'function');
});
