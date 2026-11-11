"""Live curl smoke tests: launches the real binary against only our loopback stub."""

import json
import os
import socket
import subprocess
import threading
import time
import unittest
from pathlib import Path

from mock_nas import MockNas

ROOT = Path(__file__).resolve().parent.parent


class ApiSmoke(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.nas = MockNas()
        threading.Thread(target=cls.nas.serve_forever, daemon=True).start()
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        cls.base = f'http://127.0.0.1:{port}'
        cls.process = subprocess.Popen([
            str(ROOT / 'target/debug/thunder-remoted'), 'serve', '--addr', f'127.0.0.1:{port}',
            '--base', f'http://127.0.0.1:{cls.nas.server_port}', '--web-dir', str(ROOT / 'web/dist'),
        ], stdout=subprocess.DEVNULL)
        for _attempt in range(100):
            try:
                with socket.create_connection(('127.0.0.1', port), timeout=.1):
                    return
            except OSError:
                if cls.process.poll() is not None:
                    raise RuntimeError('serve process exited during startup')
                time.sleep(.05)
        raise RuntimeError('serve did not start')

    @classmethod
    def tearDownClass(cls):
        cls.process.terminate()
        cls.process.wait(timeout=5)
        cls.nas.shutdown()
        cls.nas.server_close()

    def setUp(self):
        with self.nas.lock:
            self.nas.reset()

    def request(self, method, path, data=None, expected=200, raw=None):
        command = ['curl', '--silent', '--show-error', '--max-time', '25', '--write-out', '\n%{http_code}', '-X', method, self.base + path]
        if data is not None or raw is not None:
            command += ['-H', 'Content-Type: application/json', '--data', raw if raw is not None else json.dumps(data)]
        result = subprocess.run(command, check=True, capture_output=True, text=True)
        body, status = result.stdout.rsplit('\n', 1)
        self.assertEqual(int(status), expected, body)
        value = json.loads(body)
        if expected >= 400:
            self.assertIsInstance(value['error']['code'], str)
            self.assertIsInstance(value['error']['message'], str)
        print(f'{method} {path} -> {status} {json.dumps(value, ensure_ascii=False)}', flush=True)
        return value

    def test_every_endpoint_and_selected_files(self):
        health = self.request('GET', '/api/health')
        self.assertTrue(health['ok'] and health['connected'])
        self.assertEqual(health['nas'], f'http://127.0.0.1:{self.nas.server_port}/')
        self.assertEqual(self.request('GET', '/api/device')['name'], '书房 NAS')
        self.assertEqual(self.request('GET', '/api/quota'), {'limit': 3, 'usage': 0, 'remaining': 3})
        self.assertEqual(len(self.request('GET', '/api/tasks?limit=1')), 1)
        rows = self.request('GET', '/api/tasks?phase=PHASE_TYPE_COMPLETE&limit=1')
        self.assertEqual(rows[0]['id'], 'task-2')
        resource = self.request('POST', '/api/analyze', {'url': 'magnet:?xt=urn:btih:fixture'})
        self.assertEqual(len(resource['resources'][0]['files']), 3)
        task = self.request('POST', '/api/tasks', {'url': 'magnet:?xt=urn:btih:fixture', 'name': '选中文件', 'parent_folder_id': 'downloads', 'sub_file_index': [0, 2]})
        self.assertEqual(task['task']['id'], 'new-1')
        self.assertEqual(self.nas.created[0]['params']['sub_file_index'], '0,2')
        self.assertEqual(self.nas.created[0]['params']['file_id'], 'resource-1')
        self.assertEqual(self.nas.created[0]['params']['total_file_count'], '2')
        self.assertEqual(self.nas.created[0]['params']['parent_folder_id'], 'downloads')
        self.request('POST', '/api/tasks', {'url': 'https://example.invalid/file.zip'})
        self.assertEqual(self.nas.created[1]['params']['url'], 'https://example.invalid/file.zip')
        self.request('PATCH', '/api/tasks/task-1', {'action': 'pause'})
        self.assertEqual(self.nas.tasks[0]['phase'], 'PHASE_TYPE_PAUSED')
        self.request('PATCH', '/api/tasks/task-1', {'action': 'resume'})
        self.assertEqual(self.nas.tasks[0]['phase'], 'PHASE_TYPE_RUNNING')
        self.request('DELETE', '/api/tasks', {'ids': ['task-1', 'task-2']})
        self.assertFalse(any(task['id'].startswith('task-') for task in self.nas.tasks))

    def test_exhausted_and_unlimited_quota(self):
        self.nas.usage = 3
        result = self.request('POST', '/api/tasks', {'url': 'ftp://example.invalid/file'}, expected=409)
        self.assertIn('今日免费额度已用完（3/3）', result['error']['message'])
        self.assertEqual(self.nas.created, [])
        self.nas.limit = -1
        self.assertIsNone(self.request('GET', '/api/quota')['remaining'])
        self.request('POST', '/api/tasks', {'url': 'ed2k://|file|fixture|1|abc|/'})

    def test_reconnects_on_rotation_auth_and_network_once(self):
        self.request('GET', '/api/device')
        before = self.nas.connections
        self.nas.token = 'rotated-fixture-token'
        self.request('GET', '/api/device')
        self.assertEqual(self.nas.connections, before + 1)
        for failure in [401, 402, 'network']:
            before = self.nas.connections
            self.nas.failures = [failure]
            self.request('GET', '/api/device')
            self.assertEqual(self.nas.connections, before + 1)
        self.nas.failures = [401, 401]
        before = self.nas.connections
        self.request('GET', '/api/device', expected=502)
        self.assertEqual(self.nas.connections, before + 1)
        self.assertEqual(self.nas.failures, [])

    def test_errors_remain_json_and_unknown_quota_fails_closed(self):
        self.request('POST', '/api/analyze', raw='{', expected=400)
        self.request('POST', '/api/analyze', {'url': 'file:///etc/passwd'}, expected=400)
        self.request('GET', '/api/tasks?limit=bad', expected=400)
        self.request('GET', '/api/tasks?limit=0', expected=400)
        self.request('PATCH', '/api/tasks/task-1', {'action': 'destroy'}, expected=400)
        self.request('PATCH', '/api/tasks/missing', {'action': 'pause'}, expected=404)
        self.request('DELETE', '/api/tasks', {'ids': []}, expected=400)
        self.request('DELETE', '/api/tasks', {'ids': ['../bad']}, expected=400)
        self.request('POST', '/api/tasks', {'url': 'https://example.invalid/a', 'sub_file_index': []}, expected=400)
        self.request('GET', '/api/missing', expected=404)
        self.request('PUT', '/api/tasks', {}, expected=405)
        self.nas.failures = [500]
        self.request('POST', '/api/tasks', {'url': 'https://example.invalid/a'}, expected=502)
        self.assertEqual(self.nas.created, [])
        self.nas.failures = [503]
        self.assertFalse(self.request('GET', '/api/health')['connected'])

    def test_static_frontend_and_gzip(self):
        page = subprocess.check_output(['curl', '-fsS', self.base + '/'], text=True)
        self.assertIn('<html lang="zh-CN">', page)
        asset = next((ROOT / 'web/dist/assets').glob('*.js')).name
        headers = subprocess.check_output(['curl', '-fsS', '-D', '-', '-o', os.devnull, '-H', 'Accept-Encoding: gzip', self.base + '/assets/' + asset], text=True)
        self.assertIn('content-encoding: gzip', headers.lower())
        print('GET / -> 200 HTML; GET /assets/*.js -> 200 gzip', flush=True)

    def test_selected_files_require_resource_id(self):
        self.nas.resource_file_id = None
        result = self.request('POST', '/api/tasks', {'url': 'magnet:?xt=urn:btih:fixture', 'sub_file_index': [0]}, expected=400)
        self.assertIn('file_id', result['error']['message'])
        self.assertEqual(self.nas.created, [])

    def test_concurrent_creation_respects_last_quota_slot(self):
        self.nas.usage = 2
        results = []
        def create():
            result = subprocess.check_output(['curl', '-sS', '--max-time', '25', '-o', os.devnull, '-w', '%{http_code}', '-H', 'Content-Type: application/json', '--data', '{"url":"https://example.invalid/file"}', self.base + '/api/tasks'], text=True)
            results.append(result)
        workers = [threading.Thread(target=create) for _index in range(2)]
        for worker in workers:
            worker.start()
        for worker in workers:
            worker.join()
        self.assertEqual(sorted(results), ['200', '409'])
        self.assertEqual(len(self.nas.created), 1)
        print('Concurrent POST /api/tasks -> 200, 409; exactly one NAS submission', flush=True)


if __name__ == '__main__':
    unittest.main(verbosity=2)
