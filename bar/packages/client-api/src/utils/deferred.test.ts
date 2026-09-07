import { describe, expect, it } from 'vitest';

import { Deferred } from './deferred';

describe('Deferred', () => {
  it('resolves the promise with the value passed to resolve', async () => {
    const deferred = new Deferred<number>();

    deferred.resolve(42);

    await expect(deferred.promise).resolves.toBe(42);
  });

  it('rejects the promise with the error passed to reject', async () => {
    const deferred = new Deferred<number>();
    const error = new Error('failed');

    deferred.reject(error);

    await expect(deferred.promise).rejects.toBe(error);
  });
});
