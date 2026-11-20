// Exercise the real Tauri/WebView2 window with an isolated Settings profile.
const assert = require("node:assert/strict");
const { spawn, execFileSync } = require("node:child_process");
const { randomBytes } = require("node:crypto");
const fs = require("node:fs");
const net = require("node:net");
const os = require("node:os");
const path = require("node:path");
const { setTimeout: sleep } = require("node:timers/promises");
const { chromium } = require("playwright");

async function waitFor(name, check, timeout = 20000) {
  const deadline = Date.now() + timeout;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const result = await check();
      if (result) return result;
    } catch (error) {
      lastError = error;
    }
    await sleep(100);
  }
  throw Error(`Timed out waiting for ${name}`, { cause: lastError });
}

async function freePort() {
  const server = net.createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const { port } = server.address();
  await new Promise((resolve) => server.close(resolve));
  return port;
}

function nativeWindow(pid, action) {
  const script = `
import ctypes as c
from ctypes import wintypes as w
import sys
user = c.windll.user32
pid, action = int(sys.argv[1]), sys.argv[2]
user.IsWindowVisible.argtypes = [w.HWND]
user.GetWindowThreadProcessId.argtypes = [w.HWND, c.POINTER(w.DWORD)]
user.IsIconic.argtypes = [w.HWND]
user.ShowWindow.argtypes = [w.HWND, c.c_int]
user.PostMessageW.argtypes = [w.HWND, w.UINT, w.WPARAM, w.LPARAM]
callback = c.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)
def visit(hwnd, _):
    owner = w.DWORD()
    user.GetWindowThreadProcessId(hwnd, c.byref(owner))
    if owner.value != pid or not user.IsWindowVisible(hwnd):
        return True
    if action == "minimize":
        user.ShowWindow(hwnd, 6)
    if action == "close":
        user.PostMessageW(hwnd, 0x10, 0, 0)
    print("minimized" if user.IsIconic(hwnd) else "visible")
    return False
user.EnumWindows(callback(visit), 0)
`;
  return execFileSync("python", ["-c", script, String(pid), action], { encoding: "utf8", timeout: 5000 }).trim();
}

function processInfo(command) {
  return execFileSync("powershell.exe", ["-NoProfile", "-Command", command], {
    encoding: "utf8", timeout: 10000, windowsHide: true,
  }).trim().split(/\s+/).filter(Boolean).map(Number);
}

function processAlive(pid) {
  try { process.kill(pid, 0); return true; }
  catch (error) { if (error.code === "ESRCH") return false; throw error; }
}

