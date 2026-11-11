import { StrictMode, useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { createRoot } from 'react-dom/client';
import { api, bytes, categories, category, fileTree, leaves, number, object, paths, phases, progress, resource, running, text, type Category, type FileNode, type ObjectValue, type Task } from './model';
import './style.css';

interface Health { ok: boolean; nas: string; connected: boolean }
interface Quota { limit: number; usage: number; remaining: number | null }
const filters = [['', '全部'], ['PHASE_TYPE_RUNNING', '下载中'], ['PHASE_TYPE_COMPLETE', '已完成'], ['PHASE_TYPE_ERROR', '失败'], ['PHASE_TYPE_PAUSED', '暂停']];
const message = (error: unknown): string => error instanceof Error ? error.message : '请求失败，请重试';

function CategoryChips({ value, onChange }: { value: Category; onChange: (value: Category) => void }) {
  return <div className="chips categories" aria-label="文件类型">{categories.map(item => <button type="button" key={item} aria-pressed={item === value} onClick={() => onChange(item)}>{item}</button>)}</div>;
}

function TreeRow({ node, selected, setSelected, filter }: { node: FileNode; selected: Set<number>; setSelected: (value: Set<number>) => void; filter: Category }) {
  const checkbox = useRef<HTMLInputElement>(null);
  const files = leaves([node]);
  const visible = files.filter(file => filter === '全部' || category(file.name) === filter);
  const checked = files.filter(file => file.index !== undefined && selected.has(file.index)).length;
  useEffect(() => { if (checkbox.current) checkbox.current.indeterminate = checked > 0 && checked < files.length; }, [checked, files.length]);
  if (!visible.length) return null;
  return <li><label className="file-row"><input ref={checkbox} type="checkbox" checked={checked === files.length} onChange={() => {
    const next = new Set(selected);
    for (const file of files) if (file.index !== undefined) { if (checked === files.length) next.delete(file.index); else next.add(file.index); }
    setSelected(next);
  }} /><span>{node.children.length ? '▸ ' : ''}{node.name}</span><small>{bytes(node.size)}</small></label>
    {node.children.length > 0 && <ul>{node.children.map((child, index) => <TreeRow key={`${child.name}-${index}`} node={child} selected={selected} setSelected={setSelected} filter={filter} />)}</ul>}
  </li>;
}

function AddTask({ device, close, created, notify }: { device: ObjectValue; close: () => void; created: () => Promise<void>; notify: (value: string) => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [url, setUrl] = useState('');
  const [analyzedUrl, setAnalyzedUrl] = useState('');
  const [parsed, setParsed] = useState<ObjectValue | null>(null);
  const [name, setName] = useState('');
  const [tree, setTree] = useState<FileNode[]>([]);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [filter, setFilter] = useState<Category>('全部');
  const presets = paths(device);
  const [folder, setFolder] = useState(presets[0]?.id ?? '');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const files = leaves(tree);
  useEffect(() => { const modal = dialog.current; modal?.showModal(); modal?.querySelector<HTMLInputElement>('#link')?.focus(); return () => modal?.close(); }, []);
  async function analyze(event: FormEvent) {
    event.preventDefault(); setBusy(true); setError(''); setParsed(null);
    try {
      const result = resource(await api<unknown>('/analyze', 'POST', { url: url.trim() }));
      const nodes = fileTree(result.files ?? result.children ?? result.file_list ?? []);
      setParsed(result); setAnalyzedUrl(url.trim()); setName(text(result.name ?? result.file_name)); setTree(nodes);
      setSelected(new Set(leaves(nodes).flatMap(file => file.index === undefined ? [] : [file.index])));
    } catch (error) { setError(message(error)); } finally { setBusy(false); }
  }
  async function submit() {
    setBusy(true); setError('');
    try {
      const response = await api<unknown>('/tasks', 'POST', {
        url: analyzedUrl, name: name || undefined, parent_folder_id: folder || undefined,
        sub_file_index: files.length > 1 ? [...selected].sort((left, right) => left - right) : undefined,
      });
      const task = object(object(response).task ?? response);
      notify(`任务已创建${text(task.id) ? ` · ${text(task.id)}` : ''}`);
      close(); await created();
    } catch (error) { setError(message(error)); } finally { setBusy(false); }
  }
  return <dialog ref={dialog} onCancel={close} aria-labelledby="add-title" className="add-dialog">
    <div className="dialog-head"><div><h2 id="add-title">新建下载</h2><p>把链接交给 NAS，剩下的慢慢来。</p></div><button type="button" className="icon-button" aria-label="关闭" onClick={close}>×</button></div>
    <form onSubmit={analyze}><label htmlFor="link">下载链接</label><div className="link-input"><input id="link" autoFocus required disabled={busy} placeholder="magnet / https / ftp / ed2k" value={url} onChange={event => { setUrl(event.target.value); setParsed(null); }} /><button className="primary" disabled={busy || !url.trim()}>{busy ? '请稍候…' : '解析'}</button></div></form>
    {error && <p role="alert" className="error">{error}</p>}
    {parsed && <section className="parsed">
      <label htmlFor="task-name">任务名称</label><input id="task-name" value={name} onChange={event => setName(event.target.value)} />
      <p className="resource-meta">{bytes(parsed.size ?? parsed.file_size ?? files.reduce((sum, file) => sum + file.size, 0))} / {files.length || number(parsed.file_count) || 1} 个文件</p>
      {(parsed.infohash ?? parsed.info_hash) !== undefined && <p className="hash">Infohash: {text(parsed.infohash ?? parsed.info_hash)}</p>}
      {files.length > 1 && <><CategoryChips value={filter} onChange={setFilter} /><div className="selection-tools"><span>已选 {selected.size}/{files.length}</span><button type="button" onClick={() => setSelected(new Set(files.flatMap(file => file.index === undefined ? [] : [file.index])))}>全选</button><button type="button" onClick={() => setSelected(new Set(files.flatMap(file => file.index !== undefined && !selected.has(file.index) ? [file.index] : [])))}>反选</button></div><ul className="file-tree">{tree.map((node, index) => <TreeRow key={`${node.name}-${index}`} node={node} selected={selected} setSelected={setSelected} filter={filter} />)}</ul></>}
      <label htmlFor="folder">下载目录</label>{presets.length ? <select id="folder" value={folder} onChange={event => setFolder(event.target.value)}>{!presets.some(preset => preset.id === folder) && <option value={folder}>{folder || '默认目录'}</option>}{presets.map(preset => <option key={preset.id} value={preset.id}>{preset.name}</option>)}</select> : <p className="muted">NAS 默认下载目录</p>}
      <details><summary>高级</summary><label htmlFor="folder-id">文件夹 ID</label><input id="folder-id" value={folder} onChange={event => setFolder(event.target.value)} placeholder="留空使用默认目录" /></details>
      <footer className="dialog-footer"><button type="button" onClick={close}>取消</button><button type="button" className="primary" disabled={busy || (files.length > 1 && !selected.size)} onClick={() => void submit()}>{busy ? '正在提交…' : '开始下载'}</button></footer>
    </section>}
  </dialog>;
}

function App() {
  const [health, setHealth] = useState<Health | null>(null);
  const [device, setDevice] = useState<ObjectValue>({});
  const [quota, setQuota] = useState<Quota | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [phase, setPhase] = useState('');
  const [filter, setFilter] = useState<Category>('全部');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  const [add, setAdd] = useState(false);
  const [toast, setToast] = useState('');
  const [deleting, setDeleting] = useState<Task | null>(null);
  const [pending, setPending] = useState<Set<string>>(new Set());
  const pendingRef = useRef(new Set<string>());
  const confirmation = useRef<HTMLDialogElement>(null);
  const refreshing = useRef(false);
  const refreshQueued = useRef(false);
  const refresh = useCallback(async () => {
    if (refreshing.current) { refreshQueued.current = true; return; }
    refreshing.current = true;
    try {
      const status = await api<Health>('/health'); setHealth(status);
      if (!status.connected) throw new Error('无法连接 NAS，请检查迅雷客户端是否运行。');
      const [info, allowance, rows] = await Promise.all([api<ObjectValue>('/device'), api<Quota>('/quota'), api<Task[]>('/tasks')]);
      setDevice(info); setQuota(allowance); setTasks(previous => {
        const optimistic = previous.filter(task => pendingRef.current.has(task.id));
        return [...rows.filter(task => !pendingRef.current.has(task.id)), ...optimistic];
      }); setError('');
    } catch (error) { setError(message(error)); } finally {
      setLoading(false); refreshing.current = false;
      if (refreshQueued.current) { refreshQueued.current = false; void refresh(); }
    }
  }, []);
  useEffect(() => { void refresh(); }, [refresh]);
  useEffect(() => {
    let timer: ReturnType<typeof setInterval> | undefined;
    const schedule = () => {
      if (timer) clearInterval(timer);
      if (!document.hidden && tasks.some(running)) timer = setInterval(() => { void refresh(); }, 3000);
    };
    const visibility = () => { schedule(); if (!document.hidden) void refresh(); };
    schedule(); document.addEventListener('visibilitychange', visibility);
    return () => { if (timer) clearInterval(timer); document.removeEventListener('visibilitychange', visibility); };
  }, [tasks, refresh]);
  useEffect(() => { if (!toast) return; const timer = setTimeout(() => setToast(''), 6000); return () => clearTimeout(timer); }, [toast]);
  useEffect(() => { if (deleting) confirmation.current?.showModal(); }, [deleting]);
  async function action(task: Task, operation: 'pause' | 'resume' | 'delete') {
    pendingRef.current.add(task.id); setPending(new Set(pendingRef.current));
    setTasks(rows => operation === 'delete' ? rows.filter(row => row.id !== task.id) : rows.map(row => row.id === task.id ? { ...row, phase: operation === 'pause' ? 'PHASE_TYPE_PAUSED' : 'PHASE_TYPE_RUNNING' } : row));
    try {
      await api(operation === 'delete' ? '/tasks' : `/tasks/${encodeURIComponent(task.id)}`, operation === 'delete' ? 'DELETE' : 'PATCH', operation === 'delete' ? { ids: [task.id] } : { action: operation });
      setToast(operation === 'delete' ? '任务已删除' : operation === 'pause' ? '任务已暂停' : '任务已继续');
    } catch (error) { setTasks(rows => [...rows.filter(row => row.id !== task.id), task]); setToast(message(error)); }
    finally { pendingRef.current.delete(task.id); setPending(new Set(pendingRef.current)); await refresh(); }
  }
  const shown = tasks.filter(task => (!phase || task.phase === phase || (phase === 'PHASE_TYPE_RUNNING' && running(task))) && (filter === '全部' || [task.name, ...leaves(fileTree(object(task.params).files ?? task.files ?? [])).map(file => file.name)].some(name => category(name) === filter)));
  const disk = object(device.disk_usage ?? device.disk ?? object(device.device).disk_usage);
  const used = number(disk.usage ?? disk.used ?? device.usage ?? device.disk_usage);
  const limit = number(disk.limit ?? disk.total ?? device.limit ?? device.disk_limit);
  const deviceName = text(device.name ?? device.device_name ?? object(device.device).name, '我的 NAS');
  return <div className="shell">
    <header className="topbar"><a className="brand" href="/" aria-label="迅雷下载管理首页"><span className="brand-mark" aria-hidden="true">↧</span><span>迅雷<span className="brand-sub">下载管理</span></span></a><span className={`status ${health?.connected && !error ? 'online' : ''}`}><span aria-hidden="true">●</span> {health?.connected && !error ? 'NAS 在线' : 'NAS 未连接'}</span></header>
    <main><section className="device-strip" aria-label="设备信息"><div><span className="muted">下载到</span><h1>{deviceName}</h1></div><div className="disk"><div><span>存储空间</span><span>{limit ? `${bytes(used)} / ${bytes(limit)}` : '暂未提供'}</span></div><progress aria-label="磁盘使用率" value={used} max={limit || 1} /></div><div className={`quota ${quota?.remaining === 0 ? 'warning' : ''}`}><span>今日额度</span><strong>{quota ? quota.remaining === null ? '不限次数' : `${quota.usage}/${quota.limit}` : '—'}</strong><small>{quota?.remaining === 0 ? '免费额度已用完' : quota?.remaining != null ? `还可创建 ${quota.remaining} 个任务` : '由 NAS 客户端提供'}</small></div></section>
      <section className="downloads"><div className="section-heading"><div><h2>下载任务 <span>{tasks.length}</span></h2><p>文件在路上，一切井然有序。</p></div><button className="primary" onClick={() => setAdd(true)}>＋ 新建下载</button></div>
        <div className="filters"><div className="chips phases" aria-label="任务状态">{filters.map(([value, label]) => <button key={value} aria-pressed={phase === value} onClick={() => setPhase(value)}>{label}</button>)}</div><button className="refresh" onClick={() => void refresh()}>刷新</button></div>
        <CategoryChips value={filter} onChange={setFilter} />
        {error && <div className="error-state" role="alert"><h3>暂时无法读取 NAS</h3><p>{error}</p><code>{health?.nas ?? '请检查 THUNDER_NAS_URL 配置'}</code><button onClick={() => void refresh()}>重试连接</button></div>}
        {!error && loading && <p className="empty" role="status">正在连接 NAS…</p>}
        {!error && !loading && !shown.length && <div className="empty"><span className="empty-symbol" aria-hidden="true">↧</span><h3>{tasks.length ? '没有符合筛选的任务' : '让 NAS 开始忙碌吧'}</h3><p>{tasks.length ? '试试其他状态或文件类型。' : '添加一个下载链接，文件会直接保存到 NAS。'}</p>{!tasks.length && <button onClick={() => setAdd(true)}>添加第一个下载</button>}</div>}
        {!error && shown.length > 0 && <><div className="list-heading"><span>名称 / 进度</span><span>大小</span><span>创建时间</span><span>操作</span></div><ul className="task-list">{shown.map(task => <li className="task-row" key={task.id}><div className="task-info"><div className="task-name"><span className="file-icon" aria-hidden="true">{category(task.name) === '视频' ? '▷' : '↓'}</span><strong title={task.name}>{task.name || '未命名任务'}</strong><span className={`badge ${task.phase === 'PHASE_TYPE_ERROR' ? 'warning' : ''}`}>{phases[task.phase] ?? task.phase}</span></div><div className="task-progress"><progress aria-label={`${task.name} 下载进度`} max={100} value={progress(task)} /><small>{progress(task).toFixed(0)}%</small></div></div><span className="task-size">{bytes(task.file_size)}</span><time dateTime={text(task.created_time ?? task.created_at)}>{(() => { const date = new Date(text(task.created_time ?? task.created_at)); return Number.isNaN(date.getTime()) ? '—' : date.toLocaleString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' }); })()}</time><div className="row-actions">{(running(task) || task.phase === 'PHASE_TYPE_PAUSED') && <button disabled={pending.has(task.id)} onClick={() => void action(task, task.phase === 'PHASE_TYPE_PAUSED' ? 'resume' : 'pause')}>{task.phase === 'PHASE_TYPE_PAUSED' ? '继续' : '暂停'}</button>}<button disabled={pending.has(task.id)} onClick={() => setDeleting(task)} aria-label={`删除 ${task.name}`}>删除</button></div></li>)}</ul></>}
      </section><p className="footnote">文件直达 NAS · 无需云端登录 · 尊重客户端每日额度</p></main>
    <div className={`toast ${toast ? 'visible' : ''}`} role="status" aria-live="polite">{toast}</div>
    {add && <AddTask device={device} close={() => setAdd(false)} created={refresh} notify={setToast} />}
    {deleting && <dialog ref={confirmation} onCancel={() => setDeleting(null)} aria-labelledby="delete-title"><h2 id="delete-title">删除这个任务？</h2><p className="delete-name">{deleting.name}</p><p className="muted">此操作不可撤销。文件的保留方式由 NAS 客户端决定。</p><footer className="dialog-footer"><button autoFocus onClick={() => setDeleting(null)}>取消</button><button className="danger" onClick={() => { void action(deleting, 'delete'); setDeleting(null); }}>确认删除</button></footer></dialog>}
  </div>;
}

createRoot(document.getElementById('root')!).render(<StrictMode><App /></StrictMode>);
