import { describe, expect, it } from 'vitest';

import { simpleHash } from './simple-hash';

describe('simpleHash', () => {
  it('returns the same hash for the same arguments', () => {
    expect(simpleHash('a', 1, { b: 2 })).toBe(
      simpleHash('a', 1, { b: 2 }),
    );
  });

  it('returns different hashes for different arguments', () => {
    expect(simpleHash('a')).not.toBe(simpleHash('b'));
  });

  it('returns a hash for no arguments', () => {
    expect(simpleHash()).toBe('[]');
  });

  it('includes undefined and function values in the hash', () => {
    const hash = simpleHash(undefined, () => 'noop');

    expect(hash).toContain('undefined');
    expect(hash).toContain('noop');
  });
});
