import { describe, expect, it } from 'vitest';

import { configSchemas } from './config-schemas';

function minimalWidget() {
  return {
    name: 'my-widget',
    htmlPath: 'widget.html',
    zOrder: 'normal',
    shownInTaskbar: true,
    focused: false,
    resizable: true,
    transparent: false,
    caching: {
      defaultDuration: 0,
      rules: [],
    },
    privileges: {
      shellCommands: [],
    },
    presets: [],
  };
}

describe('configSchemas.length', () => {
  it.each(['10px', '-5%', '1.5px'])('accepts %s', value => {
    expect(configSchemas.length.safeParse(value).success).toBe(true);
  });

  it.each(['10em', 'abc'])('rejects %s', value => {
    expect(configSchemas.length.safeParse(value).success).toBe(false);
  });
});

describe('configSchemas.name', () => {
  it('accepts a name with lowercase letters, numbers, dashes, and underscores', () => {
    expect(configSchemas.name.safeParse('my-widget_1').success).toBe(true);
  });

  it('rejects an uppercase name', () => {
    expect(configSchemas.name.safeParse('A').success).toBe(false);
  });

  it('rejects a name under 2 characters', () => {
    expect(configSchemas.name.safeParse('a').success).toBe(false);
  });

  it('rejects a name over 28 characters', () => {
    expect(configSchemas.name.safeParse('a'.repeat(29)).success).toBe(
      false,
    );
  });

  it('rejects a name with a leading dash', () => {
    expect(configSchemas.name.safeParse('-widget').success).toBe(false);
  });
});

describe('configSchemas.version', () => {
  it('accepts a semver-style version', () => {
    expect(configSchemas.version.safeParse('1.0.0').success).toBe(true);
  });

  it('rejects a version missing the patch segment', () => {
    expect(configSchemas.version.safeParse('1.0').success).toBe(false);
  });

  it('rejects a version with a leading "v"', () => {
    expect(configSchemas.version.safeParse('v1.0.0').success).toBe(false);
  });
});

describe('configSchemas.widget', () => {
  it('parses a minimal valid widget config', () => {
    const result = configSchemas.widget.safeParse(minimalWidget());

    expect(result.success).toBe(true);
  });

  it('fails with a message mentioning htmlPath when it is missing', () => {
    const { htmlPath: _htmlPath, ...widgetWithoutHtmlPath } =
      minimalWidget();

    const result = configSchemas.widget.safeParse(widgetWithoutHtmlPath);

    expect(result.success).toBe(false);

    if (!result.success) {
      expect(
        result.error.issues.some(issue => issue.path.includes('htmlPath')),
      ).toBe(true);
    }
  });
});
