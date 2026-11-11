import { describe, expect, it } from 'vitest';
import { bytes, category, fileTree, leaves, paths, progress, resource } from './model';

describe('NAS view models', () => {
  it('keeps explicit resource indexes through nested folders', () => {
    const tree = fileTree([{ name: 'album', children: [{ name: 'a.FLAC', index: 9, size: '1024' }, { name: 'cover.png', index: 12 }] }]);
    expect(leaves(tree).map(file => file.index)).toEqual([9, 12]);
    expect(category('a.FLAC')).toBe('音频');
    expect(bytes(tree[0].size)).toBe('1.0 KB');
  });
  it('normalizes paths and resources without inventing a folder ID', () => {
    expect(paths({ download_paths: [{ id: 'folder-1', path: '/downloads/' }] })).toEqual([{ id: 'folder-1', name: '/downloads/' }]);
    expect(resource({ resources: [{ name: 'album' }] })).toEqual({ name: 'album' });
    expect(paths({})).toEqual([]);
  });
  it('accepts basis-point progress and completed tasks', () => {
    expect(progress({ id: '1', name: 'a', phase: 'PHASE_TYPE_RUNNING', progress: '4500' })).toBe(45);
    expect(progress({ id: '1', name: 'a', phase: 'PHASE_TYPE_COMPLETE', progress: null })).toBe(100);
  });
});
