import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { join } from "node:path";
import { createServer } from "node:net";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";
import { inflateSync } from "node:zlib";

const root = new URL("..", import.meta.url);
const localOpenGlOnlyMarker = new URL(".witty-local-opengl-only", root);
if (
  existsSync(localOpenGlOnlyMarker) &&
  process.env.WITTY_ALLOW_LOCAL_CHROMIUM_SMOKE !== "1"
) {
  throw new Error(
    "Witty browser real-TUI smoke is disabled by .witty-local-opengl-only; set WITTY_ALLOW_LOCAL_CHROMIUM_SMOKE=1 to override deliberately.",
  );
}
const dist = new URL("target/witty-web-smoke/", root);
const artifactDir = new URL("target/witty-web-real-tui/", root);
const caseId = process.env.WITTY_WEB_REAL_TUI_CASE ?? "less-basic-restore";
const gatewayMode = process.env.WITTY_WEB_SMOKE_GATEWAY ?? "rust";
const port = Number.parseInt(process.env.WITTY_WEB_REAL_TUI_PORT ?? "8887", 10);
const gatewayPort = Number.parseInt(
  process.env.WITTY_WEB_REAL_TUI_GATEWAY_PORT ?? String(port + 1),
  10,
);
const gatewayToken = process.env.WITTY_WEB_GATEWAY_TOKEN ?? "witty-real-tui-token";
const gatewayUrl = `ws://127.0.0.1:${gatewayPort}/witty?token=${encodeURIComponent(gatewayToken)}`;
const playwrightRoot =
  process.env.WITTY_PLAYWRIGHT_ROOT ?? new URL("target/witty-web-smoke-tools/", root).pathname;
const requirePlaywright = createRequire(pathToFileURL(join(playwrightRoot, "package.json")));
const { chromium } = requirePlaywright("playwright");
const executablePath = process.env.WITTY_CHROMIUM_EXECUTABLE || undefined;

mkdirSync(artifactDir, { recursive: true });

if (!["rust", "launcher"].includes(gatewayMode)) {
  throw new Error(
    `WITTY_WEB_SMOKE_GATEWAY must be rust or launcher for real-TUI smoke, got ${gatewayMode}`,
  );
}

const realTuiProgram = setupRealTuiProgram(caseId);
let url = `http://127.0.0.1:${port}/index.html`;

function setupRealTuiProgram(id) {
  if (id === "less-basic-restore") {
    return setupLessProgram(id);
  }
  if (id === "vim-basic-edit") {
    return setupVimProgram(id);
  }
  if (id === "tmux-basic-pane") {
    return setupTmuxProgram(id);
  }
  throw new Error(`unsupported real-TUI browser smoke case ${id}`);
}

function setupLessProgram(id) {
  const lessPath = findExecutable("less");
  if (!lessPath) {
    throw new Error("less not found; cannot run less-basic-restore browser smoke");
  }
  const fixture = new URL("less-basic-restore.txt", artifactDir);
  writeFileSync(fixture, lessFixtureText());
  const command = [
    "TERM=xterm-256color",
    "COLORTERM=truecolor",
    "LC_ALL=C.UTF-8",
    "LESS=",
    "exec",
    shellQuote(lessPath),
    "-R",
    "-M",
    shellQuote(fixture.pathname),
  ].join(" ");
  return {
    id,
    binary: "/bin/sh",
    args: ["-lc", command],
    initialText: "Line 001",
    searchText: "Line 050",
    exitKey: "q",
  };
}

function setupVimProgram(id) {
  const vimPath = findExecutable("vim");
  if (!vimPath) {
    throw new Error("vim not found; cannot run vim-basic-edit browser smoke");
  }
  const fixture = new URL("vim-basic-edit.txt", artifactDir);
  writeFileSync(fixture, editorFixtureText());
  const supportDirs = createSupportDirs(id);
  const token = "WITTY_browser_vim_smoke_";
  const command = [
    ...controlledEnvAssignments(supportDirs),
    "VIMINIT=",
    "GVIMINIT=",
    "EXINIT=",
    "exec",
    shellQuote(vimPath),
    "-Nu",
    "NONE",
    "-n",
    "-i",
    "NONE",
    "-N",
    shellQuote(fixture.pathname),
  ].join(" ");
  return {
    id,
    kind: "vim",
    binary: "/bin/sh",
    args: ["-lc", command],
    initialText: "Witty browser editor smoke fixture",
    insertToken: token,
    insertInput: `gg0i${token}\x1b`,
    exitInput: ":wq\r",
    fixturePath: fixture.pathname,
  };
}

