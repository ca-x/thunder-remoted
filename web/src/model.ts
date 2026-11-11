export type ObjectValue = Record<string, unknown>;
export const object = (value: unknown): ObjectValue => value !== null && typeof value === 'object' && !Array.isArray(value) ? value as ObjectValue : {};
export const text = (value: unknown, fallback = ''): string => typeof value === 'string' ? value : fallback;
export const number = (value: unknown): number => Number.isFinite(Number(value)) ? Number(value) : 0;
export const bytes = (value: unknown): string => {
  const size = number(value);
  if (!size) return '0 B';
  const unit = Math.min(Math.floor(Math.log(Math.max(size, 1)) / Math.log(1024)), 4);
  return `${(size / 1024 ** unit).toFixed(unit ? 1 : 0)} ${['B', 'KB', 'MB', 'GB', 'TB'][unit]}`;
};
export const categories = ['全部', '视频', '音频', '图片', '文档', '压缩包', '其他'] as const;
export type Category = typeof categories[number];
const extensions: Record<string, string[]> = {
  视频: ['mp4', 'mkv', 'avi', 'mov', 'webm', 'ts', 'm4v', 'wmv', 'flv'],
  音频: ['mp3', 'flac', 'wav', 'aac', 'ogg', 'm4a', 'opus'],
  图片: ['jpg', 'jpeg', 'png', 'gif', 'webp', 'heic', 'svg', 'avif'],
  文档: ['pdf', 'txt', 'doc', 'docx', 'epub', 'xls', 'xlsx', 'ppt', 'pptx', 'md'],
  压缩包: ['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz', 'zst'],
};
export function category(name: string): Category {
  const extension = name.split('.').pop()?.toLowerCase() ?? '';
  return (Object.entries(extensions).find(([, values]) => values.includes(extension))?.[0] as Category | undefined) ?? '其他';
}
export interface FileNode { name: string; size: number; index?: number; children: FileNode[] }
export function fileTree(value: unknown): FileNode[] {
  let nextIndex = 0;
  const visit = (input: unknown): FileNode[] => {
    if (Array.isArray(input)) return input.flatMap(visit);
    const item = object(input);
    const nested = item.children ?? item.files ?? item.file_list;
    const name = text(item.name ?? item.file_name ?? item.path);
    if (nested) {
      const children = visit(nested);
      return name ? [{ name, size: children.reduce((total, child) => total + child.size, 0), children }] : children;
    }
    if (!name) return [];
    const index = item.index ?? item.file_index ?? item.sub_file_index;
    const parsedIndex = index === undefined ? nextIndex : number(index);
    nextIndex = Math.max(nextIndex + 1, parsedIndex + 1);
    return [{ name, size: number(item.size ?? item.file_size), index: parsedIndex, children: [] }];
  };
  return visit(value);
}
export const leaves = (nodes: FileNode[]): FileNode[] => nodes.flatMap(node => node.children.length ? leaves(node.children) : [node]);
export function resource(value: unknown): ObjectValue {
  const root = object(value);
  const nested = root.resources ?? root.list ?? root.resource;
  return nested ? object(Array.isArray(nested) ? nested[0] : nested) : root;
}
export function paths(value: unknown): { id: string; name: string }[] {
  const device = object(value);
  const raw = device.download_paths ?? device.paths ?? object(device.device).download_paths;
  if (!Array.isArray(raw)) return [];
  return raw.map(item => typeof item === 'string' ? { id: item, name: item } : {
    id: text(object(item).id ?? object(item).folder_id ?? object(item).path),
    name: text(object(item).path ?? object(item).name),
  }).filter(item => item.id && item.name);
}
export interface Task extends ObjectValue { id: string; name: string; phase: string }
export const running = (task: Task): boolean => ['PHASE_TYPE_RUNNING', 'PHASE_TYPE_PENDING'].includes(task.phase);
export const phases: Record<string, string> = {
  PHASE_TYPE_RUNNING: '下载中', PHASE_TYPE_PENDING: '等待中', PHASE_TYPE_COMPLETE: '已完成', PHASE_TYPE_ERROR: '失败', PHASE_TYPE_PAUSED: '暂停',
};
export function progress(task: Task): number {
  if (task.phase === 'PHASE_TYPE_COMPLETE') return 100;
  const raw = number(task.progress);
  return Math.max(0, Math.min(100, raw > 100 ? raw / 100 : raw));
}
export async function api<T>(path: string, method = 'GET', body?: unknown): Promise<T> {
  const response = await fetch(`/api${path}`, {
    method, headers: body === undefined ? undefined : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data: unknown = await response.json();
  if (!response.ok) throw new Error(text(object(object(data).error).message, `请求失败 (${response.status})`));
  return data as T;
}
