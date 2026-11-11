"""Loopback-only NAS fixture. No real devices, cloud login, or external requests."""

import argparse
import json
import socket
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

PREFIX = '/webman/3rdparty/pan-xunlei-com/index.cgi/'


class MockNas(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, port=0):
        super().__init__(('127.0.0.1', port), Handler)
        self.lock = threading.Lock()
        self.reset()

    def reset(self):
        self.token = 'fixture-session'
        self.connections = 0
        self.usage = 0
        self.limit = 3
        self.failures = []
        self.requests = []
        self.created = []
        self.resource_file_id = 'resource-1'
        self.tasks = [
            {'id': 'task-1', 'name': '海岸.mkv', 'phase': 'PHASE_TYPE_RUNNING',
             'space': 'fixture-device-space', 'file_size': '104857600', 'progress': '4200',
             'created_time': '2026-11-11T08:00:00Z',
             'params': {'files': [{'name': '海岸.mkv'}, {'name': 'notes.pdf'}]}},
            {'id': 'task-2', 'name': '手册.pdf', 'phase': 'PHASE_TYPE_COMPLETE',
             'space': '', 'file_size': '2048', 'progress': '10000'},
        ]


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def reply(self, status, value, content_type='application/json'):
        data = (json.dumps(value, ensure_ascii=False) if content_type == 'application/json' else value).encode()
        self.send_response(status)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def handle_request(self):
        with self.server.lock:
            path = urlsplit(self.path)
            if path.path == '/':
                self.server.connections += 1
                self.reply(200, f'<script>function uiauth(value){{ return "{self.server.token}" }}</script>', 'text/html')
                return
            if not path.path.startswith(PREFIX):
                self.reply(404, {'error': 'unknown fixture route'})
                return
            route = path.path.removeprefix(PREFIX)
            length = int(self.headers.get('Content-Length', '0'))
            body = json.loads(self.rfile.read(length)) if length else None
            self.server.requests.append((self.command, route, body))
            if self.headers.get('pan-auth') != self.server.token:
                self.reply(401, {'message': 'fixture token rotated'})
                return
            if self.server.failures:
                failure = self.server.failures.pop(0)
                if failure == 'network':
                    self.connection.shutdown(socket.SHUT_RDWR)
                    self.connection.close()
                else:
                    self.reply(failure, {'message': 'fixture failure'})
                return
            if self.command == 'GET' and route == 'device/info':
                self.reply(200, {'name': '书房 NAS', 'disk_usage': {'usage': 1099511627776, 'limit': 4398046511104},
                                 'download_paths': [{'id': 'downloads', 'path': '/downloads/'}, {'id': 'data', 'path': '/data/'}]})
            elif self.command == 'GET' and route == 'drive/v1/about':
                self.reply(200, {'quotas': {'CREATE_USER_DOWNLOAD_TASK_LIMIT_INTERNAL': {'limit': self.server.limit, 'usage': self.server.usage}}})
            elif self.command == 'GET' and route == 'drive/v1/tasks':
                self.reply(200, {'tasks': self.server.tasks})
            elif self.command == 'POST' and route == 'drive/v1/resource/list':
                self.reply(200, {'resources': [{'file_id': self.server.resource_file_id, 'name': '海岸合集', 'size': '104863744',
                    'infohash': '0123456789abcdef0123456789abcdef01234567', 'files': [
                        {'name': '影片', 'children': [{'name': '海岸.mkv', 'index': 0, 'size': '104857600'}, {'name': '预告.mp4', 'index': 3, 'size': '2048'}]},
                        {'name': '手册.pdf', 'index': 1, 'size': '2048'},
                        {'name': '封面.png', 'index': 2, 'size': '2048'}]}]})
            elif self.command == 'POST' and route == 'drive/v1/task':
                if self.server.limit >= 0 and self.server.usage >= self.server.limit:
                    self.reply(402, {'message': 'daily quota exhausted'})
                    return
                self.server.created.append(body)
                self.server.usage += 1
                task = {'id': f'new-{self.server.usage}', 'name': body.get('name', '新下载'),
                        'phase': 'PHASE_TYPE_PENDING', 'space': body.get('space', ''), 'params': body['params']}
                self.server.tasks.append(task)
                self.reply(200, {'task': task})
            elif self.command == 'PATCH' and route.startswith('drive/v1/task/'):
                task_id = route.rsplit('/', 1)[-1]
                task = next(task for task in self.server.tasks if task['id'] == task_id)
                if body['space'] != task['space']:
                    self.reply(400, {'message': 'wrong task space'})
                    return
                task['phase'] = json.loads(body['set_params']['spec'])['phase']
                self.reply(200, {})
            elif self.command == 'DELETE' and route == 'drive/v1/tasks':
                ids = parse_qs(path.query).get('task_ids', [])
                self.server.tasks = [task for task in self.server.tasks if task['id'] not in ids]
                self.reply(200, {})
            else:
                self.reply(404, {'error': f'unsupported fixture route: {self.command} {route}'})

    do_GET = handle_request
    do_POST = handle_request
    do_PATCH = handle_request
    do_DELETE = handle_request


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', type=int, default=42345)
    args = parser.parse_args()
    server = MockNas(args.port)
    print(f'Mock NAS listening on http://127.0.0.1:{server.server_port}', flush=True)
    server.serve_forever()