function setupTmuxProgram(id) {
  const tmuxPath = findExecutable("tmux");
  if (!tmuxPath) {
    throw new Error("tmux not found; cannot run tmux-basic-pane browser smoke");
  }
  const supportDirs = createSupportDirs(id);
  const socketPath = new URL("tmux-browser-smoke.sock", supportDirs.workDir);
  const configPath = new URL("tmux-browser-smoke.conf", supportDirs.workDir);
  writeFileSync(configPath, tmuxConfigText());
  const command = [
    ...controlledEnvAssignments(supportDirs),
    "exec",
    shellQuote(tmuxPath),
    "-S",
    shellQuote(socketPath.pathname),
    "-f",
    shellQuote(configPath.pathname),
    "-u",
    "new-session",
    "-s",
    "witty-browser-smoke",
    shellQuote("/bin/sh -lc 'printf \"TMUX READY\\n\"; exec /bin/sh'"),
  ].join(" ");
  return {
    id,
    kind: "tmux",
    binary: "/bin/sh",
    args: ["-lc", command],
    tmuxPath,
    socketPath: socketPath.pathname,
    initialText: "TMUX READY",
    paneText: "TMUX BROWSER PANE OK",
  };
}

function lessFixtureText() {
  let text = "";
  for (let line = 1; line <= 120; line += 1) {
    const label = String(line).padStart(3, "0");
    text += `Line ${label} Witty browser real TUI fixture row ${label}\n`;
  }
  return text;
}

function editorFixtureText() {
  return [
    "Witty browser editor smoke fixture",
    "This file is edited through the browser real-TUI gateway.",
    "The first line should receive a deterministic prefix.",
    "",
  ].join("\n");
}

function createSupportDirs(id) {
  const safeId = id.replaceAll(/[^A-Za-z0-9_-]/g, "_");
  const workDir = new URL(`${safeId}-support/`, artifactDir);
  const home = new URL("home/", workDir);
  const xdgConfig = new URL("xdg-config/", workDir);
  const xdgCache = new URL("xdg-cache/", workDir);
  const xdgState = new URL("xdg-state/", workDir);
  for (const dir of [workDir, home, xdgConfig, xdgCache, xdgState]) {
    mkdirSync(dir, { recursive: true });
  }
  return { workDir, home, xdgConfig, xdgCache, xdgState };
}

function controlledEnvAssignments({ home, xdgConfig, xdgCache, xdgState }) {
  return [
    "TERM=xterm-256color",
    "COLORTERM=truecolor",
    "LC_ALL=C.UTF-8",
    `HOME=${shellQuote(home.pathname)}`,
    `XDG_CONFIG_HOME=${shellQuote(xdgConfig.pathname)}`,
    `XDG_CACHE_HOME=${shellQuote(xdgCache.pathname)}`,
    `XDG_STATE_HOME=${shellQuote(xdgState.pathname)}`,
  ];
}

function tmuxConfigText() {
  return [
    'set -g default-terminal "screen-256color"',
    "set -g status on",
    'set -g status-left "Witty"',
    'set -g status-right ""',
    "set -g prefix C-b",
    "",
  ].join("\n");
}

function shellQuote(value) {
  return `'${String(value).replaceAll("'", "'\\''")}'`;
}

function findExecutable(name) {
  const result = spawnSync("bash", ["-lc", `command -v ${name}`], { encoding: "utf8" });
  if (result.status !== 0) {
    return null;
  }
  return result.stdout.trim() || null;
}

function cleanupRealTuiProgram() {
  if (realTuiProgram.kind === "tmux") {
    spawnSync(realTuiProgram.tmuxPath, ["-S", realTuiProgram.socketPath, "kill-server"], {
      stdio: "ignore",
    });
  }
}

const server =
  gatewayMode === "launcher"
    ? null
    : spawn(
        "python3",
        ["-m", "http.server", String(port), "--bind", "127.0.0.1", "--directory", dist.pathname],
        { stdio: ["ignore", "pipe", "pipe"] },
      );

server?.stdout.on("data", (chunk) => process.stdout.write(chunk));
server?.stderr.on("data", (chunk) => process.stderr.write(chunk));

