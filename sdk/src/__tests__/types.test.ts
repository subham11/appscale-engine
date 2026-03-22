/**
 * Tests for AppScale shared types (types.ts)
 *
 * Verifies type shapes, defaults, and serialization contracts
 * match the Rust IR layer definitions.
 */

import type { IrCommand, IrBatch, NativeEngine } from '../types';

describe('IrCommand type shape', () => {
  test('accepts create command', () => {
    const cmd: IrCommand = {
      type: 'create',
      id: 1,
      view_type: 'Text',
      props: { text: 'Hello' },
    };
    expect(cmd.type).toBe('create');
    expect(cmd.id).toBe(1);
  });

  test('accepts update_props command', () => {
    const cmd: IrCommand = {
      type: 'update_props',
      id: 1,
      diff: { text: 'Updated' },
    };
    expect(cmd.type).toBe('update_props');
  });

  test('accepts remove_child command', () => {
    const cmd: IrCommand = {
      type: 'remove_child',
      parent: 1,
      child: 2,
    };
    expect(cmd.type).toBe('remove_child');
  });

  test('accepts set_root command', () => {
    const cmd: IrCommand = { type: 'set_root', id: 0 };
    expect(cmd.type).toBe('set_root');
  });
});

describe('IrBatch type shape', () => {
  test('contains required fields', () => {
    const batch: IrBatch = {
      commit_id: 42,
      timestamp_ms: 1234.5,
      commands: [],
    };
    expect(batch.commit_id).toBe(42);
    expect(batch.timestamp_ms).toBe(1234.5);
    expect(batch.commands).toHaveLength(0);
  });

  test('serializes to JSON matching Rust struct', () => {
    const batch: IrBatch = {
      commit_id: 1,
      timestamp_ms: 100,
      commands: [
        { type: 'create', id: 1, view_type: 'Container' },
        { type: 'set_root', id: 1 },
      ],
    };
    const json = JSON.stringify(batch);
    const restored = JSON.parse(json) as IrBatch;
    expect(restored.commit_id).toBe(1);
    expect(restored.commands).toHaveLength(2);
    expect(restored.commands[0].type).toBe('create');
  });
});

describe('NativeEngine interface', () => {
  test('mock engine satisfies interface', () => {
    const engine: NativeEngine = {
      applyCommit: jest.fn(),
      sync: jest.fn(() => ({})),
    };
    engine.applyCommit({ commit_id: 1, timestamp_ms: 0, commands: [] });
    expect(engine.applyCommit).toHaveBeenCalledTimes(1);
    engine.sync('is_processing');
    expect(engine.sync).toHaveBeenCalledWith('is_processing');
  });
});
