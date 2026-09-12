import { describe, expect, it } from 'vitest';

import { getCoordinateDistance } from './get-coordinate-distance';

describe('getCoordinateDistance', () => {
  it('returns zero for two identical points', () => {
    const distance = getCoordinateDistance({ x: 5, y: 5 }, { x: 5, y: 5 });

    expect(distance).toBe(0);
  });

  it('returns the straight-line distance between two points', () => {
    const distance = getCoordinateDistance({ x: 0, y: 0 }, { x: 3, y: 4 });

    expect(distance).toBe(5);
  });

  it('returns a positive distance when coordinates are negative', () => {
    const distance = getCoordinateDistance(
      { x: -3, y: -4 },
      { x: 0, y: 0 },
    );

    expect(distance).toBe(5);
  });
});