function startGateway() {
  if (gatewayMode === "rust") {
    return startRustGateway();
  }
  return startLauncher();
}

function startRustGateway() {
  const bind = `127.0.0.1:${gatewayPort}`;
  const args = [
    "run",
    "-p",
    "witty-gateway",
    "--",
    "--once",
    "--bind",
    bind,
    "--token",
    gatewayToken,
    "--allow-origin",
    `http://127.0.0.1:${port}`,
    "--program",
    realTuiProgram.binary,
  ];
  for (const arg of realTuiProgram.args) {
    args.push("--arg", arg);
  }

  const child = spawn("cargo", args, {
    cwd: root.pathname,
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.on("data", (chunk) => process.stdout.write(chunk));

  return {
    kind: "rust",
    child,
    start() {
      return waitForStderr(child, /Witty gateway listening on /, "witty-gateway listen");
    },
    stop() {
      stopChild(child);
    },
    waitForExit(timeoutMs = 5000) {
      return waitForExit(child, timeoutMs, "witty-gateway");
    },
  };
}

function startLauncher() {
  const args = [
    "run",
    "-p",
    "witty-app",
    "--",
    "--web",
    "--web-root",
    dist.pathname,
    "--program",
    realTuiProgram.binary,
  ];
  for (const arg of realTuiProgram.args) {
    args.push("--arg", arg);
  }

  const child = spawn("cargo", args, {
    cwd: root.pathname,
    stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.on("data", (chunk) => process.stdout.write(chunk));

  return {
    kind: "launcher",
    child,
    async start() {
      const match = await waitForStderr(
        child,
        /Witty launcher listening on (http:\/\/\S+)/,
        "witty --web listen",
      );
      url = match[1];
    },
    stop() {
      stopChild(child);
    },
    waitForExit(timeoutMs = 5000) {
      return waitForExit(child, timeoutMs, "witty --web");
    },
  };
}

function waitForStderr(child, pattern, label, timeoutMs = 30000) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error(`timed out waiting for ${label}`));
    }, timeoutMs);
    child.stderr.on("data", onData);
    child.on("exit", onExit);

    function cleanup() {
      clearTimeout(timeout);
      child.stderr.off("data", onData);
      child.off("exit", onExit);
    }

    function onData(chunk) {
      const text = chunk.toString("utf8");
      process.stderr.write(chunk);
      const match = text.match(pattern);
      if (match) {
        cleanup();
        resolve(match);
      }
    }

    function onExit(code, signal) {
      cleanup();
      reject(new Error(`${label} exited before ready: ${JSON.stringify({ code, signal })}`));
    }
  });
}

function waitForExit(child, timeoutMs, label) {
  return new Promise((resolve, reject) => {
    if (child.exitCode !== null || child.signalCode !== null) {
      resolve({ code: child.exitCode, signal: child.signalCode });
      return;
    }
    const timeout = setTimeout(() => {
      reject(new Error(`${label} did not exit within ${timeoutMs} ms`));
    }, timeoutMs);
    child.once("exit", (code, signal) => {
      clearTimeout(timeout);
      resolve({ code, signal });
    });
  }).then((status) => {
    if (status.code !== 0 || status.signal) {
      throw new Error(`${label} exited unexpectedly: ${JSON.stringify(status)}`);
    }
    return status;
  });
}

function stopChild(child) {
  if (child && child.exitCode === null && child.signalCode === null) {
    child.kill("SIGTERM");
  }
}

async function waitForServerReady() {
  if (gatewayMode === "launcher") {
    return;
  }
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        return;
      }
    } catch {
      await delay(100);
    }
  }
  throw new Error(`server did not become ready at ${url}`);
}

function stopServer() {
  if (server && !server.killed) {
    server.kill("SIGTERM");
  }
}

async function waitForGatewayOutputText(page, text, label, timeout = 10000) {
  const started = Date.now();
  let lastOutput = "";
  while (Date.now() - started < timeout) {
    await page.evaluate(() => window.wittyGatewayIdle?.());
    await delay(50);
    lastOutput = await page.evaluate(() => window.wittyGatewayOutputText ?? "");
    if (lastOutput.includes(text)) {
      return;
    }
    await delay(100);
  }
  throw new Error(`${label} not observed within ${timeout} ms\n${JSON.stringify(lastOutput)}`);
}

