"""Optional real Chromium UI smoke tests (requires chromium and chromedriver)."""

import base64
import json
import socket
import subprocess
import tempfile
import time
import unittest
from pathlib import Path
from urllib.request import Request, urlopen

import smoke


class BrowserSmoke(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        smoke.ApiSmoke.setUpClass()
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
        cls.driver_url = f'http://127.0.0.1:{port}'
        cls.driver = subprocess.Popen(['chromedriver', f'--port={port}'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _attempt in range(100):
            try:
                cls.command('GET', '/status')
                break
            except OSError:
                time.sleep(.05)
        session = cls.command('POST', '/session', {'capabilities': {'alwaysMatch': {
            'browserName': 'chrome', 'goog:chromeOptions': {'binary': '/usr/bin/chromium',
            'args': ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage', '--window-size=1280,900']},
        }}})
        cls.session = session['sessionId']
        cls.artifacts = Path(tempfile.mkdtemp(prefix='thunder-browser-'))
        print(f'Browser screenshots: {cls.artifacts}', flush=True)

    @classmethod
    def tearDownClass(cls):
        cls.command('DELETE', f'/session/{cls.session}')
        cls.driver.terminate()
        cls.driver.wait(timeout=5)
        smoke.ApiSmoke.tearDownClass()

    @classmethod
    def command(cls, method, path, value=None):
        request = Request(cls.driver_url + path, data=None if value is None else json.dumps(value).encode(),
                          headers={'Content-Type': 'application/json'}, method=method)
        with urlopen(request, timeout=20) as response:
            return json.load(response)['value']

    def script(self, script):
        return self.command('POST', f'/session/{self.session}/execute/sync', {'script': script, 'args': []})

    def wait(self, script):
        for _attempt in range(100):
            if self.script(script):
                return
            time.sleep(.1)
        self.screenshot('failure.png')
        print(self.script('return {active: document.activeElement.outerHTML, text: document.body.innerText}'), flush=True)
        self.fail(f'Browser condition not met: {script}')

    def click(self, selector):
        element = self.command('POST', f'/session/{self.session}/element', {'using': 'css selector', 'value': selector})
        element_id = element['element-6066-11e4-a52e-4f735466cecf']
        self.command('POST', f'/session/{self.session}/element/{element_id}/click', {})

    def type(self, selector, value):
        element = self.command('POST', f'/session/{self.session}/element', {'using': 'css selector', 'value': selector})
        element_id = element['element-6066-11e4-a52e-4f735466cecf']
        self.command('POST', f'/session/{self.session}/element/{element_id}/value', {'text': value})

    def cdp(self, command, params):
        return self.command('POST', f'/session/{self.session}/goog/cdp/execute', {'cmd': command, 'params': params})

    def screenshot(self, name):
        data = self.command('GET', f'/session/{self.session}/screenshot')
        (self.artifacts / name).write_bytes(base64.b64decode(data))

    def setUp(self):
        with smoke.ApiSmoke.nas.lock:
            smoke.ApiSmoke.nas.reset()
        self.command('POST', f'/session/{self.session}/url', {'url': smoke.ApiSmoke.base})
        self.wait('return document.querySelectorAll(".task-row").length === 2')

    def test_task_actions_file_picker_mobile_dark_and_keyboard(self):
        self.screenshot('desktop.png')
        self.click('.row-actions button')
        self.wait('return document.querySelector(".badge").textContent === "暂停"')
        self.click('.row-actions button')
        self.wait('return document.querySelector(".badge").textContent === "下载中"')
        self.click('.section-heading .primary')
        self.wait('return document.activeElement.id === "link"')
        self.type('#link', 'magnet:?xt=urn:btih:fixture')
        self.click('.link-input button')
        self.wait('return document.querySelectorAll(".file-tree input").length === 5')
        self.click('.file-tree ul input')
        self.assertTrue(self.script('return document.querySelector(".file-tree > li input").indeterminate'))
        self.click('.parsed .categories button:nth-child(2)')
        self.assertEqual(self.script('return document.querySelectorAll(".file-tree > li").length'), 1)
        self.click('.parsed .categories button:first-child')
        self.screenshot('file-picker.png')
        self.click('.dialog-footer .primary')
        self.wait('return !document.querySelector(".add-dialog") && document.querySelector(".toast").textContent.includes("new-1")')
        self.assertEqual(smoke.ApiSmoke.nas.created[0]['params']['sub_file_index'], '1,2,3')
        self.wait('return document.querySelectorAll(".task-row").length === 3')
        self.click('.row-actions button:last-child')
        self.wait('return document.querySelector("dialog[open]") !== null')
        self.assertEqual(self.script('return document.activeElement.textContent'), '取消')
        self.click('dialog .danger')
        self.wait('return document.querySelectorAll(".task-row").length === 2')
        self.cdp('Emulation.setDeviceMetricsOverride', {'width': 360, 'height': 800, 'deviceScaleFactor': 1, 'mobile': True})
        self.cdp('Emulation.setEmulatedMedia', {'features': [{'name': 'prefers-color-scheme', 'value': 'dark'}, {'name': 'prefers-reduced-motion', 'value': 'reduce'}]})
        self.assertTrue(self.script('return matchMedia("(prefers-color-scheme: dark)").matches'))
        self.assertFalse(self.script('return document.documentElement.scrollWidth > innerWidth'))
        self.screenshot('mobile-dark.png')
        self.click('.section-heading .primary')
        self.assertEqual(self.script('return getComputedStyle(document.querySelector("dialog")).animationName'), 'none')
        self.type('#link', '\ue00c')
        self.wait('return !document.querySelector("dialog")')
        self.cdp('Emulation.clearDeviceMetricsOverride', {})

    def test_polling_stops_in_hidden_tab(self):
        before = sum(route == 'drive/v1/tasks' for _method, route, _body in smoke.ApiSmoke.nas.requests)
        time.sleep(3.4)
        after = sum(route == 'drive/v1/tasks' for _method, route, _body in smoke.ApiSmoke.nas.requests)
        self.assertGreater(after, before)
        original = self.command('GET', f'/session/{self.session}/window')
        new = self.command('POST', f'/session/{self.session}/window/new', {'type': 'tab'})
        self.command('POST', f'/session/{self.session}/window', {'handle': new['handle']})
        time.sleep(.3)
        before = len(smoke.ApiSmoke.nas.requests)
        time.sleep(3.4)
        self.assertEqual(len(smoke.ApiSmoke.nas.requests), before)
        self.command('DELETE', f'/session/{self.session}/window')
        self.command('POST', f'/session/{self.session}/window', {'handle': original})

    def test_empty_error_retry_and_quota_message(self):
        smoke.ApiSmoke.nas.tasks = []
        smoke.ApiSmoke.nas.usage = 3
        smoke.ApiSmoke.nas.failures = [503]
        self.command('POST', f'/session/{self.session}/url', {'url': smoke.ApiSmoke.base})
        self.wait('return document.querySelector(".error-state") !== null')
        self.assertIn('127.0.0.1', self.script('return document.querySelector(".error-state code").textContent'))
        self.click('.error-state button')
        self.wait('return document.querySelector(".empty h3")?.textContent === "让 NAS 开始忙碌吧"')
        self.assertIn('免费额度已用完', self.script('return document.querySelector(".quota").textContent'))
        self.click('.section-heading .primary')
        self.type('#link', 'https://example.invalid/archive.zip')
        self.click('.link-input button')
        self.wait('return document.querySelector(".parsed") !== null')
        self.click('.dialog-footer .primary')
        self.wait('return document.querySelector(".add-dialog .error")?.textContent.includes("今日免费额度已用完（3/3）")')
        self.assertEqual(smoke.ApiSmoke.nas.created, [])
        self.click('.icon-button')


if __name__ == '__main__':
    unittest.main(verbosity=2)
