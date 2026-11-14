import test from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, nextTick } from 'vue';
import { apiClient } from '../api/client';
import { usePageSearch } from './usePageSearch';

test('検索変更時にdebounce待ちでも旧応答/旧失敗を無効化する', async () => {
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
  const originalGet = apiClient.get;
  const timers = new Map<number, () => void>();
  const pending = new Map<string, {
    resolve: (value: unknown) => void;
    reject: (error: Error) => void;
  }>();
  let timerId = 0;
  let calls = 0;
  Object.defineProperty(globalThis, 'window', { configurable: true, value: {
    setTimeout: (callback: () => void) => {
      const id = ++timerId; timers.set(id, callback); return id;
    },
    clearTimeout: (id: number) => timers.delete(id),
  } });
  apiClient.get = ((...args: unknown[]) => {
    calls += 1;
    const config = args[1] as { params: { expr: string } };
    return new Promise<unknown>((resolve, reject) => pending.set(config.params.expr, { resolve, reject }));
  }) as typeof apiClient.get;
  const scope = effectScope();
  const state = scope.run(usePageSearch)!;
  const runTimer = () => {
    const [id, callback] = [...timers.entries()][0];
    timers.delete(id); callback();
  };
  const settle = () => new Promise<void>((resolve) => setImmediate(resolve));
  const response = (path: string) => ({ status: 200, data: [
    { page_id: path, path, revision: 1, score: 1, deleted: false, text: '' },
  ] });
  try {
    state.query.value = 'A'; await nextTick(); runTimer();
    state.query.value = 'B'; await nextTick();
    pending.get('A')!.resolve(response('/old'));
    await settle();
    assert.equal(state.results.value.length, 0);
    assert.equal(state.isLoading.value, true);
    runTimer(); pending.get('B')!.resolve(response('/docs/B')); await settle();
    assert.equal(state.results.value[0].path, '/docs/B');
    assert.equal(state.isLoading.value, false);

    state.pathFilter.value = '/docs';
    await nextTick();
    assert.equal(calls, 2);
    assert.equal(timers.size, 0);
    state.query.value = 'C'; await nextTick(); runTimer();
    state.query.value = 'D'; await nextTick();
    pending.get('C')!.reject(new Error('obsolete error'));
    await settle();
    assert.equal(state.errorMessage.value, '');
    assert.equal(state.isLoading.value, true);
    runTimer(); pending.get('D')!.reject(new Error('current error'));
    await settle();
    assert.equal(state.errorMessage.value, 'current error');
    assert.equal(state.pathFilter.value, '/docs');
    state.query.value = ''; await nextTick();
    assert.equal(state.errorMessage.value, '');
    assert.equal(state.results.value.length, 0);
    assert.equal(state.isLoading.value, false);
    assert.equal(timers.size, 0);
    state.query.value = 'E'; await nextTick();
    scope.stop();
    assert.equal(timers.size, 0);
  } finally {
    scope.stop(); apiClient.get = originalGet;
    if (previousWindow) Object.defineProperty(globalThis, 'window', previousWindow);
    else Reflect.deleteProperty(globalThis, 'window');
  }
});