async function runBrowserSmoke(page) {
  if (realTuiProgram.kind === "vim") {
    return runVimBrowserSmoke(page);
  }
  if (realTuiProgram.kind === "tmux") {
    return runTmuxBrowserSmoke(page);
  }
  return runLessBrowserSmoke(page);
}

async function runLessBrowserSmoke(page) {
  await waitForGatewayOutputText(page, realTuiProgram.initialText, "initial less text");
  await page.locator("#witty-canvas").focus();
  await writeTerminalText(page, `/${realTuiProgram.searchText}\r`);
  await waitForGatewayOutputText(page, realTuiProgram.searchText, "searched less text");
  await writeTerminalText(page, realTuiProgram.exitKey);

  const screenshotPath = await captureNonblankCanvasScreenshot(page);

  return {
    caseId,
    gatewayMode,
    screenshot: screenshotPath.pathname,
    outputBytes: await page.evaluate(() => window.wittyGatewayOutputText?.length ?? 0),
  };
}

async function runVimBrowserSmoke(page) {
  await waitForGatewayOutputText(page, realTuiProgram.initialText, "initial vim text");
  await page.locator("#witty-canvas").focus();
  await writeTerminalText(page, realTuiProgram.insertInput);
  await waitForGatewayOutputText(page, realTuiProgram.insertToken, "inserted vim token");
  await readScreenText(page);
  const screenshotPath = await captureNonblankCanvasScreenshot(page);
  await writeTerminalText(page, realTuiProgram.exitInput);

  return {
    caseId,
    gatewayMode,
    screenshot: screenshotPath.pathname,
    outputBytes: await page.evaluate(() => window.wittyGatewayOutputText?.length ?? 0),
    verifyAfterExit() {
      const fileText = readFileSync(realTuiProgram.fixturePath, "utf8");
      if (!fileText.startsWith(realTuiProgram.insertToken)) {
        throw new Error("vim smoke fixture did not start with inserted token");
      }
      if (!fileText.includes(realTuiProgram.initialText)) {
        throw new Error("vim smoke fixture lost original text");
      }
    },
  };
}

async function runTmuxBrowserSmoke(page) {
  await waitForGatewayOutputText(page, realTuiProgram.initialText, "initial tmux pane text");
  await page.locator("#witty-canvas").focus();
  await writeTerminalText(page, "\x02\"");
  await delay(100);
  await writeTerminalText(page, "stty -echo\r");
  await delay(100);
  await writeTerminalText(page, `printf '${realTuiProgram.paneText}\\n'\r`);
  await waitForGatewayOutputText(page, realTuiProgram.paneText, "tmux split pane output");
  await readScreenText(page);
  const screenshotPath = await captureNonblankCanvasScreenshot(page);
  await writeTerminalText(page, "\x02d");

  return {
    caseId,
    gatewayMode,
    screenshot: screenshotPath.pathname,
    outputBytes: await page.evaluate(() => window.wittyGatewayOutputText?.length ?? 0),
    cleanup() {
      spawnSync(realTuiProgram.tmuxPath, ["-S", realTuiProgram.socketPath, "kill-server"], {
        stdio: "ignore",
      });
    },
  };
}

async function readScreenText(page) {
  return page.evaluate(async () => {
    if (window.wittyReadScreenText) {
      return window.wittyReadScreenText();
    }
    await window.wittyGatewayIdle?.();
    return window.wittySession?.screen_text?.() ?? "";
  });
}

async function captureNonblankCanvasScreenshot(page) {
  const screenshot = await page.locator("#witty-canvas").screenshot({ type: "png" });
  const screenshotPath = new URL(`${caseId}-${gatewayMode}.png`, artifactDir);
  await writeFile(screenshotPath, screenshot);
  if (!pngHasNonblackPixel(screenshot)) {
    throw new Error(`real-TUI browser smoke rendered blank canvas at ${screenshotPath.pathname}`);
  }
  return screenshotPath;
}

async function writeTerminalText(page, text) {
  const bytes = Array.from(Buffer.from(text, "utf8"));
  await page.evaluate((inputBytes) => {
    if (!window.wittySendGatewayInputBytes) {
      throw new Error("wittySendGatewayInputBytes helper is missing");
    }
    window.wittySendGatewayInputBytes(inputBytes);
  }, bytes);
}