async function main() {
  assert.equal(process.platform, "win32", "This test drives Windows native window controls");
  const bin = path.resolve(process.argv[2] || "target/debug");
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), "simple-stt-settings-window-"));
  const config = path.join(temp, "config.json");
  const port = await freePort();
  const cdp = `http://127.0.0.1:${port}`;
  const env = {
    ...process.env,
    SIMPLE_STT_CONFIG: config,
    SIMPLE_STT_RUNTIME_ROOT: path.join(temp, "runtime"),
    LOCALAPPDATA: path.join(temp, "data"),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
  };
  const processes = [];
  const browsers = [];
  const nativeProcesses = [];
  function launch(binary, args = []) {
    const child = spawn(path.join(bin, `${binary}.exe`), args, { env, windowsHide: true });
    child.stdout.resume();
    child.stderr.on("data", (chunk) => process.stderr.write(chunk));
    processes.push(child);
    return child;
  }
  function runCapture() {
    return launch("simple-stt-capture", ["--token", randomBytes(32).toString("hex"), "--config", config]);
  }
  async function settingsPage() {
    await waitFor("WebView2 debug listener", async () => (await fetch(`${cdp}/json/version`)).ok);
    const browser = await chromium.connectOverCDP(cdp);
    browsers.push(browser);
    const context = browser.contexts()[0];
    const page = await waitFor("Settings document", () => context.pages().find((tab) => tab.url().includes("127.0.0.1")));
    await page.locator("#ui-language").waitFor();
    const health = await page.evaluate(async () => {
      const token = new URLSearchParams(location.hash.slice(1)).get("token");
      const response = await fetch("/api/health", { headers: { "X-Simple-STT-Token": token } });
      return response.json();
    });
    assert.equal(health.ok, true);
    nativeProcesses.push(health.pid);
    return { page, context, pid: health.pid };
  }
  try {
    const started = Date.now();
    const launcher = launch("simple-stt-settings");
    const { page, context, pid: host } = await settingsPage();
    const startup = Date.now() - started;
    assert.equal(launcher.exitCode, 0, "Only the Tauri Settings process should remain");
    assert.equal(nativeWindow(host, "state"), "visible", "The Settings server must own the window");
    assert.equal(nativeWindow(host, "minimize"), "minimized");
    const duplicate = launch("simple-stt-settings");
    await waitFor("second launch to exit", () => duplicate.exitCode !== null);
    assert.equal(duplicate.exitCode, 0);
    await waitFor("existing native window to restore", () => nativeWindow(host, "state") === "visible");
    assert.equal(context.pages().length, 1);
    console.log(`PASS: default opens one native Settings window in ${startup} ms; relaunch restores it`);

    assert.equal(await page.locator("#service").count(), 0);
    assert.equal(await page.getByRole("button", { name: "Close Settings" }).count(), 0);
    const language = page.locator("#ui-language");
    await language.selectOption("en");
    await page.locator("[data-page=models]").click();
    await page.getByText("Custom model locations", { exact: true }).filter({ visible: true }).first().click();
    const unload = page.getByRole("button", { name: "Unload speech model" });
    await waitFor("capture actions disabled while offline", () => unload.isDisabled());
    const firstCapture = runCapture();
    await waitFor("capture actions enabled on connection", () => unload.isEnabled());
    const webview = await context.newCDPSession(page);
    await webview.send("Page.setWebLifecycleState", { state: "frozen" });
    firstCapture.kill();
    await waitFor("capture to exit while Settings is suspended", () => firstCapture.exitCode !== null || firstCapture.signalCode !== null);
    await webview.send("Page.setWebLifecycleState", { state: "active" });
    await waitFor("capture actions disabled after disconnect", () => unload.isDisabled());
    const capture = runCapture();
    await waitFor("capture actions reenabled after restart", () => unload.isEnabled());
    await page.locator("[data-page=general]").click();
    assert.equal(await language.inputValue(), "en", "Reconnect discarded the unsaved draft");
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await waitFor("saved Settings JSON", () => JSON.parse(fs.readFileSync(config, "utf8")).general.ui_language === "en");
    await page.locator("#notice").waitFor();
    const position = await page.locator("#notice").boundingBox();
    const height = await page.evaluate(() => innerHeight);
    assert.ok(position.x < 25 && position.y + position.height > height - 30, "native notice did not use the bottom left");
    console.log("PASS: Settings connects after capture startup, reconnects after credential rotation, and saves the retained draft");

    const webviews = processInfo(`
      $webviews = Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'"
      $parents = @(${host})
      while ($parents) {
        $parents = @($webviews | Where-Object { $_.ParentProcessId -in $parents } | Select-Object -ExpandProperty ProcessId)
        $parents
      }
    `);
    assert.ok(webviews.length, "No WebView2 processes identified for the isolated profile");
    const origin = new URL(page.url()).origin;
    await language.selectOption("auto");
    let dialog = page.waitForEvent("dialog");
    assert.equal(nativeWindow(host, "close"), "visible");
    await (await dialog).dismiss();
    assert.equal(await language.inputValue(), "auto");
    dialog = page.waitForEvent("dialog");
    nativeWindow(host, "close");
    await (await dialog).accept();
    await waitFor("Settings and WebView2 processes to exit", () => [host, ...webviews].every((pid) => !processAlive(pid)));
    await assert.rejects(fetch(origin));
    assert.equal(capture.exitCode, null, "Closing Settings stopped the capture service");
    console.log("PASS: titlebar Close guards the draft and releases Settings and all profile WebView2 processes; capture stays running");

    launch("simple-stt-settings");
    const { page: fresh, pid: reopened } = await settingsPage();
    assert.equal(await fresh.locator("#ui-language").inputValue(), "en");
    assert.equal(await fresh.getByRole("button", { name: "Close Settings" }).count(), 0);
    assert.equal(nativeWindow(reopened, "close"), "visible");
    await waitFor("clean titlebar Close to exit Settings", () => !processAlive(reopened));
    console.log("PASS: reopen reads saved Settings and the native Close exits the window");
  } finally {
    for (const pid of nativeProcesses) {
      if (processAlive(pid)) nativeWindow(pid, "close");
    }
    for (const child of processes.reverse()) child.kill();
    for (const browser of browsers) await browser.close().catch(() => {});
    await sleep(500);
    fs.rmSync(temp, { recursive: true, force: true, maxRetries: 4, retryDelay: 300 });
  }
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