function pngHasNonblackPixel(buffer) {
  const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  if (!buffer.subarray(0, 8).equals(signature)) {
    throw new Error("screenshot is not a PNG");
  }

  let offset = 8;
  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  const idatChunks = [];

  while (offset < buffer.length) {
    const length = buffer.readUInt32BE(offset);
    const type = buffer.subarray(offset + 4, offset + 8).toString("ascii");
    const data = buffer.subarray(offset + 8, offset + 8 + length);
    offset += 12 + length;

    if (type === "IHDR") {
      width = data.readUInt32BE(0);
      height = data.readUInt32BE(4);
      bitDepth = data[8];
      colorType = data[9];
    } else if (type === "IDAT") {
      idatChunks.push(data);
    } else if (type === "IEND") {
      break;
    }
  }

  if (bitDepth !== 8 || ![0, 2, 6].includes(colorType)) {
    throw new Error(`unsupported PNG format bitDepth=${bitDepth} colorType=${colorType}`);
  }

  const bytesPerPixel = colorType === 6 ? 4 : colorType === 2 ? 3 : 1;
  const stride = width * bytesPerPixel;
  const inflated = inflateSync(Buffer.concat(idatChunks));
  let readOffset = 0;
  const previous = Buffer.alloc(stride);
  const current = Buffer.alloc(stride);

  for (let row = 0; row < height; row += 1) {
    const filter = inflated[readOffset];
    readOffset += 1;
    for (let column = 0; column < stride; column += 1) {
      const raw = inflated[readOffset + column];
      const left = column >= bytesPerPixel ? current[column - bytesPerPixel] : 0;
      const up = previous[column];
      const upLeft = column >= bytesPerPixel ? previous[column - bytesPerPixel] : 0;
      if (filter === 0) {
        current[column] = raw;
      } else if (filter === 1) {
        current[column] = (raw + left) & 0xff;
      } else if (filter === 2) {
        current[column] = (raw + up) & 0xff;
      } else if (filter === 3) {
        current[column] = (raw + Math.floor((left + up) / 2)) & 0xff;
      } else if (filter === 4) {
        current[column] = (raw + paeth(left, up, upLeft)) & 0xff;
      } else {
        throw new Error(`unsupported PNG filter=${filter}`);
      }
    }
    readOffset += stride;
    for (let column = 0; column < stride; column += bytesPerPixel) {
      const red = current[column];
      const green = colorType === 0 ? red : current[column + 1];
      const blue = colorType === 0 ? red : current[column + 2];
      if (red !== 0 || green !== 0 || blue !== 0) {
        return true;
      }
    }
    current.copy(previous);
  }

  return false;
}

function paeth(left, up, upLeft) {
  const estimate = left + up - upLeft;
  const leftDistance = Math.abs(estimate - left);
  const upDistance = Math.abs(estimate - up);
  const upLeftDistance = Math.abs(estimate - upLeft);
  if (leftDistance <= upDistance && leftDistance <= upLeftDistance) {
    return left;
  }
  return upDistance <= upLeftDistance ? up : upLeft;
}

const gateway = startGateway();
let browser;

try {
  await gateway.start();
  await waitForServerReady();
  browser = await chromium.launch({
    executablePath,
    args: ["--enable-unsafe-webgpu"],
  });
  const page = await browser.newPage({ viewport: { width: 1000, height: 620 } });
  await page.goto(url);
  await page.waitForFunction(
    () => document.documentElement.dataset.wittySmoke === "ok",
    { timeout: 30000 },
  );

  if (gatewayMode === "rust") {
    await page.evaluate((targetUrl) => window.wittyConnectGateway(targetUrl), gatewayUrl);
  }

  await page.waitForFunction(
    () =>
      window.wittyGatewayFrames?.some(
        ({ direction, frame }) => direction === "server" && frame.type === "ready",
      ),
    { timeout: 5000 },
  );
  const result = await runBrowserSmoke(page);
  const gatewayExit = await gateway.waitForExit();
  result.verifyAfterExit?.();
  result.cleanup?.();

  if (gatewayMode === "launcher") {
    await browser.close();
    browser = null;
  }

  console.log(
    `Witty browser real-TUI smoke ${caseId} via ${gatewayMode} passed; gateway exit=${gatewayExit.code}; screenshot=${result.screenshot}`,
  );
} finally {
  if (browser) {
    await browser.close();
  }
  gateway.stop();
  stopServer();
  cleanupRealTuiProgram();
}
