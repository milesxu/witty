import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { chmod, mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { createServer } from "node:net";
import { delimiter, join } from "node:path";
import { tmpdir } from "node:os";
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
    "Witty browser smoke is disabled by .witty-local-opengl-only; set WITTY_ALLOW_LOCAL_CHROMIUM_SMOKE=1 to override deliberately.",
  );
}
const dist = new URL("target/witty-web-smoke/", root);
const canvasScreenshot = new URL("target/witty-web-smoke/smoke-canvas.png", root);
const port = Number.parseInt(process.env.WITTY_WEB_SMOKE_PORT ?? "8787", 10);
const gatewayPort = Number.parseInt(
  process.env.WITTY_WEB_GATEWAY_SMOKE_PORT ?? String(port + 1),
  10,
);
const gatewayMode = process.env.WITTY_WEB_SMOKE_GATEWAY ?? "node";
const launcherUseDefaultWebRoot =
  process.env.WITTY_WEB_SMOKE_LAUNCHER_DEFAULT_ROOT === "1";
const launcherScrollbackLines = Number.parseInt(
  process.env.WITTY_WEB_SMOKE_SCROLLBACK_LINES ?? "64",
  10,
);
if (!Number.isSafeInteger(launcherScrollbackLines) || launcherScrollbackLines < 0) {
  throw new Error(`invalid WITTY_WEB_SMOKE_SCROLLBACK_LINES=${launcherScrollbackLines}`);
}
let url = `http://127.0.0.1:${port}/index.html`;
const gatewayToken =
  process.env.WITTY_WEB_GATEWAY_TOKEN ??
  (gatewayMode === "rust" ? "witty-smoke-token" : "");
const gatewayQuery = gatewayToken ? `?token=${encodeURIComponent(gatewayToken)}` : "";
const gatewayUrl = `ws://127.0.0.1:${gatewayPort}/witty${gatewayQuery}`;
const gatewaySmokeTitle = "Witty Gateway Smoke";
const gatewayAltScreenText = "WITTY ALT SCREEN SMOKE";
const gatewayMainScreenText = "WITTY MAIN SCREEN RESTORED";
const profilePickerPageUrlPattern = /^\/index\.html#profile_picker=[0-9a-f]{32}$/;
const profileImportPageUrlPattern = /^\/index\.html#profile_import=[0-9a-f]{32}$/;
const profilePickerSelectionUrlPattern = /^\/profile-picker\/[0-9a-f]{32}\/select$/;
const profilePickerImportUrlPattern = /^\/profile-picker\/[0-9a-f]{32}\/import$/;
const profileImportConfirmUrlPattern = /^\/profile-import\/[0-9a-f]{32}\/confirm$/;
const launcherTokenPattern = /^[0-9a-f]{64}$/;
const browserMaxGlyphRunChars = 120;
const playwrightRoot =
  process.env.WITTY_PLAYWRIGHT_ROOT ?? new URL("target/witty-web-smoke-tools/", root).pathname;
const requirePlaywright = createRequire(pathToFileURL(join(playwrightRoot, "package.json")));
const { chromium } = requirePlaywright("playwright");
const executablePath = process.env.WITTY_CHROMIUM_EXECUTABLE || undefined;

function fakeLauncherToken(hexDigit) {
  return String(hexDigit).repeat(64);
}

function launcherSessionConfigFixture(token = fakeLauncherToken("a")) {
  return {
    protocol: 1,
    gateway_url: "ws://127.0.0.1:12345/witty",
    token,
    mouse_selection_override: "shift-select",
    scrollback_lines: 64,
    expires_at_ms: 180000,
  };
}

const launcherBackedModes = new Set([
  "launcher",
  "profile-picker",
  "profile-import",
  "profile-import-reject",
  "profile-picker-import",
]);
const launcherBackedGatewayModes = new Set(["launcher", "profile-picker", "profile-picker-import"]);
const profileImportOnlyModes = new Set(["profile-import", "profile-import-reject"]);
const profileImportModes = new Set([...profileImportOnlyModes, "profile-picker-import"]);
const server =
  launcherBackedModes.has(gatewayMode)
    ? null
    : spawn(
        "python3",
        ["-m", "http.server", String(port), "--bind", "127.0.0.1", "--directory", dist.pathname],
        { stdio: ["ignore", "pipe", "pipe"] },
      );

server?.stdout.on("data", (chunk) => process.stdout.write(chunk));
server?.stderr.on("data", (chunk) => process.stderr.write(chunk));

async function waitForServer() {
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

function createGatewaySmokeServer(listenPort) {
  const protocolGuid = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
  const receivedFrames = [];
  const inputBytes = [];
  const waiters = [];
  const sockets = new Set();
  let echoed = false;
  let listening = false;

  const gatewayServer = createServer((socket) => {
    const state = {
      buffer: Buffer.alloc(0),
      handshakeComplete: false,
    };
    sockets.add(socket);

    socket.on("data", (chunk) => {
      try {
        state.buffer = Buffer.concat([state.buffer, chunk]);
        if (!state.handshakeComplete) {
          const requestText = state.buffer.toString("latin1");
          const requestEnd = requestText.indexOf("\r\n\r\n");
          if (requestEnd === -1) {
            return;
          }

          completeWebSocketHandshake(socket, requestText.slice(0, requestEnd), protocolGuid);
          state.handshakeComplete = true;
          state.buffer = state.buffer.subarray(requestEnd + 4);
        }

        state.buffer = consumeWebSocketFrames(state.buffer, (opcode, payload) => {
          handleGatewayFrame(socket, opcode, payload);
        });
      } catch (error) {
        socket.destroy(error);
      }
    });

    socket.on("close", () => {
      sockets.delete(socket);
    });
  });

  function handleGatewayFrame(socket, opcode, payload) {
    if (opcode === 0x8) {
      socket.end();
      return;
    }
    if (opcode === 0x9) {
      sendWebSocketFrame(socket, 0xa, payload);
      return;
    }
    if (opcode !== 0x1) {
      return;
    }

    const frame = JSON.parse(payload.toString("utf8"));
    receivedFrames.push(frame);

    if (frame.type === "hello") {
      sendGatewayJson(socket, { type: "ready", protocol: 1 });
      sendGatewayJson(socket, {
        type: "output",
        bytes: Array.from(
          Buffer.from(
            `\x1b]2;${gatewaySmokeTitle}\x07shell ready\r\n\x1b[?1049h${gatewayAltScreenText}\r\n`,
            "utf8",
          ),
        ),
      });
    } else if (frame.type === "input" && Array.isArray(frame.bytes)) {
      inputBytes.push(...frame.bytes);
      if (!echoed && bytePrefixEquals(inputBytes, [120, 121, 13])) {
        echoed = true;
        sendGatewayJson(socket, {
          type: "output",
          bytes: Array.from(
            Buffer.from(`\x1b[?1049l${gatewayMainScreenText}\r\ngateway ws ok\r\n> `, "utf8"),
          ),
        });
      }
    }

    notifyWaiters();
  }

  function notifyWaiters() {
    for (let index = waiters.length - 1; index >= 0; index -= 1) {
      const waiter = waiters[index];
      const value = waiter.condition();
      if (value) {
        clearTimeout(waiter.timeout);
        waiters.splice(index, 1);
        waiter.resolve(value);
      }
    }
  }

  function waitFor(condition, label, timeoutMs = 5000) {
    const immediate = condition();
    if (immediate) {
      return Promise.resolve(immediate);
    }

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        const index = waiters.findIndex((waiter) => waiter.resolve === resolve);
        if (index !== -1) {
          waiters.splice(index, 1);
        }
        reject(new Error(`timed out waiting for gateway ${label}`));
      }, timeoutMs);
      waiters.push({ condition, resolve, timeout });
    });
  }

  return {
    kind: "node",
    start() {
      return new Promise((resolve, reject) => {
        gatewayServer.once("error", reject);
        gatewayServer.listen(listenPort, "127.0.0.1", () => {
          gatewayServer.off("error", reject);
          listening = true;
          resolve();
        });
      });
    },
    stop() {
      for (const socket of sockets) {
        socket.end();
      }
      if (listening) {
        listening = false;
        gatewayServer.close();
      }
    },
    frameCount(type) {
      return receivedFrames.filter((frame) => frame.type === type).length;
    },
    waitForFrame(type, predicate = () => true) {
      return waitFor(
        () => receivedFrames.find((frame) => frame.type === type && predicate(frame)),
        `${type} frame`,
      );
    },
    waitForInputBytes(expected) {
      return waitFor(
        () => (bytePrefixEquals(inputBytes, expected) ? [...inputBytes] : null),
        `input bytes ${JSON.stringify(expected)}`,
      );
    },
    waitForResizeCount(count) {
      return waitFor(
        () => {
          const resizeFrames = receivedFrames.filter((frame) => frame.type === "resize");
          return resizeFrames.length >= count ? resizeFrames.at(-1) : null;
        },
        `${count} resize frames`,
      );
    },
  };
}

function createRustGatewaySmokeServer(listenPort) {
  const bind = `127.0.0.1:${listenPort}`;
  const shellScript =
    'stty -echo 2>/dev/null; printf "\\033]2;Witty Gateway Smoke\\007shell ready\\r\\n\\033[?1049hWITTY ALT SCREEN SMOKE\\r\\n"; while IFS= read -r line; do printf "\\033[?1049lWITTY MAIN SCREEN RESTORED\\r\\npty saw:%s\\r\\n> " "$line"; done';
  let gatewayProcess = null;
  let exited = null;

  return {
    kind: "rust",
    start() {
      const gatewayArgs = [
        "run",
        "-p",
        "witty-gateway",
        "--",
        "--once",
        "--bind",
        bind,
        "--allow-origin",
        `http://127.0.0.1:${port}`,
      ];
      if (gatewayToken) {
        gatewayArgs.push("--token", gatewayToken);
      }
      gatewayArgs.push(
        "--program",
        "/bin/sh",
        "--arg",
        "-lc",
        "--arg",
        shellScript,
      );

      gatewayProcess = spawn(
        "cargo",
        gatewayArgs,
        { cwd: root.pathname, stdio: ["ignore", "pipe", "pipe"] },
      );

      gatewayProcess.stdout.on("data", (chunk) => process.stdout.write(chunk));

      return new Promise((resolve, reject) => {
        let ready = false;
        const onStderr = (chunk) => {
          const text = chunk.toString("utf8");
          process.stderr.write(chunk);
          if (!ready && text.includes("Witty gateway listening on")) {
            ready = true;
            resolve();
          }
        };
        gatewayProcess.stderr.on("data", onStderr);
        gatewayProcess.on("exit", (code, signal) => {
          exited = { code, signal };
          if (!ready) {
            reject(new Error(`witty-gateway exited before listening: ${JSON.stringify(exited)}`));
          }
        });
      });
    },
    stop() {
      if (gatewayProcess && !gatewayProcess.killed && !exited) {
        gatewayProcess.kill("SIGTERM");
      }
    },
  };
}

function createLauncherSmokeServer() {
  const shellScript =
    'stty -echo 2>/dev/null; printf "\\033]2;Witty Gateway Smoke\\007shell ready\\r\\n\\033[?1049hWITTY ALT SCREEN SMOKE\\r\\n"; while IFS= read -r line; do printf "\\033[?1049lWITTY MAIN SCREEN RESTORED\\r\\npty saw:%s\\r\\n> " "$line"; done';
  let launcherProcess = null;
  let exited = null;

  return {
    kind: "launcher",
    start() {
      const launcherArgs = [
        "run",
        "-p",
        "witty-app",
        "--",
        "--web",
        "--scrollback-lines",
        String(launcherScrollbackLines),
        "--program",
        "/bin/sh",
        "--arg",
        "-lc",
        "--arg",
        shellScript,
      ];
      if (!launcherUseDefaultWebRoot) {
        launcherArgs.splice(5, 0, "--web-root", dist.pathname);
      }

      launcherProcess = spawn("cargo", launcherArgs, {
        cwd: root.pathname,
        stdio: ["ignore", "pipe", "pipe"],
      });

      launcherProcess.stdout.on("data", (chunk) => process.stdout.write(chunk));

      return new Promise((resolve, reject) => {
        let ready = false;
        const onStderr = (chunk) => {
          const text = chunk.toString("utf8");
          process.stderr.write(chunk);
          const match = text.match(/Witty launcher listening on (http:\/\/\S+)/);
          if (!ready && match) {
            ready = true;
            resolve(match[1]);
          }
        };
        launcherProcess.stderr.on("data", onStderr);
        launcherProcess.on("exit", (code, signal) => {
          exited = { code, signal };
          if (!ready) {
            reject(new Error(`witty --web exited before listening: ${JSON.stringify(exited)}`));
          }
        });
      });
    },
    async waitForExit(timeoutMs = 5000) {
      const status =
        exited ??
        (await new Promise((resolve, reject) => {
          const timeout = setTimeout(() => {
            reject(
              new Error(`witty --web did not exit within ${timeoutMs} ms after browser close`),
            );
          }, timeoutMs);
          launcherProcess.once("exit", (code, signal) => {
            clearTimeout(timeout);
            resolve({ code, signal });
          });
        }));

      if (status.code !== 0 || status.signal) {
        throw new Error(`witty --web exited unexpectedly: ${JSON.stringify(status)}`);
      }
      return status;
    },
    stop() {
      if (launcherProcess && !launcherProcess.killed && !exited) {
        launcherProcess.kill("SIGTERM");
      }
    },
  };
}

const profilePickerSensitiveValues = [
  "prod.example.com",
  "staging.example.com",
  "vault.example.com",
  "alice",
  "2222",
  "prod_ed25519",
  "/home/alice/.ssh/config",
  "ServerAliveInterval=30",
  "uptime",
  "vault-secret-prod",
];

const profileImportSensitiveValues = [
  "prod.example.com",
  "staging.example.com",
  "old.example.com",
  "alice",
  "2222",
  "prod_ed25519",
  "/home/alice/.ssh/config",
  "ServerAliveInterval=30",
  "uptime",
];
const profileImportConfigText = [
  "Host prod",
  "    HostName prod.example.com",
  "    User alice",
  "    Port 2222",
  "    IdentityFile /home/alice/.ssh/prod_ed25519",
  "    RemoteCommand uptime",
  "Host staging",
  "    HostName staging.example.com",
  "",
].join("\n");
const profileImportProdOnlyConfigText = [
  "Host prod",
  "    HostName prod.example.com",
  "    User alice",
  "    Port 2222",
  "    IdentityFile /home/alice/.ssh/prod_ed25519",
  "    RemoteCommand uptime",
  "",
].join("\n");

async function createProfilePickerFixture() {
  const tempRoot = await mkdtemp(join(tmpdir(), "witty-profile-picker-smoke-"));
  const fakeBin = join(tempRoot, "bin");
  const fakeSshPath = join(fakeBin, "ssh");
  const fakeSshArgsPath = join(tempRoot, "fake-ssh-args.txt");
  const profileStorePath = join(tempRoot, "profiles.v1.json");

  await mkdir(fakeBin);
  await writeFile(
    fakeSshPath,
    [
      "#!/usr/bin/env sh",
      'printf "%s\\n" "$@" > "${WITTY_FAKE_SSH_ARGS}"',
      "stty -echo 2>/dev/null || true",
      'printf "\\033]2;Witty Gateway Smoke\\007shell ready\\r\\n\\033[?1049hWITTY ALT SCREEN SMOKE\\r\\n"',
      'while IFS= read -r line; do printf "\\033[?1049lWITTY MAIN SCREEN RESTORED\\r\\npty saw:%s\\r\\n> " "$line"; done',
      "",
    ].join("\n"),
  );
  await chmod(fakeSshPath, 0o755);

  await writeFile(
    profileStorePath,
    JSON.stringify(
      {
        schema: 1,
        app: "witty-profiles",
        profiles: [
          {
            id: "prod",
            name: "Production",
            description: "Primary production SSH profile",
            tags: ["work", "prod"],
            target: {
              host: "prod.example.com",
              user: "alice",
              port: 2222,
              jump_host: null,
            },
            credential: {
              kind: "identity_file",
              path: "/home/alice/.ssh/prod_ed25519",
            },
            terminal: {
              term: "xterm-256color",
              request_tty: true,
            },
            openssh: {
              config_file: "/home/alice/.ssh/config",
              extra_args: ["-o", "ServerAliveInterval=30"],
              remote_command: ["uptime"],
            },
          },
          {
            id: "vaulted",
            name: "Vaulted",
            description: "Profile requiring a future credential resolver",
            tags: ["work", "vault"],
            target: {
              host: "vault.example.com",
              user: "alice",
              port: null,
              jump_host: null,
            },
            credential: {
              kind: "vault_secret",
              secret_id: "vault-secret-prod",
            },
            terminal: {
              term: "xterm-256color",
              request_tty: true,
            },
            openssh: {
              config_file: null,
              extra_args: [],
              remote_command: [],
            },
          },
          {
            id: "staging",
            name: "Staging",
            description: null,
            tags: ["stage"],
            target: {
              host: "staging.example.com",
              user: null,
              port: null,
              jump_host: null,
            },
            credential: {
              kind: "default_agent",
            },
            terminal: {
              term: "xterm-256color",
              request_tty: true,
            },
            openssh: {
              config_file: null,
              extra_args: [],
              remote_command: [],
            },
          },
        ],
        default_profile_id: "prod",
      },
      null,
      2,
    ),
  );

  return {
    tempRoot,
    fakeBin,
    fakeSshArgsPath,
    profileStorePath,
  };
}

function createProfilePickerLauncherSmokeServer() {
  let launcherProcess = null;
  let exited = null;
  let fixturePromise = null;

  return {
    kind: "profile-picker",
    async start() {
      const fixture = await createProfilePickerFixture();
      fixturePromise = Promise.resolve(fixture);
      const launcherArgs = [
        "run",
        "-p",
        "witty-app",
        "--",
        "--web",
        "--scrollback-lines",
        String(launcherScrollbackLines),
        "--profile-picker",
        "--profile-store",
        fixture.profileStorePath,
      ];
      if (!launcherUseDefaultWebRoot) {
        launcherArgs.splice(5, 0, "--web-root", dist.pathname);
      }

      launcherProcess = spawn("cargo", launcherArgs, {
        cwd: root.pathname,
        env: {
          ...process.env,
          PATH: `${fixture.fakeBin}${delimiter}${process.env.PATH ?? ""}`,
          WITTY_FAKE_SSH_ARGS: fixture.fakeSshArgsPath,
        },
        stdio: ["ignore", "pipe", "pipe"],
      });

      launcherProcess.stdout.on("data", (chunk) => process.stdout.write(chunk));

      return new Promise((resolve, reject) => {
        let ready = false;
        const onStderr = (chunk) => {
          const text = chunk.toString("utf8");
          process.stderr.write(chunk);
          const match = text.match(/Witty profile picker listening on (http:\/\/\S+)/);
          if (!ready && match) {
            ready = true;
            resolve(match[1]);
          }
        };
        launcherProcess.stderr.on("data", onStderr);
        launcherProcess.on("exit", (code, signal) => {
          exited = { code, signal };
          if (!ready) {
            reject(
              new Error(
                `witty --web --profile-picker exited before listening: ${JSON.stringify(exited)}`,
              ),
            );
          }
        });
      });
    },
    async assertFakeSshArgs() {
      const fixture = await fixturePromise;
      const args = (await readFile(fixture.fakeSshArgsPath, "utf8"))
        .split(/\r?\n/)
        .filter(Boolean);
      const expected = [
        "-tt",
        "-p",
        "2222",
        "-i",
        "/home/alice/.ssh/prod_ed25519",
        "-F",
        "/home/alice/.ssh/config",
        "-o",
        "ServerAliveInterval=30",
        "alice@prod.example.com",
        "uptime",
      ];
      if (JSON.stringify(args) !== JSON.stringify(expected)) {
        throw new Error(`fake ssh argv mismatch: ${JSON.stringify(args)}`);
      }
      return true;
    },
    async waitForExit(timeoutMs = 5000) {
      const status =
        exited ??
        (await new Promise((resolve, reject) => {
          const timeout = setTimeout(() => {
            reject(
              new Error(
                `witty --web --profile-picker did not exit within ${timeoutMs} ms after browser close`,
              ),
            );
          }, timeoutMs);
          launcherProcess.once("exit", (code, signal) => {
            clearTimeout(timeout);
            resolve({ code, signal });
          });
        }));

      if (status.code !== 0 || status.signal) {
        throw new Error(
          `witty --web --profile-picker exited unexpectedly: ${JSON.stringify(status)}`,
        );
      }
      return status;
    },
    stop() {
      if (launcherProcess && !launcherProcess.killed && !exited) {
        launcherProcess.kill("SIGTERM");
      }
    },
  };
}

async function createProfileImportFixture() {
  const tempRoot = await mkdtemp(join(tmpdir(), "witty-profile-import-smoke-"));
  const fakeBin = join(tempRoot, "bin");
  const fakeSshPath = join(fakeBin, "ssh");
  const fakeSshArgsPath = join(tempRoot, "fake-ssh-args.txt");
  const configPath = join(tempRoot, "ssh_config");
  const profileStorePath = join(tempRoot, "profiles.v1.json");

  await mkdir(fakeBin);
  await writeFile(
    fakeSshPath,
    [
      "#!/usr/bin/env sh",
      'printf "%s\\n" "$@" > "${WITTY_FAKE_SSH_ARGS}"',
      "stty -echo 2>/dev/null || true",
      'printf "\\033]2;Witty Gateway Smoke\\007shell ready\\r\\n\\033[?1049hWITTY ALT SCREEN SMOKE\\r\\n"',
      'while IFS= read -r line; do printf "\\033[?1049lWITTY MAIN SCREEN RESTORED\\r\\npty saw:%s\\r\\n> " "$line"; done',
      "",
    ].join("\n"),
  );
  await chmod(fakeSshPath, 0o755);

  await writeFile(configPath, profileImportConfigText);
  await writeFile(
    profileStorePath,
    JSON.stringify(
      {
        schema: 1,
        app: "witty-profiles",
        profiles: [
          {
            id: "prod",
            name: "Existing Production",
            description: null,
            tags: ["existing"],
            target: {
              host: "old.example.com",
              user: null,
              port: null,
              jump_host: null,
            },
            credential: {
              kind: "default_agent",
            },
            terminal: {
              term: "xterm-256color",
              request_tty: true,
            },
            openssh: {
              config_file: null,
              extra_args: [],
              remote_command: [],
            },
          },
        ],
        default_profile_id: "prod",
      },
      null,
      2,
    ),
  );

  return {
    tempRoot,
    fakeBin,
    fakeSshArgsPath,
    configPath,
    profileStorePath,
  };
}

function createProfileImportLauncherSmokeServer(conflictFlow = "replace") {
  let launcherProcess = null;
  let exited = null;
  let fixturePromise = null;

  return {
    kind: conflictFlow === "reject" ? "profile-import-reject" : "profile-import",
    async start() {
      const fixture = await createProfileImportFixture();
      fixturePromise = Promise.resolve(fixture);
      const launcherArgs = [
        "run",
        "-p",
        "witty-app",
        "--",
        "--web",
        "--profile-import-openssh",
        fixture.configPath,
        "--profile-store",
        fixture.profileStorePath,
      ];
      if (!launcherUseDefaultWebRoot) {
        launcherArgs.splice(5, 0, "--web-root", dist.pathname);
      }

      launcherProcess = spawn("cargo", launcherArgs, {
        cwd: root.pathname,
        env: {
          ...process.env,
          PATH: `${fixture.fakeBin}${delimiter}${process.env.PATH ?? ""}`,
          WITTY_FAKE_SSH_ARGS: fixture.fakeSshArgsPath,
        },
        stdio: ["ignore", "pipe", "pipe"],
      });

      launcherProcess.stdout.on("data", (chunk) => process.stdout.write(chunk));

      return new Promise((resolve, reject) => {
        let ready = false;
        const onStderr = (chunk) => {
          const text = chunk.toString("utf8");
          process.stderr.write(chunk);
          const match = text.match(/Witty profile import review listening on (http:\/\/\S+)/);
          if (!ready && match) {
            ready = true;
            resolve(match[1]);
          }
        };
        launcherProcess.stderr.on("data", onStderr);
        launcherProcess.on("exit", (code, signal) => {
          exited = { code, signal };
          if (!ready) {
            reject(
              new Error(
                `witty --web --profile-import-openssh exited before listening: ${JSON.stringify(exited)}`,
              ),
            );
          }
        });
      });
    },
    async setImportConfig(text) {
      const fixture = await fixturePromise;
      await writeFile(fixture.configPath, text);
    },
    async assertImportedStore() {
      const fixture = await fixturePromise;
      const store = JSON.parse(await readFile(fixture.profileStorePath, "utf8"));
      const profiles = new Map(store.profiles.map((profile) => [profile.id, profile]));
      if (store.default_profile_id !== "prod") {
        throw new Error(`profile import changed default unexpectedly: ${JSON.stringify(store)}`);
      }
      const expectedProdHost = conflictFlow === "reject" ? "old.example.com" : "prod.example.com";
      if (profiles.get("prod")?.target?.host !== expectedProdHost) {
        throw new Error(`profile import prod host mismatch: ${JSON.stringify(store)}`);
      }
      if (profiles.get("staging")?.target?.host !== "staging.example.com") {
        throw new Error(`profile import did not add staging: ${JSON.stringify(store)}`);
      }
      return {
        profileCount: store.profiles.length,
        defaultProfileId: store.default_profile_id,
        prodHost: profiles.get("prod")?.target?.host,
        stagingHost: profiles.get("staging")?.target?.host,
      };
    },
    async waitForExit(timeoutMs = 5000) {
      const status =
        exited ??
        (await new Promise((resolve, reject) => {
          const timeout = setTimeout(() => {
            reject(
              new Error(
                `witty --web --profile-import-openssh did not exit within ${timeoutMs} ms after confirmation`,
              ),
            );
          }, timeoutMs);
          launcherProcess.once("exit", (code, signal) => {
            clearTimeout(timeout);
            resolve({ code, signal });
          });
        }));

      if (status.code !== 0 || status.signal) {
        throw new Error(
          `witty --web --profile-import-openssh exited unexpectedly: ${JSON.stringify(status)}`,
        );
      }
      return status;
    },
    stop() {
      if (launcherProcess && !launcherProcess.killed && !exited) {
        launcherProcess.kill("SIGTERM");
      }
    },
  };
}

function createProfilePickerImportLauncherSmokeServer() {
  let launcherProcess = null;
  let exited = null;
  let fixturePromise = null;

  return {
    kind: "profile-picker-import",
    async start() {
      const fixture = await createProfileImportFixture();
      fixturePromise = Promise.resolve(fixture);
      const launcherArgs = [
        "run",
        "-p",
        "witty-app",
        "--",
        "--web",
        "--scrollback-lines",
        String(launcherScrollbackLines),
        "--profile-picker",
        "--profile-store",
        fixture.profileStorePath,
        "--profile-picker-import-openssh",
        fixture.configPath,
      ];
      if (!launcherUseDefaultWebRoot) {
        launcherArgs.splice(5, 0, "--web-root", dist.pathname);
      }

      launcherProcess = spawn("cargo", launcherArgs, {
        cwd: root.pathname,
        env: {
          ...process.env,
          PATH: `${fixture.fakeBin}${delimiter}${process.env.PATH ?? ""}`,
          WITTY_FAKE_SSH_ARGS: fixture.fakeSshArgsPath,
        },
        stdio: ["ignore", "pipe", "pipe"],
      });

      launcherProcess.stdout.on("data", (chunk) => process.stdout.write(chunk));

      return new Promise((resolve, reject) => {
        let ready = false;
        const onStderr = (chunk) => {
          const text = chunk.toString("utf8");
          process.stderr.write(chunk);
          const match = text.match(/Witty profile picker listening on (http:\/\/\S+)/);
          if (!ready && match) {
            ready = true;
            resolve(match[1]);
          }
        };
        launcherProcess.stderr.on("data", onStderr);
        launcherProcess.on("exit", (code, signal) => {
          exited = { code, signal };
          if (!ready) {
            reject(
              new Error(
                `witty --web --profile-picker --profile-picker-import-openssh exited before listening: ${JSON.stringify(exited)}`,
              ),
            );
          }
        });
      });
    },
    async setImportConfig(text) {
      const fixture = await fixturePromise;
      await writeFile(fixture.configPath, text);
    },
    async assertImportedStore() {
      const fixture = await fixturePromise;
      const store = JSON.parse(await readFile(fixture.profileStorePath, "utf8"));
      const profiles = new Map(store.profiles.map((profile) => [profile.id, profile]));
      if (store.default_profile_id !== "prod") {
        throw new Error(`profile picker import changed default unexpectedly: ${JSON.stringify(store)}`);
      }
      if (profiles.get("prod")?.target?.host !== "prod.example.com") {
        throw new Error(`profile picker import did not replace prod exactly: ${JSON.stringify(store)}`);
      }
      if (profiles.get("staging")?.target?.host !== "staging.example.com") {
        throw new Error(`profile picker import did not add staging: ${JSON.stringify(store)}`);
      }
      return {
        profileCount: store.profiles.length,
        defaultProfileId: store.default_profile_id,
        prodHost: profiles.get("prod")?.target?.host,
        stagingHost: profiles.get("staging")?.target?.host,
      };
    },
    async assertFakeSshArgs() {
      const fixture = await fixturePromise;
      const args = (await readFile(fixture.fakeSshArgsPath, "utf8"))
        .split(/\r?\n/)
        .filter(Boolean);
      const expected = [
        "-tt",
        "-p",
        "2222",
        "-i",
        "/home/alice/.ssh/prod_ed25519",
        "alice@prod.example.com",
        "uptime",
      ];
      if (JSON.stringify(args) !== JSON.stringify(expected)) {
        throw new Error(`profile picker import fake ssh argv mismatch: ${JSON.stringify(args)}`);
      }
      return true;
    },
    async waitForExit(timeoutMs = 5000) {
      const status =
        exited ??
        (await new Promise((resolve, reject) => {
          const timeout = setTimeout(() => {
            reject(
              new Error(
                `witty --web --profile-picker --profile-picker-import-openssh did not exit within ${timeoutMs} ms after browser close`,
              ),
            );
          }, timeoutMs);
          launcherProcess.once("exit", (code, signal) => {
            clearTimeout(timeout);
            resolve({ code, signal });
          });
        }));

      if (status.code !== 0 || status.signal) {
        throw new Error(
          `witty --web --profile-picker --profile-picker-import-openssh exited unexpectedly: ${JSON.stringify(status)}`,
        );
      }
      return status;
    },
    stop() {
      if (launcherProcess && !launcherProcess.killed && !exited) {
        launcherProcess.kill("SIGTERM");
      }
    },
  };
}

function createGatewaySmokeHarness(mode, listenPort) {
  if (mode === "node") {
    return createGatewaySmokeServer(listenPort);
  }
  if (mode === "rust") {
    return createRustGatewaySmokeServer(listenPort);
  }
  if (mode === "launcher") {
    return createLauncherSmokeServer();
  }
  if (mode === "profile-picker") {
    return createProfilePickerLauncherSmokeServer();
  }
  if (mode === "profile-import") {
    return createProfileImportLauncherSmokeServer();
  }
  if (mode === "profile-import-reject") {
    return createProfileImportLauncherSmokeServer("reject");
  }
  if (mode === "profile-picker-import") {
    return createProfilePickerImportLauncherSmokeServer();
  }
  throw new Error(`unsupported WITTY_WEB_SMOKE_GATEWAY=${mode}`);
}

async function assertLauncherSessionConfigIsStale(launcherUrl) {
  const parsed = new URL(launcherUrl);
  const sessionId = new URLSearchParams(parsed.hash.slice(1)).get("session");
  if (!sessionId) {
    throw new Error(`launcher URL is missing session hash: ${launcherUrl}`);
  }

  const staleConfigUrl = new URL(`/session/${encodeURIComponent(sessionId)}.json`, parsed);
  const response = await fetch(staleConfigUrl, { cache: "no-store" });
  if (response.status !== 410) {
    throw new Error(
      `launcher session config remained readable after browser load: ${response.status}`,
    );
  }
}

async function assertProfilePickerBootstrapIsStale(launcherUrl) {
  const parsed = new URL(launcherUrl);
  const pickerId = new URLSearchParams(parsed.hash.slice(1)).get("profile_picker");
  if (!pickerId) {
    throw new Error(`profile picker URL is missing picker hash: ${launcherUrl}`);
  }

  const staleConfigUrl = new URL(`/profile-picker/${encodeURIComponent(pickerId)}.json`, parsed);
  const response = await fetch(staleConfigUrl, { cache: "no-store" });
  if (response.status !== 410) {
    throw new Error(
      `profile picker bootstrap remained readable after browser load: ${response.status}`,
    );
  }
}

async function assertProfilePickerSelectionIsStale(launcherUrl, selection) {
  const endpoint = new URL(selection.selectionUrl, new URL(launcherUrl));
  const response = await fetch(endpoint, {
    method: "POST",
    cache: "no-store",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      ui_token: selection.uiToken,
      profile_id: selection.profileId,
    }),
  });
  if (response.status !== 409) {
    throw new Error(
      `profile picker selection remained reusable after launch: ${response.status}`,
    );
  }
}

async function assertProfilePickerSelectionRejectsNonJsonContentType(launcherUrl, selection) {
  const endpoint = new URL(selection.selectionUrl, new URL(launcherUrl));
  const response = await fetch(endpoint, {
    method: "POST",
    cache: "no-store",
    headers: {
      "Content-Type": "text/plain",
    },
    body: JSON.stringify({
      ui_token: selection.uiToken,
      profile_id: selection.profileId,
    }),
  });
  const text = await response.text();
  if (response.status !== 415 || !text.includes("application/json")) {
    throw new Error(
      `profile picker selection accepted non-JSON content type: ${response.status} ${text}`,
    );
  }
}

async function assertProfilePickerImportIsStale(launcherUrl, selection) {
  const endpoint = new URL(selection.importUrl, new URL(launcherUrl));
  const response = await fetch(endpoint, {
    method: "POST",
    cache: "no-store",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      ui_token: selection.uiToken,
      action_id: selection.actionId,
    }),
  });
  if (response.status !== 409) {
    throw new Error(
      `profile picker import action remained reusable after import start: ${response.status}`,
    );
  }
}

async function assertProfilePickerImportRejectsNonJsonContentType(launcherUrl, selection) {
  const endpoint = new URL(selection.importUrl, new URL(launcherUrl));
  const response = await fetch(endpoint, {
    method: "POST",
    cache: "no-store",
    headers: {
      "Content-Type": "text/plain",
    },
    body: JSON.stringify({
      ui_token: selection.uiToken,
      action_id: selection.actionId,
    }),
  });
  const text = await response.text();
  if (response.status !== 415 || !text.includes("application/json")) {
    throw new Error(
      `profile picker import action accepted non-JSON content type: ${response.status} ${text}`,
    );
  }
}

async function assertProfileImportBootstrapIsStale(launcherUrl) {
  const parsed = new URL(launcherUrl);
  const importId = new URLSearchParams(parsed.hash.slice(1)).get("profile_import");
  if (!importId) {
    throw new Error(`profile import URL is missing import hash: ${launcherUrl}`);
  }

  const staleConfigUrl = new URL(`/profile-import/${encodeURIComponent(importId)}.json`, parsed);
  const response = await fetch(staleConfigUrl, { cache: "no-store" });
  if (response.status !== 410) {
    throw new Error(
      `profile import bootstrap remained readable after browser load: ${response.status}`,
    );
  }
}

async function assertProfileImportConfirmRejectsNonJsonContentType(launcherUrl, selection) {
  const endpoint = new URL(selection.confirmUrl, new URL(launcherUrl));
  const response = await fetch(endpoint, {
    method: "POST",
    cache: "no-store",
    headers: {
      "Content-Type": "text/plain",
    },
    body: JSON.stringify({
      ui_token: selection.uiToken,
      profile_ids: ["staging"],
      conflict: "reject",
    }),
  });
  const text = await response.text();
  if (response.status !== 415 || !text.includes("application/json")) {
    throw new Error(
      `profile import confirmation accepted non-JSON content type: ${response.status} ${text}`,
    );
  }
}

async function assertProfilePickerExposureIsFrozen(page, expected) {
  const result = await page.evaluate(() => {
    const profiles = window.wittyProfilePickerProfiles;
    const actions = window.wittyProfilePickerImportActions;
    let profileListMutationError = "";
    let profileMutationError = "";
    let tagMutationError = "";
    let actionListMutationError = "";
    let actionMutationError = "";
    try {
      profiles?.push?.({
        id: "spoofed",
        name: "Spoofed",
        tags: ["spoofed"],
        launchability: "launchable",
        isDefault: false,
      });
    } catch (error) {
      profileListMutationError = String(error?.message ?? error);
    }
    try {
      if (profiles?.[0]) {
        profiles[0].launchability = "spoofed";
        profiles[0].isDefault = false;
      }
    } catch (error) {
      profileMutationError = String(error?.message ?? error);
    }
    try {
      profiles?.[0]?.tags?.push?.("spoofed");
    } catch (error) {
      tagMutationError = String(error?.message ?? error);
    }
    try {
      actions?.push?.({
        id: "spoofed",
        kind: "openssh_config",
        label: "Spoofed Import",
      });
    } catch (error) {
      actionListMutationError = String(error?.message ?? error);
    }
    try {
      if (actions?.[0]) {
        actions[0].label = "Spoofed Import";
      }
    } catch (error) {
      actionMutationError = String(error?.message ?? error);
    }
    return {
      profileListMutationError,
      profileMutationError,
      tagMutationError,
      actionListMutationError,
      actionMutationError,
      profilesFrozen: Object.isFrozen(profiles),
      profilesAllFrozen: [...(profiles ?? [])].every((profile) => Object.isFrozen(profile)),
      profileTagsAllFrozen: [...(profiles ?? [])].every((profile) =>
        Object.isFrozen(profile.tags),
      ),
      actionsFrozen: Object.isFrozen(actions),
      actionsAllFrozen: [...(actions ?? [])].every((action) => Object.isFrozen(action)),
      profileIds: [...(profiles ?? [])].map((profile) => profile.id),
      profiles: [...(profiles ?? [])].map((profile) => ({
        id: profile.id,
        launchability: profile.launchability,
        isDefault: profile.isDefault,
        tags: [...profile.tags],
      })),
      actionIds: [...(actions ?? [])].map((action) => action.id),
      actions: [...(actions ?? [])].map((action) => ({
        id: action.id,
        kind: action.kind,
        label: action.label,
      })),
    };
  });

  if (
    result.profilesFrozen !== true ||
    result.profilesAllFrozen !== true ||
    result.profileTagsAllFrozen !== true ||
    result.actionsFrozen !== true ||
    result.actionsAllFrozen !== true ||
    JSON.stringify(result.profileIds) !== JSON.stringify(expected.profileIds) ||
    JSON.stringify(result.actionIds) !== JSON.stringify(expected.actionIds) ||
    result.profiles.some(
      (profile) =>
        profile.launchability === "spoofed" ||
        profile.tags.includes("spoofed") ||
        profile.id === "spoofed",
    ) ||
    result.actions.some(
      (action) => action.id === "spoofed" || action.label === "Spoofed Import",
    )
  ) {
    throw new Error(
      `profile picker exposed summaries were mutable: ${JSON.stringify(result)}`,
    );
  }
}

async function assertProfilePickerBootstrapExposureIsFrozen(page, expected) {
  const result = await page.evaluate(() => {
    const bootstrap = window.wittyProfilePickerBootstrap;
    let mutationError = "";
    try {
      if (bootstrap) {
        bootstrap.selection_url = "/profile-picker/spoofed/select";
        bootstrap.import_url = "/profile-picker/spoofed/import";
        bootstrap.ui_token = "spoofed-token";
      }
    } catch (error) {
      mutationError = String(error?.message ?? error);
    }
    return {
      mutationError,
      frozen: Object.isFrozen(bootstrap),
      kind: bootstrap?.kind ?? "",
      protocol: bootstrap?.protocol ?? 0,
      selectionUrl: bootstrap?.selection_url ?? "",
      importUrl: bootstrap?.import_url ?? "",
      uiToken: bootstrap?.ui_token ?? "",
    };
  });

  if (
    result.frozen !== true ||
    result.kind !== "profile_picker" ||
    result.protocol !== 1 ||
    result.selectionUrl !== expected.selectionUrl ||
    result.importUrl !== expected.importUrl ||
    result.uiToken !== expected.uiToken
  ) {
    throw new Error(
      `profile picker bootstrap exposure was mutable: ${JSON.stringify(result)}`,
    );
  }
}

async function assertProfileImportBootstrapExposureIsFrozen(page, expected) {
  const result = await page.evaluate(() => {
    const bootstrap = window.wittyProfileImportBootstrap;
    let mutationError = "";
    try {
      if (bootstrap) {
        bootstrap.confirm_url = "/profile-import/spoofed/confirm";
        bootstrap.ui_token = "spoofed-token";
      }
    } catch (error) {
      mutationError = String(error?.message ?? error);
    }
    return {
      mutationError,
      frozen: Object.isFrozen(bootstrap),
      kind: bootstrap?.kind ?? "",
      protocol: bootstrap?.protocol ?? 0,
      confirmUrl: bootstrap?.confirm_url ?? "",
      uiToken: bootstrap?.ui_token ?? "",
    };
  });

  if (
    result.frozen !== true ||
    result.kind !== "profile_import" ||
    result.protocol !== 1 ||
    result.confirmUrl !== expected.confirmUrl ||
    result.uiToken !== expected.uiToken
  ) {
    throw new Error(
      `profile import bootstrap exposure was mutable: ${JSON.stringify(result)}`,
    );
  }
}

async function runBrowserProfilePickerReadySmoke(page, launcherUrl) {
  const result = await page.evaluate(() => {
    const buttons = [...document.querySelectorAll(".profile-picker-option")].map((button) => ({
      profileId: button.dataset.profileId,
      disabled: button.disabled,
      text: button.textContent,
    }));
    return {
      smoke: document.documentElement.dataset.wittySmoke ?? "",
      pickerState: window.wittyProfilePickerState ?? "",
      bodyText: document.body.textContent ?? "",
      href: window.location.href,
      profiles: window.wittyProfilePickerProfiles ?? [],
      bootstrap: {
        selectionUrl: window.wittyProfilePickerBootstrap?.selection_url ?? "",
        uiToken: window.wittyProfilePickerBootstrap?.ui_token ?? "",
      },
      buttons,
    };
  });

  if (result.pickerState !== "profile_picker_ready") {
    throw new Error(`profile picker did not reach ready state: ${JSON.stringify(result)}`);
  }
  if (!String(result.href).startsWith(launcherUrl.split("#")[0])) {
    throw new Error(`profile picker navigated unexpectedly: ${JSON.stringify(result.href)}`);
  }
  for (const sensitive of profilePickerSensitiveValues) {
    if (result.bodyText.includes(sensitive) || result.href.includes(sensitive)) {
      throw new Error(`profile picker leaked sensitive value ${sensitive}: ${JSON.stringify(result)}`);
    }
  }

  const profileById = new Map(result.profiles.map((profile) => [profile.id, profile]));
  if (
    profileById.get("prod")?.launchability !== "launchable" ||
    profileById.get("prod")?.isDefault !== true ||
    profileById.get("vaulted")?.launchability !== "requires_credential_resolver" ||
    profileById.get("staging")?.launchability !== "launchable"
  ) {
    throw new Error(`profile picker profile summary mismatch: ${JSON.stringify(result.profiles)}`);
  }
  const buttonById = new Map(result.buttons.map((button) => [button.profileId, button]));
  if (
    buttonById.get("prod")?.disabled ||
    !buttonById.get("vaulted")?.disabled ||
    buttonById.get("staging")?.disabled
  ) {
    throw new Error(`profile picker button state mismatch: ${JSON.stringify(result.buttons)}`);
  }
  if (
    typeof result.bootstrap.selectionUrl !== "string" ||
    !profilePickerSelectionUrlPattern.test(result.bootstrap.selectionUrl) ||
    !launcherTokenPattern.test(result.bootstrap.uiToken)
  ) {
    throw new Error(`profile picker bootstrap helpers missing: ${JSON.stringify(result.bootstrap)}`);
  }
  await assertProfilePickerExposureIsFrozen(page, {
    profileIds: ["prod", "vaulted", "staging"],
    actionIds: [],
  });
  await assertProfilePickerBootstrapExposureIsFrozen(page, {
    selectionUrl: result.bootstrap.selectionUrl,
    importUrl: "",
    uiToken: result.bootstrap.uiToken,
  });

  return {
    selectionUrl: result.bootstrap.selectionUrl,
    uiToken: result.bootstrap.uiToken,
    profileId: "prod",
    profiles: result.profiles.map((profile) => ({
      id: profile.id,
      launchability: profile.launchability,
      isDefault: profile.isDefault,
    })),
  };
}

async function runBrowserProfilePickerImportReadySmoke(page, launcherUrl) {
  const result = await page.evaluate(() => {
    const buttons = [...document.querySelectorAll(".profile-picker-import-action")].map((button) => ({
      actionId: button.dataset.actionId,
      disabled: button.disabled,
      text: button.textContent,
    }));
    return {
      smoke: document.documentElement.dataset.wittySmoke ?? "",
      pickerState: window.wittyProfilePickerState ?? "",
      bodyText: document.body.textContent ?? "",
      href: window.location.href,
      actions: window.wittyProfilePickerImportActions ?? [],
      bootstrap: {
        selectionUrl: window.wittyProfilePickerBootstrap?.selection_url ?? "",
        importUrl: window.wittyProfilePickerBootstrap?.import_url ?? "",
        uiToken: window.wittyProfilePickerBootstrap?.ui_token ?? "",
      },
      buttons,
    };
  });

  if (result.pickerState !== "profile_picker_ready") {
    throw new Error(`profile picker import did not reach picker ready state: ${JSON.stringify(result)}`);
  }
  if (!String(result.href).startsWith(launcherUrl.split("#")[0])) {
    throw new Error(`profile picker import navigated unexpectedly before action: ${JSON.stringify(result.href)}`);
  }
  for (const sensitive of profileImportSensitiveValues) {
    if (result.bodyText.includes(sensitive) || result.href.includes(sensitive)) {
      throw new Error(`profile picker import leaked sensitive value ${sensitive}: ${JSON.stringify(result)}`);
    }
  }

  const action = result.actions.find((candidate) => candidate.id === "openssh-config");
  const button = result.buttons.find((candidate) => candidate.actionId === "openssh-config");
  if (
    action?.kind !== "openssh_config" ||
    action?.label !== "OpenSSH Import" ||
    !button ||
    button.disabled ||
    !button.text.includes("OpenSSH Import")
  ) {
    throw new Error(`profile picker import action mismatch: ${JSON.stringify(result)}`);
  }
  if (
    typeof result.bootstrap.selectionUrl !== "string" ||
    !profilePickerSelectionUrlPattern.test(result.bootstrap.selectionUrl) ||
    typeof result.bootstrap.importUrl !== "string" ||
    !profilePickerImportUrlPattern.test(result.bootstrap.importUrl) ||
    !launcherTokenPattern.test(result.bootstrap.uiToken)
  ) {
    throw new Error(`profile picker import bootstrap helpers missing: ${JSON.stringify(result.bootstrap)}`);
  }
  await assertProfilePickerExposureIsFrozen(page, {
    profileIds: ["prod"],
    actionIds: ["openssh-config"],
  });
  await assertProfilePickerBootstrapExposureIsFrozen(page, {
    selectionUrl: result.bootstrap.selectionUrl,
    importUrl: result.bootstrap.importUrl,
    uiToken: result.bootstrap.uiToken,
  });

  return {
    importUrl: result.bootstrap.importUrl,
    uiToken: result.bootstrap.uiToken,
    actionId: "openssh-config",
    actions: result.actions.map((candidate) => ({
      id: candidate.id,
      kind: candidate.kind,
      label: candidate.label,
    })),
  };
}

async function runBrowserPostImportProfilePickerReadySmoke(page, launcherUrl) {
  const result = await page.evaluate(() => {
    const buttons = [...document.querySelectorAll(".profile-picker-option")].map((button) => ({
      profileId: button.dataset.profileId,
      disabled: button.disabled,
      text: button.textContent,
    }));
    const actionButtons = [...document.querySelectorAll(".profile-picker-import-action")].map((button) => ({
      actionId: button.dataset.actionId,
      disabled: button.disabled,
      text: button.textContent,
    }));
    return {
      smoke: document.documentElement.dataset.wittySmoke ?? "",
      pickerState: window.wittyProfilePickerState ?? "",
      bodyText: document.body.textContent ?? "",
      href: window.location.href,
      profiles: window.wittyProfilePickerProfiles ?? [],
      actions: window.wittyProfilePickerImportActions ?? [],
      bootstrap: {
        selectionUrl: window.wittyProfilePickerBootstrap?.selection_url ?? "",
        importUrl: window.wittyProfilePickerBootstrap?.import_url ?? "",
        uiToken: window.wittyProfilePickerBootstrap?.ui_token ?? "",
      },
      buttons,
      actionButtons,
    };
  });

  if (result.pickerState !== "profile_picker_ready") {
    throw new Error(`post-import profile picker did not reach ready state: ${JSON.stringify(result)}`);
  }
  if (!String(result.href).startsWith(launcherUrl.split("#")[0])) {
    throw new Error(`post-import profile picker navigated unexpectedly: ${JSON.stringify(result.href)}`);
  }
  for (const sensitive of profileImportSensitiveValues) {
    if (result.bodyText.includes(sensitive) || result.href.includes(sensitive)) {
      throw new Error(`post-import profile picker leaked sensitive value ${sensitive}: ${JSON.stringify(result)}`);
    }
  }

  const profileById = new Map(result.profiles.map((profile) => [profile.id, profile]));
  if (
    result.profiles.length !== 2 ||
    profileById.get("prod")?.launchability !== "launchable" ||
    profileById.get("prod")?.isDefault !== true ||
    profileById.get("staging")?.launchability !== "launchable"
  ) {
    throw new Error(`post-import profile picker summary mismatch: ${JSON.stringify(result.profiles)}`);
  }
  const buttonById = new Map(result.buttons.map((button) => [button.profileId, button]));
  if (buttonById.get("prod")?.disabled || buttonById.get("staging")?.disabled) {
    throw new Error(`post-import profile picker button state mismatch: ${JSON.stringify(result.buttons)}`);
  }
  const action = result.actions.find((candidate) => candidate.id === "openssh-config");
  const actionButton = result.actionButtons.find((candidate) => candidate.actionId === "openssh-config");
  if (
    action?.kind !== "openssh_config" ||
    action?.label !== "OpenSSH Import" ||
    !actionButton ||
    actionButton.disabled
  ) {
    throw new Error(`post-import profile picker import action mismatch: ${JSON.stringify(result)}`);
  }
  if (
    typeof result.bootstrap.selectionUrl !== "string" ||
    !profilePickerSelectionUrlPattern.test(result.bootstrap.selectionUrl) ||
    typeof result.bootstrap.importUrl !== "string" ||
    !profilePickerImportUrlPattern.test(result.bootstrap.importUrl) ||
    !launcherTokenPattern.test(result.bootstrap.uiToken)
  ) {
    throw new Error(`post-import profile picker bootstrap helpers missing: ${JSON.stringify(result.bootstrap)}`);
  }
  await assertProfilePickerExposureIsFrozen(page, {
    profileIds: ["prod", "staging"],
    actionIds: ["openssh-config"],
  });
  await assertProfilePickerBootstrapExposureIsFrozen(page, {
    selectionUrl: result.bootstrap.selectionUrl,
    importUrl: result.bootstrap.importUrl,
    uiToken: result.bootstrap.uiToken,
  });

  return {
    selectionUrl: result.bootstrap.selectionUrl,
    uiToken: result.bootstrap.uiToken,
    profileId: "prod",
    profiles: result.profiles.map((profile) => ({
      id: profile.id,
      launchability: profile.launchability,
      isDefault: profile.isDefault,
    })),
    actions: result.actions.map((candidate) => ({
      id: candidate.id,
      kind: candidate.kind,
      label: candidate.label,
    })),
  };
}

async function runBrowserProfileImportReadySmoke(page, launcherUrl) {
  const result = await page.evaluate(() => ({
    smoke: document.documentElement.dataset.wittySmoke ?? "",
    importState: window.wittyProfileImportState ?? "",
    bodyText: document.body.textContent ?? "",
    href: window.location.href,
    candidates: window.wittyProfileImportCandidates ?? [],
    summary: window.wittyProfileImportReviewSummary ?? null,
    conflictPolicy: window.wittyProfileImportConflictPolicy ?? "",
    conflictGroupLabel: document
      .querySelector(".profile-import-conflict")
      ?.getAttribute("aria-label") ?? "",
    hasSetConflictPolicy: typeof window.wittySetProfileImportConflictPolicy === "function",
    conflictControls: [...document.querySelectorAll(".profile-import-conflict-option")].map(
      (button) => ({
        policy: button.dataset.conflictPolicy,
        active: button.classList.contains("is-active"),
        pressed: button.getAttribute("aria-pressed"),
        disabled: button.disabled,
        text: button.textContent,
      }),
    ),
    candidateControls: [
      ...document.querySelectorAll(".profile-import-option input[type='checkbox']"),
    ].map((input) => ({
      profileId: input.dataset.profileId,
      hasConflict: input.dataset.hasConflict,
      checked: input.checked,
      disabled: input.disabled,
    })),
    resultControl: (() => {
      const result = document.querySelector(".profile-import-result");
      return {
        exists: Boolean(result),
        hidden: result?.hidden ?? false,
        label: result?.getAttribute("aria-label") ?? "",
        live: result?.getAttribute("aria-live") ?? "",
      };
    })(),
    bootstrap: {
      confirmUrl: window.wittyProfileImportBootstrap?.confirm_url ?? "",
      uiToken: window.wittyProfileImportBootstrap?.ui_token ?? "",
    },
  }));

  if (result.importState !== "profile_import_ready") {
    throw new Error(`profile import did not reach ready state: ${JSON.stringify(result)}`);
  }
  if (!String(result.href).startsWith(launcherUrl.split("#")[0])) {
    throw new Error(`profile import navigated unexpectedly: ${JSON.stringify(result.href)}`);
  }
  for (const sensitive of profileImportSensitiveValues) {
    if (result.bodyText.includes(sensitive) || result.href.includes(sensitive)) {
      throw new Error(`profile import leaked sensitive value ${sensitive}: ${JSON.stringify(result)}`);
    }
  }

  const candidateById = new Map(result.candidates.map((candidate) => [candidate.id, candidate]));
  if (
    result.candidates.length !== 2 ||
    candidateById.get("prod")?.hasConflict !== true ||
    candidateById.get("prod")?.warningCount < 1 ||
    candidateById.get("staging")?.hasConflict !== false
  ) {
    throw new Error(`profile import candidate summary mismatch: ${JSON.stringify(result.candidates)}`);
  }
  if (
    result.summary?.candidateCount !== 2 ||
    result.summary?.warningCount !== 2 ||
    result.summary?.globalWarningCount !== 0 ||
    result.summary?.conflictCount !== 1
  ) {
    throw new Error(`profile import review summary mismatch: ${JSON.stringify(result.summary)}`);
  }
  const conflictByPolicy = new Map(
    result.conflictControls.map((control) => [control.policy, control]),
  );
  if (
    !result.hasSetConflictPolicy ||
    result.conflictGroupLabel !== "Conflict policy" ||
    result.conflictPolicy !== "reject" ||
    conflictByPolicy.size !== 2 ||
    conflictByPolicy.get("reject")?.active !== true ||
    conflictByPolicy.get("reject")?.pressed !== "true" ||
    conflictByPolicy.get("replace")?.active !== false ||
    conflictByPolicy.get("replace")?.pressed !== "false"
  ) {
    throw new Error(
      `profile import conflict control mismatch: ${JSON.stringify({
        conflictPolicy: result.conflictPolicy,
        conflictGroupLabel: result.conflictGroupLabel,
        hasSetConflictPolicy: result.hasSetConflictPolicy,
        conflictControls: result.conflictControls,
      })}`,
    );
  }
  if (
    !result.resultControl.exists ||
    !result.resultControl.hidden ||
    result.resultControl.label !== "Import result" ||
    result.resultControl.live !== "polite"
  ) {
    throw new Error(`profile import result control mismatch: ${JSON.stringify(result.resultControl)}`);
  }
  const inputById = new Map(
    result.candidateControls.map((control) => [control.profileId, control]),
  );
  if (
    inputById.get("prod")?.hasConflict !== "true" ||
    inputById.get("prod")?.checked !== false ||
    inputById.get("prod")?.disabled !== true ||
    inputById.get("staging")?.hasConflict !== "false" ||
    inputById.get("staging")?.checked !== true ||
    inputById.get("staging")?.disabled !== false
  ) {
    throw new Error(`profile import candidate control mismatch: ${JSON.stringify(result.candidateControls)}`);
  }
  if (
    typeof result.bootstrap.confirmUrl !== "string" ||
    !profileImportConfirmUrlPattern.test(result.bootstrap.confirmUrl) ||
    !launcherTokenPattern.test(result.bootstrap.uiToken)
  ) {
    throw new Error(`profile import bootstrap helpers missing: ${JSON.stringify(result.bootstrap)}`);
  }
  await assertProfileImportBootstrapExposureIsFrozen(page, {
    confirmUrl: result.bootstrap.confirmUrl,
    uiToken: result.bootstrap.uiToken,
  });
  const previewFreezeState = await page.evaluate(() => {
    const candidates = window.wittyProfileImportCandidates;
    const summary = window.wittyProfileImportReviewSummary;
    let candidateListMutationError = "";
    let candidateMutationError = "";
    let tagMutationError = "";
    let summaryMutationError = "";
    try {
      candidates?.push?.({
        id: "spoofed",
        name: "Spoofed",
        tags: ["spoofed"],
        warningCount: 99,
        hasConflict: false,
      });
    } catch (error) {
      candidateListMutationError = String(error?.message ?? error);
    }
    try {
      if (candidates?.[0]) {
        candidates[0].warningCount = 99;
        candidates[0].hasConflict = false;
      }
    } catch (error) {
      candidateMutationError = String(error?.message ?? error);
    }
    try {
      candidates?.[0]?.tags?.push?.("spoofed");
    } catch (error) {
      tagMutationError = String(error?.message ?? error);
    }
    try {
      if (summary) {
        summary.candidateCount = 99;
        summary.warningCount = 98;
        summary.globalWarningCount = 97;
        summary.conflictCount = 96;
      }
    } catch (error) {
      summaryMutationError = String(error?.message ?? error);
    }
    return {
      candidateListMutationError,
      candidateMutationError,
      tagMutationError,
      summaryMutationError,
      candidatesFrozen: Object.isFrozen(candidates),
      firstCandidateFrozen: Object.isFrozen(candidates?.[0]),
      firstTagsFrozen: Object.isFrozen(candidates?.[0]?.tags),
      summaryFrozen: Object.isFrozen(summary),
      candidateCount: candidates?.length ?? 0,
      firstCandidate: candidates?.[0] ?? null,
      summary: summary ?? null,
    };
  });
  if (
    previewFreezeState.candidatesFrozen !== true ||
    previewFreezeState.firstCandidateFrozen !== true ||
    previewFreezeState.firstTagsFrozen !== true ||
    previewFreezeState.summaryFrozen !== true ||
    previewFreezeState.candidateCount !== 2 ||
    previewFreezeState.firstCandidate?.id !== "prod" ||
    previewFreezeState.firstCandidate?.warningCount < 1 ||
    previewFreezeState.firstCandidate?.hasConflict !== true ||
    previewFreezeState.firstCandidate?.tags?.includes("spoofed") ||
    previewFreezeState.summary?.candidateCount !== 2 ||
    previewFreezeState.summary?.warningCount !== 2 ||
    previewFreezeState.summary?.globalWarningCount !== 0 ||
    previewFreezeState.summary?.conflictCount !== 1
  ) {
    throw new Error(
      `profile import preview summaries were mutable: ${JSON.stringify(
        previewFreezeState,
      )}`,
    );
  }
  const spoofedReportState = await page.evaluate(() => {
    const importButton = document.querySelector(".profile-import-confirm");
    const result = document.querySelector(".profile-import-result");
    const nextPickerButton = document.querySelector(".profile-import-next-picker");
    const spoofedReport = {
      selected: 99,
      added: 98,
      replaced: 97,
      warning_count: 96,
      global_warning_count: 95,
      next_picker_url: "/index.html#profile_picker=not-authorized",
    };
    window.wittyProfileImportReport = spoofedReport;
    const attemptedReport =
      typeof window.wittySetProfileImportReport === "function"
        ? window.wittySetProfileImportReport(spoofedReport)
        : "missing";
    const reportWasSpoofed = window.wittyProfileImportReport === spoofedReport;
    window.wittyProfileImportReport = null;
    const attemptedNextPicker =
      typeof window.wittySetProfileImportNextPicker === "function"
        ? window.wittySetProfileImportNextPicker(
            "/index.html#profile_picker=not-authorized",
          )
        : "missing";
    return {
      attemptedReport,
      attemptedNextPicker,
      reportWasSpoofed,
      report: window.wittyProfileImportReport ?? null,
      resultSummary: window.wittyProfileImportResultSummary ?? null,
      resultHidden: result?.hidden ?? false,
      resultText: result?.textContent ?? "",
      importDisabled: importButton?.disabled ?? true,
      nextPickerUrl: window.wittyProfileImportNextPickerUrl ?? "",
      nextPickerHidden: nextPickerButton?.hidden ?? true,
    };
  });
  if (
    spoofedReportState.attemptedReport !== null ||
    spoofedReportState.attemptedNextPicker !== false ||
    spoofedReportState.reportWasSpoofed !== true ||
    spoofedReportState.report !== null ||
    spoofedReportState.resultSummary !== null ||
    spoofedReportState.resultHidden !== true ||
    spoofedReportState.resultText !== "" ||
    spoofedReportState.importDisabled !== false ||
    spoofedReportState.nextPickerUrl !== "" ||
    spoofedReportState.nextPickerHidden !== true
  ) {
    throw new Error(
      `profile import accepted spoofed report helper state: ${JSON.stringify(
        spoofedReportState,
      )}`,
    );
  }

  return {
    confirmUrl: result.bootstrap.confirmUrl,
    uiToken: result.bootstrap.uiToken,
    candidates: result.candidates.map((candidate) => ({
      id: candidate.id,
      warningCount: candidate.warningCount,
      hasConflict: candidate.hasConflict,
    })),
  };
}

function indexUrlWithHash(launcherUrl, hash) {
  return new URL(`/index.html#${hash}`, launcherUrl).toString();
}

function profilePickerBootstrapFixture(pickerId, token = fakeLauncherToken("7")) {
  return {
    kind: "profile_picker",
    protocol: 1,
    ui_token: token,
    selection_url: `/profile-picker/${pickerId}/select`,
    expires_at_ms: 180000,
    summary: {
      profiles: [
        {
          id: "prod",
          name: "Prod",
          tags: ["fake"],
          launchability: "launchable",
          is_default: true,
        },
        {
          id: "vaulted",
          name: "Vaulted",
          tags: ["vault"],
          launchability: "requires_credential_resolver",
          is_default: false,
        },
      ],
      default_profile_id: "prod",
      launchable_profiles: 1,
      credential_resolver_required_profiles: 1,
    },
    import_actions: [],
  };
}

function profileImportBootstrapFixture(importId, token = fakeLauncherToken("8")) {
  return {
    kind: "profile_import",
    protocol: 1,
    ui_token: token,
    confirm_url: `/profile-import/${importId}/confirm`,
    expires_at_ms: 180000,
    review: {
      candidates: [
        {
          id: "staging",
          name: "Staging",
          tags: ["fake"],
          warning_count: 1,
          has_conflict: false,
        },
      ],
      selected_by_default: ["staging"],
      warning_count: 1,
      global_warning_count: 0,
      conflict_count: 0,
    },
  };
}

async function runMalformedBootstrapPageSmoke(
  browser,
  launcherUrl,
  hash,
  routePattern,
  responseBody,
  expectedStatusText,
) {
  const smokePage = await browser.newPage({ viewport: { width: 1000, height: 620 } });
  const pageEvents = [];
  smokePage.on("console", (message) => {
    pageEvents.push(`[console:${message.type()}] ${message.text()}`);
  });
  smokePage.on("pageerror", (error) => {
    pageEvents.push(`[pageerror] ${error.stack ?? error.message}`);
  });

  try {
    await smokePage.route(routePattern, (playwrightRoute) =>
      playwrightRoute.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(responseBody),
      }),
    );
    await smokePage.goto(indexUrlWithHash(launcherUrl, hash));
    await smokePage.waitForFunction(
      () => document.documentElement.dataset.wittySmoke === "failed",
      { timeout: 30000 },
    );
    const result = await smokePage.evaluate(() => ({
      smoke: document.documentElement.dataset.wittySmoke ?? "",
      status: document.getElementById("status")?.textContent ?? "",
      pickerState: window.wittyProfilePickerState ?? "",
      pickerBootstrap: window.wittyProfilePickerBootstrap ?? null,
      importState: window.wittyProfileImportState ?? "",
      importBootstrap: window.wittyProfileImportBootstrap ?? null,
      pickerActive: document.body.dataset.wittyPicker ?? "",
      importActive: document.body.dataset.wittyImport ?? "",
    }));
    if (
      result.smoke !== "failed" ||
      !result.status.includes(expectedStatusText) ||
      result.pickerState === "profile_picker_ready" ||
      result.importState === "profile_import_ready" ||
      result.pickerBootstrap !== null ||
      result.importBootstrap !== null ||
      result.pickerActive === "active" ||
      result.importActive === "active"
    ) {
      throw new Error(`malformed bootstrap was accepted: ${JSON.stringify(result)}`);
    }
    return result;
  } catch (error) {
    const pageState = await smokePage.evaluate(() => ({
      smoke: document.documentElement.dataset.wittySmoke ?? "",
      status: document.getElementById("status")?.textContent ?? "",
      pickerState: window.wittyProfilePickerState ?? "",
      importState: window.wittyProfileImportState ?? "",
    }));
    throw new Error(
      `${error.message}\nmalformed-bootstrap page state: ${JSON.stringify(pageState)}\n${pageEvents.join("\n")}`,
    );
  } finally {
    await smokePage.close();
  }
}

async function runMalformedOkPageSmoke(browser, launcherUrl, hash, routes, evaluateState) {
  const smokePage = await browser.newPage({ viewport: { width: 1000, height: 620 } });
  const pageEvents = [];
  smokePage.on("console", (message) => {
    pageEvents.push(`[console:${message.type()}] ${message.text()}`);
  });
  smokePage.on("pageerror", (error) => {
    pageEvents.push(`[pageerror] ${error.stack ?? error.message}`);
  });

  try {
    for (const route of routes) {
      await smokePage.route(route.pattern, (playwrightRoute) =>
        playwrightRoute.fulfill(route.response),
      );
    }
    await smokePage.goto(indexUrlWithHash(launcherUrl, hash));
    const expectedState = hash.startsWith("profile_import=")
      ? "profile_import_ready"
      : "profile_picker_ready";
    await smokePage.waitForFunction(
      (state) => document.documentElement.dataset.wittySmoke === state,
      expectedState,
      { timeout: 30000 },
    );
    return await evaluateState(smokePage);
  } catch (error) {
    const pageState = await smokePage.evaluate(() => ({
      smoke: document.documentElement.dataset.wittySmoke ?? "",
      status: document.getElementById("status")?.textContent ?? "",
      pickerState: window.wittyProfilePickerState ?? "",
      pickerError: window.wittyProfilePickerLastError ?? null,
      importState: window.wittyProfileImportState ?? "",
      importError: window.wittyProfileImportLastError ?? null,
    }));
    throw new Error(
      `${error.message}\nmalformed-ok page state: ${JSON.stringify(pageState)}\n${pageEvents.join("\n")}`,
    );
  } finally {
    await smokePage.close();
  }
}

async function runBrowserMalformedLauncherHashSmoke(browser, launcherUrl) {
  const smokePage = await browser.newPage({ viewport: { width: 1000, height: 620 } });
  try {
    await smokePage.goto(
      indexUrlWithHash(
        launcherUrl,
        "profile_picker=0123456789abcdef0123456789abcdef&profile_import=bad",
      ),
    );
    await smokePage.waitForFunction(
      () => document.documentElement.dataset.wittySmoke === "failed",
      { timeout: 30000 },
    );
    const result = await smokePage.evaluate(() => ({
      smoke: document.documentElement.dataset.wittySmoke ?? "",
      status: document.getElementById("status")?.textContent ?? "",
      href: window.location.href,
    }));
    if (
      result.smoke !== "failed" ||
      !result.status.includes("launcher hash is invalid for profile_picker") ||
      result.href.includes("token=")
    ) {
      throw new Error(`malformed launcher hash was accepted: ${JSON.stringify(result)}`);
    }
    return result;
  } finally {
    await smokePage.close();
  }
}

async function runBrowserMalformedLauncherSessionConfigSmoke(browser, launcherUrl) {
  const cases = [
    {
      id: "05050505050505050505050505050505",
      expected: "session config has unsupported field profile_store_path",
      mutate(config) {
        config.profile_store_path = "/home/user/.config/witty/profiles.v1.json";
      },
    },
    {
      id: "06060606060606060606060606060606",
      expected: "session config has invalid expires_at_ms",
      mutate(config) {
        config.expires_at_ms = "180000";
      },
    },
  ];

  const results = [];
  for (const testCase of cases) {
    const config = launcherSessionConfigFixture();
    testCase.mutate(config);
    results.push(
      await runMalformedBootstrapPageSmoke(
        browser,
        launcherUrl,
        `session=${testCase.id}`,
        `**/session/${testCase.id}.json`,
        config,
        testCase.expected,
      ),
    );
  }
  return results;
}

async function runBrowserMalformedProfilePickerBootstrapSmoke(browser, launcherUrl) {
  const cases = [
    {
      id: "01010101010101010101010101010101",
      expected: "profile picker bootstrap has unsupported field config_path",
      mutate(bootstrap) {
        bootstrap.config_path = "/home/user/.ssh/config";
      },
    },
    {
      id: "02020202020202020202020202020202",
      expected: "profile picker profile has unsupported field host",
      mutate(bootstrap) {
        bootstrap.summary.profiles[0].host = "prod.internal";
      },
    },
    {
      id: "66666666666666666666666666666666",
      expected: "profile picker profile has an invalid tag",
      mutate(bootstrap) {
        bootstrap.summary.profiles[0].tags = ["fake", 7];
      },
    },
    {
      id: "77777777777777777777777777777777",
      expected: "profile picker summary launchable count mismatch",
      mutate(bootstrap) {
        bootstrap.summary.launchable_profiles = 2;
      },
    },
    {
      id: "88888888888888888888888888888888",
      expected: "profile picker profile has invalid default flag",
      mutate(bootstrap) {
        bootstrap.summary.profiles[0].is_default = "true";
      },
    },
    {
      id: "99999999999999999999999999999999",
      expected: "profile picker summary default id mismatch",
      mutate(bootstrap) {
        bootstrap.summary.default_profile_id = "vaulted";
      },
    },
    {
      id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      expected: "profile picker bootstrap has import_url without import actions",
      mutate(bootstrap) {
        bootstrap.import_url = `/profile-picker/${bootstrap.selection_url.split("/")[2]}/import`;
      },
    },
  ];

  const results = [];
  for (const testCase of cases) {
    const bootstrap = profilePickerBootstrapFixture(testCase.id);
    testCase.mutate(bootstrap);
    results.push(
      await runMalformedBootstrapPageSmoke(
        browser,
        launcherUrl,
        `profile_picker=${testCase.id}`,
        `**/profile-picker/${testCase.id}.json`,
        bootstrap,
        testCase.expected,
      ),
    );
  }
  return results;
}

async function runBrowserMalformedProfileImportBootstrapSmoke(browser, launcherUrl) {
  const cases = [
    {
      id: "03030303030303030303030303030303",
      expected: "profile import bootstrap has unsupported field store_path",
      mutate(bootstrap) {
        bootstrap.store_path = "/home/user/.config/witty/profiles.json";
      },
    },
    {
      id: "04040404040404040404040404040404",
      expected: "profile import candidate has unsupported field host",
      mutate(bootstrap) {
        bootstrap.review.candidates[0].host = "staging.internal";
      },
    },
    {
      id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      expected: "profile import candidate has an invalid tag",
      mutate(bootstrap) {
        bootstrap.review.candidates[0].tags = ["fake", false];
      },
    },
    {
      id: "cccccccccccccccccccccccccccccccc",
      expected: "profile import review warning count mismatch",
      mutate(bootstrap) {
        bootstrap.review.warning_count = 2;
      },
    },
    {
      id: "dddddddddddddddddddddddddddddddd",
      expected: "profile import review conflict count mismatch",
      mutate(bootstrap) {
        bootstrap.review.conflict_count = 1;
      },
    },
    {
      id: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
      expected: "profile import default selection contains unknown or duplicate ids",
      mutate(bootstrap) {
        bootstrap.review.selected_by_default = ["missing"];
      },
    },
    {
      id: "ffffffffffffffffffffffffffffffff",
      expected: "profile import default selection contains a conflict",
      mutate(bootstrap) {
        bootstrap.review.candidates[0].has_conflict = true;
        bootstrap.review.conflict_count = 1;
      },
    },
  ];

  const results = [];
  for (const testCase of cases) {
    const bootstrap = profileImportBootstrapFixture(testCase.id);
    testCase.mutate(bootstrap);
    results.push(
      await runMalformedBootstrapPageSmoke(
        browser,
        launcherUrl,
        `profile_import=${testCase.id}`,
        `**/profile-import/${testCase.id}.json`,
        bootstrap,
        testCase.expected,
      ),
    );
  }
  return results;
}

async function runBrowserMalformedPickerSelectionOkSmoke(browser, launcherUrl) {
  const pickerId = "11111111111111111111111111111111";
  const token = fakeLauncherToken("1");
  const evaluatePickerSelectionFailure = (smokePage) =>
    smokePage.evaluate(async () => {
      const attempted = await window.wittySelectProfile("prod");
      return {
        attempted,
        smoke: document.documentElement.dataset.wittySmoke ?? "",
        pickerState: window.wittyProfilePickerState ?? "",
        pickerError: window.wittyProfilePickerLastError ?? null,
        pickerToken: window.wittyProfilePickerBootstrap?.ui_token ?? null,
        launchButtonDisabled:
          document.querySelector(".profile-picker-option")?.disabled ?? false,
      };
    });
  const result = await runMalformedOkPageSmoke(
    browser,
    launcherUrl,
    `profile_picker=${pickerId}`,
    [
      {
        pattern: `**/profile-picker/${pickerId}.json`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            kind: "profile_picker",
            protocol: 1,
            ui_token: token,
            selection_url: `/profile-picker/${pickerId}/select`,
            expires_at_ms: 180000,
            summary: {
              profiles: [
                {
                  id: "prod",
                  name: "Prod",
                  tags: ["fake"],
                  launchability: "launchable",
                  is_default: true,
                },
              ],
              default_profile_id: "prod",
              launchable_profiles: 1,
              credential_resolver_required_profiles: 0,
            },
            import_actions: [],
          }),
        },
      },
      {
        pattern: `**/profile-picker/${pickerId}/select`,
        response: {
          status: 200,
          contentType: "application/json",
          body: "{",
        },
      },
    ],
    evaluatePickerSelectionFailure,
  );

  if (
    result.attempted !== false ||
    result.smoke !== "profile_picker_error" ||
    result.pickerState !== "profile_picker_error" ||
    result.pickerError?.status !== 0 ||
    result.pickerToken !== "" ||
    result.launchButtonDisabled !== true
  ) {
    throw new Error(
      `malformed 200 profile picker selection did not consume token: ${JSON.stringify(result)}`,
    );
  }

  const invalidGatewayPickerId = "55555555555555555555555555555555";
  const invalidGatewayResult = await runMalformedOkPageSmoke(
    browser,
    launcherUrl,
    `profile_picker=${invalidGatewayPickerId}`,
    [
      {
        pattern: `**/profile-picker/${invalidGatewayPickerId}.json`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            kind: "profile_picker",
            protocol: 1,
            ui_token: fakeLauncherToken("2"),
            selection_url: `/profile-picker/${invalidGatewayPickerId}/select`,
            expires_at_ms: 180000,
            summary: {
              profiles: [
                {
                  id: "prod",
                  name: "Prod",
                  tags: ["fake"],
                  launchability: "launchable",
                  is_default: true,
                },
              ],
              default_profile_id: "prod",
              launchable_profiles: 1,
              credential_resolver_required_profiles: 0,
            },
            import_actions: [],
          }),
        },
      },
      {
        pattern: `**/profile-picker/${invalidGatewayPickerId}/select`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            protocol: 1,
            gateway_url: "ws://example.com/witty",
            token: fakeLauncherToken("3"),
            mouse_selection_override: "shift-select",
            scrollback_lines: 64,
            expires_at_ms: 180000,
          }),
        },
      },
    ],
    evaluatePickerSelectionFailure,
  );

  if (
    invalidGatewayResult.attempted !== false ||
    invalidGatewayResult.smoke !== "profile_picker_error" ||
    invalidGatewayResult.pickerState !== "profile_picker_error" ||
    invalidGatewayResult.pickerError?.status !== 0 ||
    invalidGatewayResult.pickerToken !== "" ||
    invalidGatewayResult.launchButtonDisabled !== true
  ) {
    throw new Error(
      `malformed profile picker gateway URL was accepted: ${JSON.stringify(
        invalidGatewayResult,
      )}`,
    );
  }

  const extraFieldPickerId = "70707070707070707070707070707070";
  const extraFieldResult = await runMalformedOkPageSmoke(
    browser,
    launcherUrl,
    `profile_picker=${extraFieldPickerId}`,
    [
      {
        pattern: `**/profile-picker/${extraFieldPickerId}.json`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            kind: "profile_picker",
            protocol: 1,
            ui_token: fakeLauncherToken("b"),
            selection_url: `/profile-picker/${extraFieldPickerId}/select`,
            expires_at_ms: 180000,
            summary: {
              profiles: [
                {
                  id: "prod",
                  name: "Prod",
                  tags: ["fake"],
                  launchability: "launchable",
                  is_default: true,
                },
              ],
              default_profile_id: "prod",
              launchable_profiles: 1,
              credential_resolver_required_profiles: 0,
            },
            import_actions: [],
          }),
        },
      },
      {
        pattern: `**/profile-picker/${extraFieldPickerId}/select`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            ...launcherSessionConfigFixture(fakeLauncherToken("c")),
            ssh_profile: { id: "prod", host: "prod.internal" },
          }),
        },
      },
    ],
    evaluatePickerSelectionFailure,
  );

  if (
    extraFieldResult.attempted !== false ||
    extraFieldResult.smoke !== "profile_picker_error" ||
    extraFieldResult.pickerState !== "profile_picker_error" ||
    extraFieldResult.pickerError?.status !== 0 ||
    !extraFieldResult.pickerError?.message?.includes(
      "session config has unsupported field ssh_profile",
    ) ||
    extraFieldResult.pickerToken !== "" ||
    extraFieldResult.launchButtonDisabled !== true
  ) {
    throw new Error(
      `profile picker selection accepted extra session config fields: ${JSON.stringify(
        extraFieldResult,
      )}`,
    );
  }
}

async function runBrowserMalformedPickerImportOkSmoke(browser, launcherUrl) {
  const pickerId = "22222222222222222222222222222222";
  const token = fakeLauncherToken("4");
  const evaluatePickerImportFailure = (smokePage) =>
    smokePage.evaluate(async () => {
      const attempted = await window.wittyStartProfileImport("openssh-config");
      return {
        attempted,
        href: window.location.href,
        smoke: document.documentElement.dataset.wittySmoke ?? "",
        pickerState: window.wittyProfilePickerState ?? "",
        pickerError: window.wittyProfilePickerLastError ?? null,
        pickerToken: window.wittyProfilePickerBootstrap?.ui_token ?? null,
        importEntry: window.wittyProfilePickerImportEntry ?? null,
        importButtonDisabled:
          document.querySelector(".profile-picker-import-action")?.disabled ?? false,
      };
    });
  const result = await runMalformedOkPageSmoke(
    browser,
    launcherUrl,
    `profile_picker=${pickerId}`,
    [
      {
        pattern: `**/profile-picker/${pickerId}.json`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            kind: "profile_picker",
            protocol: 1,
            ui_token: token,
            selection_url: `/profile-picker/${pickerId}/select`,
            import_url: `/profile-picker/${pickerId}/import`,
            expires_at_ms: 180000,
            summary: {
              profiles: [
                {
                  id: "prod",
                  name: "Prod",
                  tags: ["fake"],
                  launchability: "launchable",
                  is_default: true,
                },
              ],
              default_profile_id: "prod",
              launchable_profiles: 1,
              credential_resolver_required_profiles: 0,
            },
            import_actions: [
              {
                id: "openssh-config",
                kind: "openssh_config",
                label: "OpenSSH Import",
              },
            ],
          }),
        },
      },
      {
        pattern: `**/profile-picker/${pickerId}/import`,
        response: {
          status: 200,
          contentType: "application/json",
          body: "{",
        },
      },
    ],
    evaluatePickerImportFailure,
  );

  if (
    result.attempted !== false ||
    result.smoke !== "profile_picker_error" ||
    result.pickerState !== "profile_picker_error" ||
    result.pickerError?.status !== 0 ||
    result.pickerToken !== "" ||
    result.importEntry !== null ||
    result.importButtonDisabled !== true
  ) {
    throw new Error(
      `malformed 200 picker import entry did not consume token: ${JSON.stringify(result)}`,
    );
  }

  const invalidUrlPickerId = "33333333333333333333333333333333";
  const invalidUrlToken = fakeLauncherToken("5");
  const invalidUrlResult = await runMalformedOkPageSmoke(
    browser,
    launcherUrl,
    `profile_picker=${invalidUrlPickerId}`,
    [
      {
        pattern: `**/profile-picker/${invalidUrlPickerId}.json`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            kind: "profile_picker",
            protocol: 1,
            ui_token: invalidUrlToken,
            selection_url: `/profile-picker/${invalidUrlPickerId}/select`,
            import_url: `/profile-picker/${invalidUrlPickerId}/import`,
            expires_at_ms: 180000,
            summary: {
              profiles: [
                {
                  id: "prod",
                  name: "Prod",
                  tags: ["fake"],
                  launchability: "launchable",
                  is_default: true,
                },
              ],
              default_profile_id: "prod",
              launchable_profiles: 1,
              credential_resolver_required_profiles: 0,
            },
            import_actions: [
              {
                id: "openssh-config",
                kind: "openssh_config",
                label: "OpenSSH Import",
              },
            ],
          }),
        },
      },
      {
        pattern: `**/profile-picker/${invalidUrlPickerId}/import`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            kind: "profile_import_entry",
            protocol: 1,
            import_url: "/index.html#profile_import=not-authorized",
          }),
        },
      },
    ],
    evaluatePickerImportFailure,
  );

  if (
    invalidUrlResult.attempted !== false ||
    invalidUrlResult.smoke !== "profile_picker_error" ||
    invalidUrlResult.pickerState !== "profile_picker_error" ||
    invalidUrlResult.pickerError?.status !== 0 ||
    invalidUrlResult.pickerToken !== "" ||
    invalidUrlResult.importEntry !== null ||
    invalidUrlResult.importButtonDisabled !== true ||
    invalidUrlResult.href.includes("not-authorized")
  ) {
    throw new Error(
      `malformed picker import URL was accepted: ${JSON.stringify(invalidUrlResult)}`,
    );
  }

  const extraFieldPickerId = "12121212121212121212121212121212";
  const extraFieldToken = fakeLauncherToken("d");
  const extraFieldResult = await runMalformedOkPageSmoke(
    browser,
    launcherUrl,
    `profile_picker=${extraFieldPickerId}`,
    [
      {
        pattern: `**/profile-picker/${extraFieldPickerId}.json`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            kind: "profile_picker",
            protocol: 1,
            ui_token: extraFieldToken,
            selection_url: `/profile-picker/${extraFieldPickerId}/select`,
            import_url: `/profile-picker/${extraFieldPickerId}/import`,
            expires_at_ms: 180000,
            summary: {
              profiles: [
                {
                  id: "prod",
                  name: "Prod",
                  tags: ["fake"],
                  launchability: "launchable",
                  is_default: true,
                },
              ],
              default_profile_id: "prod",
              launchable_profiles: 1,
              credential_resolver_required_profiles: 0,
            },
            import_actions: [
              {
                id: "openssh-config",
                kind: "openssh_config",
                label: "OpenSSH Import",
              },
            ],
          }),
        },
      },
      {
        pattern: `**/profile-picker/${extraFieldPickerId}/import`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            kind: "profile_import_entry",
            protocol: 1,
            import_url: "/index.html#profile_import=56565656565656565656565656565656",
            config_path: "/home/user/.ssh/config",
          }),
        },
      },
    ],
    evaluatePickerImportFailure,
  );

  if (
    extraFieldResult.attempted !== false ||
    extraFieldResult.smoke !== "profile_picker_error" ||
    extraFieldResult.pickerState !== "profile_picker_error" ||
    extraFieldResult.pickerError?.status !== 0 ||
    !extraFieldResult.pickerError?.message?.includes(
      "profile picker import entry has unsupported field config_path",
    ) ||
    extraFieldResult.pickerToken !== "" ||
    extraFieldResult.importEntry !== null ||
    extraFieldResult.importButtonDisabled !== true ||
    extraFieldResult.href.includes("56565656565656565656565656565656")
  ) {
    throw new Error(
      `profile picker import entry accepted extra fields: ${JSON.stringify(
        extraFieldResult,
      )}`,
    );
  }
}

async function runBrowserMalformedProfileImportOkSmoke(browser, launcherUrl) {
  const importId = "44444444444444444444444444444444";
  const token = fakeLauncherToken("6");
  const evaluateImportConfirmationFailure = (smokePage) =>
    smokePage.evaluate(async () => {
      const attempted = await window.wittyConfirmProfileImport(["staging"], "reject");
      return {
        attempted,
        smoke: document.documentElement.dataset.wittySmoke ?? "",
        importState: window.wittyProfileImportState ?? "",
        importError: window.wittyProfileImportLastError ?? null,
        importToken: window.wittyProfileImportBootstrap?.ui_token ?? null,
        report: window.wittyProfileImportReport ?? null,
        importButtonDisabled:
          document.querySelector(".profile-import-confirm")?.disabled ?? false,
        candidateDisabled:
          document.querySelector(".profile-import-option input")?.disabled ?? false,
        conflictDisabled:
          document.querySelector(".profile-import-conflict-option")?.disabled ?? false,
      };
    });
  const result = await runMalformedOkPageSmoke(
    browser,
    launcherUrl,
    `profile_import=${importId}`,
    [
      {
        pattern: `**/profile-import/${importId}.json`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            kind: "profile_import",
            protocol: 1,
            ui_token: token,
            confirm_url: `/profile-import/${importId}/confirm`,
            expires_at_ms: 180000,
            review: {
              candidates: [
                {
                  id: "staging",
                  name: "Staging",
                  tags: ["fake"],
                  warning_count: 0,
                  has_conflict: false,
                },
              ],
              selected_by_default: ["staging"],
              warning_count: 0,
              global_warning_count: 0,
              conflict_count: 0,
            },
          }),
        },
      },
      {
        pattern: `**/profile-import/${importId}/confirm`,
        response: {
          status: 200,
          contentType: "application/json",
          body: "{",
        },
      },
    ],
    evaluateImportConfirmationFailure,
  );

  if (
    result.attempted !== null ||
    result.smoke !== "profile_import_error" ||
    result.importState !== "profile_import_error" ||
    result.importError?.status !== 0 ||
    result.importToken !== "" ||
    result.report !== null ||
    result.importButtonDisabled !== true ||
    result.candidateDisabled !== true ||
    result.conflictDisabled !== true
  ) {
    throw new Error(
      `malformed 200 profile import confirmation did not consume token: ${JSON.stringify(result)}`,
    );
  }

  const invalidReportId = "abababababababababababababababab";
  const invalidReportResult = await runMalformedOkPageSmoke(
    browser,
    launcherUrl,
    `profile_import=${invalidReportId}`,
    [
      {
        pattern: `**/profile-import/${invalidReportId}.json`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify(profileImportBootstrapFixture(invalidReportId, fakeLauncherToken("9"))),
        },
      },
      {
        pattern: `**/profile-import/${invalidReportId}/confirm`,
        response: {
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            changed: true,
            profiles: 2,
            default_changed: false,
            bytes: 128,
            created_parent_dir: false,
            selected: "1",
            added: 1,
            replaced: 0,
            warning_count: 0,
            global_warning_count: 0,
          }),
        },
      },
    ],
    evaluateImportConfirmationFailure,
  );

  if (
    invalidReportResult.attempted !== null ||
    invalidReportResult.smoke !== "profile_import_error" ||
    invalidReportResult.importState !== "profile_import_error" ||
    invalidReportResult.importError?.status !== 0 ||
    !invalidReportResult.importError?.message?.includes(
      "profile import report has invalid selected",
    ) ||
    invalidReportResult.importToken !== "" ||
    invalidReportResult.report !== null ||
    invalidReportResult.importButtonDisabled !== true ||
    invalidReportResult.candidateDisabled !== true ||
    invalidReportResult.conflictDisabled !== true
  ) {
    throw new Error(
      `malformed profile import report fields were accepted: ${JSON.stringify(
        invalidReportResult,
      )}`,
    );
  }
}

async function runBrowserScrollbackConfigSmoke(page, expectedLines) {
  const result = await page.evaluate(() => {
    if (!window.wittySession) {
      throw new Error("witty session is missing");
    }
    if (!window.wittyScrollbackLines) {
      throw new Error("witty scrollback config helper is missing");
    }
    return {
      configured: window.wittyScrollbackLines(),
      sessionLimit: window.wittySession.max_scrollback_lines(),
      retained: window.wittySession.scrollback_line_count(),
    };
  });

  if (result.configured !== expectedLines || result.sessionLimit !== expectedLines) {
    throw new Error(
      `scrollback config mismatch: ${JSON.stringify(result)}, expected ${expectedLines}`,
    );
  }
  if (result.retained > expectedLines) {
    throw new Error(`scrollback retained more rows than configured: ${JSON.stringify(result)}`);
  }
  return result;
}

async function runBrowserFrameStatsSmoke(page) {
  const result = await page.evaluate(() => {
    if (!window.wittySession) {
      throw new Error("witty session is missing");
    }
    if (typeof window.wittyFrameStats !== "function") {
      throw new Error("witty frame stats helper is missing");
    }
    return window.wittyFrameStats();
  });

  if (!Number.isSafeInteger(result.maxGlyphRunChars)) {
    throw new Error(`frame stats missing max glyph run: ${JSON.stringify(result)}`);
  }
  if (!Number.isSafeInteger(result.glyphPrepareBatches)) {
    throw new Error(`frame stats missing glyph prepare batch count: ${JSON.stringify(result)}`);
  }
  if (!Number.isSafeInteger(result.rectVertexCapacity)) {
    throw new Error(`frame stats missing rect vertex capacity: ${JSON.stringify(result)}`);
  }
  for (const key of [
    "rendererTextBuffersReused",
    "rendererTextBuffersRebuilt",
    "rendererTextBuffersRetired",
    "rendererTextBufferCount",
    "rendererTextRendererCount",
    "rendererRectVertexCapacity",
    "rendererCpuPrepareUs",
    "rendererTextBufferSyncUs",
    "rendererGlyphPrepareUs",
    "rendererRectVertexSyncUs",
  ]) {
    if (!Number.isSafeInteger(result[key])) {
      throw new Error(`frame stats missing renderer cache field ${key}: ${JSON.stringify(result)}`);
    }
  }
  if (result.maxGlyphRunChars > browserMaxGlyphRunChars) {
    throw new Error(
      `frame glyph run exceeded browser budget: ${JSON.stringify(result)}, budget ${browserMaxGlyphRunChars}`,
    );
  }
  if (
    result.glyphRuns > 0 &&
    (result.glyphPrepareBatches < 1 || result.glyphPrepareBatches > result.glyphRuns)
  ) {
    throw new Error(`frame glyph prepare batch count is invalid: ${JSON.stringify(result)}`);
  }
  if (result.rectVertices > 0 && result.rectVertexCapacity < result.rectVertices) {
    throw new Error(`frame rect vertex capacity is too small: ${JSON.stringify(result)}`);
  }
  if (result.rendererTextBufferCount !== result.glyphRuns) {
    throw new Error(`renderer text buffer count does not match glyph runs: ${JSON.stringify(result)}`);
  }
  if (
    result.rendererTextBuffersReused + result.rendererTextBuffersRebuilt !==
    result.glyphRuns
  ) {
    throw new Error(`renderer text buffer sync counts are inconsistent: ${JSON.stringify(result)}`);
  }
  if (result.rendererTextRendererCount < result.glyphPrepareBatches) {
    throw new Error(`renderer text renderer pool is too small: ${JSON.stringify(result)}`);
  }
  if (
    result.rectVertices > 0 &&
    result.rendererRectVertexCapacity < result.rectVertices
  ) {
    throw new Error(`renderer rect vertex capacity is too small: ${JSON.stringify(result)}`);
  }
  if (
    result.rendererCpuPrepareUs <
    result.rendererTextBufferSyncUs +
      result.rendererGlyphPrepareUs +
      result.rendererRectVertexSyncUs
  ) {
    throw new Error(`renderer CPU timing totals are inconsistent: ${JSON.stringify(result)}`);
  }
  if (result.glyphChars < result.maxGlyphRunChars || result.glyphRuns < 1) {
    throw new Error(`frame stats are internally inconsistent: ${JSON.stringify(result)}`);
  }
  return result;
}

async function runBrowserSearchSmoke(page) {
  const result = await page.evaluate(async () => {
    const canvas = document.getElementById("witty-canvas");
    if (!window.wittySession || !canvas) {
      throw new Error("witty session or canvas is missing");
    }
    if (!window.wittyPushGatewayOutput) {
      throw new Error("witty gateway output helper is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);
    const step = async (label, action) => {
      try {
        return await action();
      } catch (error) {
        throw new Error(`search smoke step ${label} failed: ${String(error?.stack ?? error)}`);
      }
    };
    const dispatchKey = (key, options = {}) => {
      const event = new KeyboardEvent("keydown", {
        key,
        code: options.code ?? key,
        ctrlKey: options.ctrlKey ?? false,
        shiftKey: options.shiftKey ?? false,
        altKey: options.altKey ?? false,
        metaKey: options.metaKey ?? false,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };

    const query = "e";
    await window.wittyGatewayIdle?.();
    await step("clear-selection", () => window.wittySession.clear_selection());
    const frameCountBefore = inputFrames().length;
    const openPrevented = await step("open", () =>
      dispatchKey("F", {
        code: "KeyF",
        ctrlKey: true,
        shiftKey: true,
      }),
    );
    for (const ch of query) {
      await step(`type-${ch}`, () => dispatchKey(ch, { code: `Key${ch.toUpperCase()}` }));
    }
    const afterQuery = { ...window.wittyLastSearch };
    const nextPrevented = await step("next", () => dispatchKey("Enter", { code: "Enter" }));
    const afterNext = { ...window.wittyLastSearch };
    const copyPrevented = await step("consume-copy-shortcut", () =>
      dispatchKey("C", {
        code: "KeyC",
        ctrlKey: true,
        shiftKey: true,
      }),
    );
    const afterCopyShortcut = { ...window.wittyLastSearch };
    const regexPrevented = await step("toggle-regex", () =>
      dispatchKey("r", {
        code: "KeyR",
        altKey: true,
      }),
    );
    const afterRegex = { ...window.wittyLastSearch };
    const invalidBackspacePrevented = await step("invalid-backspace", () =>
      dispatchKey("Backspace", { code: "Backspace" }),
    );
    const invalidTextPrevented = await step("invalid-regex-query", () =>
      dispatchKey("[", { code: "BracketLeft" }),
    );
    const afterInvalidRegex = { ...window.wittyLastSearch };
    const literalPrevented = await step("toggle-literal", () =>
      dispatchKey("r", {
        code: "KeyR",
        altKey: true,
      }),
    );
    const casePrevented = await step("toggle-case", () =>
      dispatchKey("c", {
        code: "KeyC",
        altKey: true,
      }),
    );
    const wholeWordPrevented = await step("toggle-whole-word", () =>
      dispatchKey("w", {
        code: "KeyW",
        altKey: true,
      }),
    );
    const normalizePrevented = await step("toggle-normalize-nfc", () =>
      dispatchKey("n", {
        code: "KeyN",
        altKey: true,
      }),
    );
    const afterOptions = { ...window.wittyLastSearch };
    const closePrevented = await step("close", () => dispatchKey("Escape", { code: "Escape" }));
    const afterClose = { ...window.wittyLastSearch };

    return {
      openPrevented,
      nextPrevented,
      copyPrevented,
      regexPrevented,
      invalidBackspacePrevented,
      invalidTextPrevented,
      literalPrevented,
      casePrevented,
      wholeWordPrevented,
      normalizePrevented,
      closePrevented,
      afterQuery,
      afterNext,
      afterCopyShortcut,
      afterRegex,
      afterInvalidRegex,
      afterOptions,
      afterClose,
      inputBytes: inputFrames().slice(frameCountBefore),
    };
  });

  if (
    !result.openPrevented ||
    !result.nextPrevented ||
    !result.copyPrevented ||
    !result.regexPrevented ||
    !result.invalidBackspacePrevented ||
    !result.invalidTextPrevented ||
    !result.literalPrevented ||
    !result.casePrevented ||
    !result.wholeWordPrevented ||
    !result.normalizePrevented ||
    !result.closePrevented
  ) {
    throw new Error(`search smoke key events were not prevented: ${JSON.stringify(result)}`);
  }
  if (
    !result.afterQuery.open ||
    result.afterQuery.query !== "e" ||
    result.afterQuery.matchCount < 2 ||
    result.afterQuery.activeIndex !== 0 ||
    result.afterQuery.visibleHighlights < 1 ||
    !result.afterQuery.activeVisible
  ) {
    throw new Error(`search query state mismatch: ${JSON.stringify(result.afterQuery)}`);
  }
  if (
    !result.afterNext.open ||
    result.afterNext.activeIndex !== 1 ||
    result.afterNext.matchCount < 2 ||
    result.afterNext.visibleHighlights < 1 ||
    !result.afterNext.activeVisible
  ) {
    throw new Error(`search next state mismatch: ${JSON.stringify(result.afterNext)}`);
  }
  if (
    result.afterCopyShortcut.query !== "e" ||
    result.afterCopyShortcut.activeIndex !== 1
  ) {
    throw new Error(`search did not consume clipboard shortcut while open: ${JSON.stringify(result.afterCopyShortcut)}`);
  }
  if (
    !result.afterRegex.open ||
    !result.afterRegex.regex ||
    result.afterRegex.error !== "" ||
    result.afterRegex.matchCount < 2
  ) {
    throw new Error(`search regex toggle state mismatch: ${JSON.stringify(result.afterRegex)}`);
  }
  if (
    !result.afterInvalidRegex.open ||
    result.afterInvalidRegex.query !== "[" ||
    !result.afterInvalidRegex.regex ||
    !result.afterInvalidRegex.error.includes("invalid regex") ||
    result.afterInvalidRegex.matchCount !== 0 ||
    result.afterInvalidRegex.visibleHighlights !== 0
  ) {
    throw new Error(`search invalid regex state mismatch: ${JSON.stringify(result.afterInvalidRegex)}`);
  }
  if (
    !result.afterOptions.open ||
    result.afterOptions.regex ||
    !result.afterOptions.caseSensitive ||
    !result.afterOptions.wholeWord ||
    !result.afterOptions.normalizeNfc ||
    result.afterOptions.error !== "" ||
    !result.afterOptions.status.includes("[Aa lit word nfc]")
  ) {
    throw new Error(`search option toggle state mismatch: ${JSON.stringify(result.afterOptions)}`);
  }
  if (
    result.afterClose.open ||
    result.afterClose.status !== "closed" ||
    result.afterClose.matchCount !== 0 ||
    result.afterClose.visibleHighlights !== 0 ||
    result.afterClose.error !== ""
  ) {
    throw new Error(`search close state mismatch: ${JSON.stringify(result.afterClose)}`);
  }
  if (result.inputBytes.length !== 0) {
    throw new Error(`search smoke sent terminal input bytes: ${JSON.stringify(result.inputBytes)}`);
  }

  return {
    query: result.afterQuery.query,
    matchCount: result.afterQuery.matchCount,
    afterQuery: result.afterQuery.status,
    afterNext: result.afterNext.status,
    invalidRegex: result.afterInvalidRegex.status,
    options: result.afterOptions.status,
  };
}

async function runBrowserCommandPaletteSmoke(page, gatewayKind) {
  const exerciseVisibleWindowing = true;
  const exerciseFullFlow = true;
  const result = await page.evaluate(async ({ exerciseVisibleWindowing, exerciseFullFlow }) => {
    const canvas = document.getElementById("witty-canvas");
    if (!window.wittySession || !canvas) {
      throw new Error("witty session or canvas is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);
    const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
    const withSessionRetry = async (label, action) => {
      let lastError = null;
      for (let attempt = 0; attempt < 20; attempt += 1) {
        try {
          return action();
        } catch (error) {
          const message = String(error?.message ?? error);
          if (!message.includes("recursive use of an object detected")) {
            throw error;
          }
          lastError = error;
          await settle();
          await window.wittyGatewayIdle?.();
        }
      }
      throw new Error(`${label} kept hitting wasm session borrow retry: ${String(lastError?.message ?? lastError)}`);
    };
    const dispatchKey = (key, options = {}) => {
      const event = new KeyboardEvent("keydown", {
        key,
        code: options.code ?? key,
        ctrlKey: options.ctrlKey ?? false,
        shiftKey: options.shiftKey ?? false,
        altKey: options.altKey ?? false,
        metaKey: options.metaKey ?? false,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };

    await window.wittyGatewayIdle?.();
    const frameCountBefore = inputFrames().length;
    const openPrevented = dispatchKey("P", {
      code: "KeyP",
      ctrlKey: true,
      shiftKey: true,
    });
    let pageDownPrevented = true;
    let afterPageDown = null;
    if (exerciseVisibleWindowing) {
      pageDownPrevented = dispatchKey("PageDown", { code: "PageDown" });
      afterPageDown = { ...window.wittyLastCommandPalette };
    }
    for (const ch of "sea") {
      dispatchKey(ch, { code: `Key${ch.toUpperCase()}` });
    }
    const afterFilter = { ...window.wittyLastCommandPalette };
    if (!exerciseFullFlow) {
      const closePrevented = dispatchKey("Escape", { code: "Escape" });
      const afterClose = { ...window.wittyLastCommandPalette };
      return {
        skippedFullFlow: true,
        openPrevented,
        pageDownPrevented,
        enterPrevented: true,
        reopenPrevented: true,
        closePrevented,
        imePreeditChanged: true,
        imeCommitted: true,
        afterPageDown,
        afterFilter,
        afterClose,
        inputBytes: inputFrames().slice(frameCountBefore),
      };
    }
    const imePreeditChanged = await withSessionRetry("command palette IME preedit", () =>
      window.wittySetImePreedit("zhong", 5, 5),
    );
    const afterImePreedit = {
      palette: { ...window.wittyLastCommandPalette },
      ime: { ...window.wittyLastIme },
    };
    const imeCommitted = await withSessionRetry("command palette IME commit", () =>
      window.wittyCommitImeText("中"),
    );
    const afterImeCommit = {
      palette: { ...window.wittyLastCommandPalette },
      ime: { ...window.wittyLastIme },
    };
    for (let index = 0; index < 16 && window.wittyLastCommandPalette.query !== ""; index += 1) {
      dispatchKey("Backspace", { code: "Backspace" });
    }
    for (const ch of "sea") {
      dispatchKey(ch, { code: `Key${ch.toUpperCase()}` });
    }
    const beforeConfirm = { ...window.wittyLastCommandPalette };
    const enterPrevented = dispatchKey("Enter", { code: "Enter" });
    await settle();
    await window.wittyGatewayIdle?.();
    const afterConfirm = {
      palette: { ...window.wittyLastCommandPalette },
      invocation: { ...window.wittyLastCommandPaletteInvocation },
      search: { ...window.wittyLastSearch },
    };
    const reopenPrevented = dispatchKey("P", {
      code: "KeyP",
      ctrlKey: true,
      shiftKey: true,
    });
    const afterReopen = {
      palette: { ...window.wittyLastCommandPalette },
      search: { ...window.wittyLastSearch },
    };
    const closePrevented = dispatchKey("Escape", { code: "Escape" });
    const afterClose = { ...window.wittyLastCommandPalette };

    return {
      openPrevented,
      pageDownPrevented,
      enterPrevented,
      reopenPrevented,
      closePrevented,
      imePreeditChanged,
      imeCommitted,
      afterPageDown,
      afterFilter,
      afterImePreedit,
      afterImeCommit,
      beforeConfirm,
      afterConfirm,
      afterReopen,
      afterClose,
      inputBytes: inputFrames().slice(frameCountBefore),
    };
  }, { exerciseVisibleWindowing, exerciseFullFlow });

  if (
    !result.openPrevented ||
    !result.pageDownPrevented ||
    !result.enterPrevented ||
    !result.reopenPrevented ||
    !result.closePrevented
  ) {
    throw new Error(`command palette key events were not prevented: ${JSON.stringify(result)}`);
  }
  if (
    exerciseVisibleWindowing &&
    (!result.afterPageDown.open ||
      result.afterPageDown.query !== "" ||
      result.afterPageDown.filteredCount !== 14 ||
      result.afterPageDown.selectedIndex !== 5 ||
      result.afterPageDown.selectedId !== "web.echo" ||
      result.afterPageDown.visibleItems.length !== 3 ||
      result.afterPageDown.visibleItems[0]?.filteredIndex !== 3 ||
      result.afterPageDown.visibleItems[2]?.id !== "web.echo" ||
      !result.afterPageDown.visibleItems[2]?.selected ||
      !result.afterPageDown.status.includes("6/14"))
  ) {
    throw new Error(`command palette page-down window mismatch: ${JSON.stringify(result.afterPageDown)}`);
  }
  if (
    !result.afterFilter.open ||
    result.afterFilter.query !== "sea" ||
    result.afterFilter.filteredCount !== 4 ||
    result.afterFilter.selectedIndex !== 0 ||
    result.afterFilter.selectedId !== "witty.search.open" ||
    result.afterFilter.visibleItems.length !== 3 ||
    !result.afterFilter.visibleItems.some((item) => item.selected && item.id === "witty.search.open")
  ) {
    throw new Error(`command palette filter state mismatch: ${JSON.stringify(result.afterFilter)}`);
  }
  if (
    !result.skippedFullFlow &&
    (!result.imePreeditChanged ||
      !result.afterImePreedit.palette.open ||
      !result.afterImePreedit.palette.status.includes("sea") ||
      result.afterImePreedit.ime.target !== "palette" ||
      result.afterImePreedit.ime.cursorRect?.target !== "palette")
  ) {
    throw new Error(`command palette IME preedit mismatch: ${JSON.stringify(result.afterImePreedit)}`);
  }
  if (
    !result.skippedFullFlow &&
    (!result.imeCommitted ||
      result.afterImeCommit.palette.query !== "sea中" ||
      result.afterImeCommit.ime.active ||
      result.afterImeCommit.ime.preedit !== "")
  ) {
    throw new Error(`command palette IME commit mismatch: ${JSON.stringify(result.afterImeCommit)}`);
  }
  if (
    !result.skippedFullFlow &&
    (result.beforeConfirm.query !== "sea" ||
      result.beforeConfirm.selectedId !== "witty.search.open")
  ) {
    throw new Error(`command palette confirm pre-state mismatch: ${JSON.stringify(result.beforeConfirm)}`);
  }
  if (
    !result.skippedFullFlow &&
    (result.afterConfirm.palette.open ||
      result.afterConfirm.invocation.commandId !== "witty.search.open" ||
      !result.afterConfirm.search.open)
  ) {
    throw new Error(`command palette confirm did not invoke local search: ${JSON.stringify(result.afterConfirm)}`);
  }
  if (
    (!result.skippedFullFlow &&
      (!result.afterReopen.palette.open ||
        result.afterReopen.search.open)) ||
    result.afterClose.open
  ) {
    throw new Error(`command palette reopen/close state mismatch: ${JSON.stringify(result)}`);
  }
  if (result.inputBytes.length !== 0) {
    throw new Error(`command palette smoke sent terminal input bytes: ${JSON.stringify(result.inputBytes)}`);
  }

  return {
    filteredCount: result.afterFilter.filteredCount,
    pageDownSelectedId: result.afterPageDown?.selectedId ?? "skipped-non-node-gateway",
    selectedId: result.afterFilter.selectedId,
    imeQuery: result.afterImeCommit?.palette?.query ?? "skipped-non-node-gateway",
    invoked: result.afterConfirm?.invocation?.commandId ?? "skipped-non-node-gateway",
  };
}

async function runBrowserCommandShortcutSmoke(page) {
  const expectedEchoBytes = Array.from(Buffer.from("echo from web\n", "utf8"));
  const result = await page.evaluate(async () => {
    const canvas = document.getElementById("witty-canvas");
    if (!window.wittySession || !canvas) {
      throw new Error("witty session or canvas is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);
    const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
    const dispatchKey = (key, options = {}) => {
      const event = new KeyboardEvent("keydown", {
        key,
        code: options.code ?? key,
        ctrlKey: options.ctrlKey ?? false,
        shiftKey: options.shiftKey ?? false,
        altKey: options.altKey ?? false,
        metaKey: options.metaKey ?? false,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };

    await window.wittyGatewayIdle?.();
    const frameCountBefore = inputFrames().length;
    const openForF1Prevented = dispatchKey("P", {
      code: "KeyP",
      ctrlKey: true,
      shiftKey: true,
    });
    const f1Prevented = dispatchKey("F1");
    await settle();
    await window.wittyGatewayIdle?.();
    const afterF1 = { ...window.wittyLastCommandShortcutInvocation };
    const screenAfterF1 = window.wittySession.screen_text();
    const framesAfterF1 = inputFrames().slice(frameCountBefore);

    const openForF2Prevented = dispatchKey("P", {
      code: "KeyP",
      ctrlKey: true,
      shiftKey: true,
    });
    const f2Prevented = dispatchKey("F2");
    await settle();
    await window.wittyGatewayIdle?.();
    const afterF2 = { ...window.wittyLastCommandShortcutInvocation };
    const framesAfterF2 = inputFrames().slice(frameCountBefore);

    return {
      f1Prevented,
      f2Prevented,
      openForF1Prevented,
      openForF2Prevented,
      afterF1,
      afterF2,
      screenAfterF1,
      framesAfterF1,
      framesAfterF2,
    };
  });

  if (
    !result.openForF1Prevented ||
    !result.f1Prevented ||
    result.afterF1.commandId !== "witty.about" ||
    !result.screenAfterF1.includes("Witty Rust/wgpu browser prototype") ||
    result.framesAfterF1.length !== 0
  ) {
    throw new Error(`F1 command shortcut mismatch: ${JSON.stringify(result)}`);
  }
  if (
    !result.openForF2Prevented ||
    !result.f2Prevented ||
    result.afterF2.commandId !== "web.echo" ||
    JSON.stringify(result.framesAfterF2) !== JSON.stringify([expectedEchoBytes])
  ) {
    throw new Error(`F2 command shortcut mismatch: ${JSON.stringify(result)}`);
  }

  return {
    f1: result.afterF1.commandId,
    f2: result.afterF2.commandId,
    f2Bytes: result.framesAfterF2[0],
  };
}

async function runBrowserImeSmoke(page) {
  const result = await page.evaluate(async () => {
    const session = window.wittySession;
    const input = window.wittyImeInput;
    if (!session || !input) {
      throw new Error("witty session or IME input is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);
    const readSearchState = () => ({ ...window.wittyLastSearch });
    const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
    const frameCountBefore = inputFrames().length;
    input.focus({ preventScroll: true });

    const dispatchComposition = (type, data = "") => {
      const event = new CompositionEvent(type, {
        data,
        bubbles: true,
        cancelable: true,
      });
      input.dispatchEvent(event);
      return event.defaultPrevented;
    };
    const withSessionRetry = async (label, action) => {
      let lastError = null;
      for (let attempt = 0; attempt < 20; attempt += 1) {
        try {
          return action();
        } catch (error) {
          const message = String(error?.message ?? error);
          if (!message.includes("recursive use of an object detected")) {
            throw error;
          }
          lastError = error;
          await settle();
          await window.wittyGatewayIdle?.();
        }
      }
      throw new Error(`${label} kept hitting wasm session borrow retry: ${String(lastError?.message ?? lastError)}`);
    };
    const dispatchComposingKey = () => {
      const event = new KeyboardEvent("keydown", {
        key: "n",
        code: "KeyN",
        isComposing: true,
        bubbles: true,
        cancelable: true,
      });
      input.dispatchEvent(event);
      return event.defaultPrevented;
    };
    const dispatchBeforeInput = (data) => {
      const event = new InputEvent("beforeinput", {
        data,
        inputType: "insertCompositionText",
        bubbles: true,
        cancelable: true,
      });
      input.dispatchEvent(event);
      return event.defaultPrevented;
    };
    const dispatchKey = (key, options = {}) => {
      const event = new KeyboardEvent("keydown", {
        key,
        code: options.code ?? key,
        ctrlKey: options.ctrlKey ?? false,
        shiftKey: options.shiftKey ?? false,
        altKey: options.altKey ?? false,
        metaKey: options.metaKey ?? false,
        bubbles: true,
        cancelable: true,
      });
      input.dispatchEvent(event);
      return event.defaultPrevented;
    };

    await window.wittyGatewayIdle?.();
    const initialIme = window.wittyLastImeCursorRect
      ? {
          ...window.wittyLastIme,
          source: "cached-diagnostic",
          cursorRect: { ...window.wittyLastImeCursorRect },
        }
      : await withSessionRetry("initial IME diagnostics", () =>
          window.wittyImeDiagnostics(),
        );
    const searchOpenPrevented = dispatchKey("F", {
      code: "KeyF",
      ctrlKey: true,
      shiftKey: true,
    });
    for (let index = 0; index < 64 && readSearchState().query !== ""; index += 1) {
      dispatchKey("Backspace", { code: "Backspace" });
    }
    await settle();
    await window.wittyGatewayIdle?.();
    const searchFrameCountBefore = inputFrames().length;
    dispatchComposition("compositionstart", "");
    dispatchComposition("compositionupdate", "zhong");
    await settle();
    await window.wittyGatewayIdle?.();
    const searchPreeditChanged = !!window.wittyLastIme?.changed;
    const searchImeAfterPreedit = { ...window.wittyLastIme };
    const searchAfterPreedit = readSearchState();
    const searchFramesAfterPreedit = inputFrames().slice(searchFrameCountBefore);
    dispatchComposition("compositionend", "中");
    await settle();
    await window.wittyGatewayIdle?.();
    const searchCommitted = !!window.wittyLastIme?.committed;
    const searchAfterCommit = readSearchState();
    const searchFramesAfterCommit = inputFrames().slice(searchFrameCountBefore);
    const searchClosePrevented = dispatchKey("Escape", { code: "Escape" });
    const searchAfterClose = readSearchState();

    dispatchComposition("compositionstart", "");
    const composingKeyPrevented = dispatchComposingKey();
    dispatchComposition("compositionupdate", "ni");
    const afterPreedit = { ...window.wittyLastIme };
    const framesAfterPreedit = inputFrames().slice(frameCountBefore);
    dispatchComposition("compositionend", "你");
    const afterCommit = { ...window.wittyLastIme };
    const framesAfterCommit = inputFrames().slice(frameCountBefore);
    const duplicatePrevented = dispatchBeforeInput("你");
    await settle();
    const afterDuplicate = { ...window.wittyLastIme };
    const framesAfterDuplicate = inputFrames().slice(frameCountBefore);

    return {
      composingKeyPrevented,
      duplicatePrevented,
      afterPreedit,
      afterCommit,
      afterDuplicate,
      framesAfterPreedit,
      framesAfterCommit,
      framesAfterDuplicate,
      searchPreeditChanged,
      searchCommitted,
      searchAfterPreedit,
      searchAfterCommit,
      searchAfterClose,
      searchFramesAfterPreedit,
      searchFramesAfterCommit,
      searchOpenPrevented,
      searchClosePrevented,
      initialIme,
      searchImeAfterPreedit,
      inputValue: input.value,
    };
  });

  const expectedCommit = Array.from(Buffer.from("你", "utf8"));
  if (
    !result.searchOpenPrevented ||
    !result.searchPreeditChanged ||
    !result.searchAfterPreedit.open ||
    result.searchAfterPreedit.query !== "" ||
    !result.searchAfterPreedit.status.includes("Find: zhong") ||
    result.searchImeAfterPreedit.target !== "search" ||
    result.searchImeAfterPreedit.cursorRect?.target !== "search" ||
    result.searchFramesAfterPreedit.length !== 0
  ) {
    throw new Error(`search IME preedit state mismatch: ${JSON.stringify(result)}`);
  }
  if (
    !result.searchCommitted ||
    !result.searchAfterCommit.open ||
    result.searchAfterCommit.query !== "中" ||
    !result.searchAfterCommit.status.includes("Find: 中") ||
    result.searchFramesAfterCommit.length !== 0
  ) {
    throw new Error(`search IME commit state mismatch: ${JSON.stringify(result)}`);
  }
  if (
    !result.searchClosePrevented ||
    result.searchAfterClose.open ||
    result.searchAfterClose.query !== ""
  ) {
    throw new Error(`search IME close state mismatch: ${JSON.stringify(result)}`);
  }
  if (!result.composingKeyPrevented || !result.duplicatePrevented) {
    throw new Error(`IME smoke did not suppress composing events: ${JSON.stringify(result)}`);
  }
  if (
    !result.afterPreedit.active ||
    result.afterPreedit.preedit !== "ni" ||
    result.afterPreedit.target !== "terminal" ||
    result.afterPreedit.inputMode !== "text" ||
    result.afterPreedit.cursorRect?.target !== "terminal" ||
    result.framesAfterPreedit.length !== 0
  ) {
    throw new Error(`IME preedit state mismatch: ${JSON.stringify(result)}`);
  }
  if (
    result.initialIme.cursorRect?.target !== "terminal" ||
    !Number.isFinite(result.initialIme.cursorRect?.left) ||
    !Number.isFinite(result.initialIme.cursorRect?.top)
  ) {
    throw new Error(`IME diagnostics missing stable cursor rect: ${JSON.stringify(result)}`);
  }
  if (
    result.afterCommit.active ||
    result.afterCommit.preedit !== "" ||
    !result.afterCommit.committed ||
    result.afterCommit.commitText !== "你" ||
    JSON.stringify(result.framesAfterCommit) !== JSON.stringify([expectedCommit])
  ) {
    throw new Error(`IME commit state mismatch: ${JSON.stringify(result)}`);
  }
  if (
    JSON.stringify(result.framesAfterDuplicate) !== JSON.stringify([expectedCommit]) ||
    result.inputValue !== ""
  ) {
    throw new Error(`IME duplicate suppression mismatch: ${JSON.stringify(result)}`);
  }

  return {
    searchQuery: result.searchAfterCommit.query,
    preedit: result.afterPreedit.preedit,
    commitBytes: result.framesAfterCommit.at(-1),
    duplicatePrevented: result.duplicatePrevented,
  };
}

async function runBrowserKeypadSmoke(page) {
  const result = await page.evaluate(async () => {
    const session = window.wittySession;
    const canvas = document.getElementById("witty-canvas");
    if (!session || !canvas) {
      throw new Error("witty session or canvas is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    const frameCountBefore = inputFrames().length;
    const pushModeOutput = (bytes) => {
      if (!window.wittyPushGatewayOutput) {
        throw new Error("witty gateway output helper is missing");
      }
      return window.wittyPushGatewayOutput(bytes);
    };
    const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
    const dispatchKey = (key, code, location) => {
      const event = new KeyboardEvent("keydown", {
        key,
        code,
        location,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };

    await pushModeOutput([27, 61]);
    const topRowPrevented = dispatchKey("1", "Digit1", 0);
    const keypadApplicationPrevented = dispatchKey("1", "Numpad1", 3);
    await settle();
    await pushModeOutput([27, 62]);
    const keypadNormalPrevented = dispatchKey("1", "Numpad1", 3);

    return {
      prevented: [
        topRowPrevented,
        keypadApplicationPrevented,
        keypadNormalPrevented,
      ],
      inputBytes: inputFrames().slice(frameCountBefore),
    };
  });

  const expected = [[49], [27, 79, 113], [49]];
  if (JSON.stringify(result.inputBytes) !== JSON.stringify(expected)) {
    throw new Error(
      `keypad runtime smoke sent ${JSON.stringify(result.inputBytes)}, expected ${JSON.stringify(expected)}`,
    );
  }
  if (!result.prevented.every(Boolean)) {
    throw new Error(`keypad runtime smoke key events were not all handled: ${JSON.stringify(result)}`);
  }

  return result.inputBytes;
}

async function runBrowserFunctionKeySmoke(page) {
  const result = await page.evaluate(async () => {
    const session = window.wittySession;
    const canvas = document.getElementById("witty-canvas");
    if (!session || !canvas) {
      throw new Error("witty session or canvas is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    const frameCountBefore = inputFrames().length;
    const pushModeOutput = (bytes) => {
      if (!window.wittyPushGatewayOutput) {
        throw new Error("witty gateway output helper is missing");
      }
      return window.wittyPushGatewayOutput(bytes);
    };
    const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
    const dispatchKey = (key, options = {}) => {
      const event = new KeyboardEvent("keydown", {
        key,
        code: options.code ?? key,
        location: options.location ?? 0,
        shiftKey: options.shiftKey ?? false,
        altKey: options.altKey ?? false,
        ctrlKey: options.ctrlKey ?? false,
        metaKey: options.metaKey ?? false,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };

    const prevented = [];
    prevented.push(dispatchKey("Home"));
    await settle();
    await pushModeOutput([27, 91, 63, 49, 104]);
    prevented.push(dispatchKey("Home"));
    await settle();
    await pushModeOutput([27, 91, 63, 49, 108]);
    prevented.push(dispatchKey("Insert"));
    prevented.push(dispatchKey("F1"));
    prevented.push(dispatchKey("F5"));
    prevented.push(dispatchKey("ArrowUp", { shiftKey: true }));
    prevented.push(dispatchKey("ArrowLeft", { ctrlKey: true }));
    prevented.push(dispatchKey("Home", { altKey: true }));
    prevented.push(dispatchKey("F1", { shiftKey: true }));
    prevented.push(dispatchKey("F5", { ctrlKey: true }));

    return {
      prevented,
      inputBytes: inputFrames().slice(frameCountBefore),
    };
  });

  const expected = [
    [27, 91, 72],
    [27, 79, 72],
    [27, 91, 50, 126],
    [27, 79, 80],
    [27, 91, 49, 53, 126],
    [27, 91, 49, 59, 50, 65],
    [27, 91, 49, 59, 53, 68],
    [27, 91, 49, 59, 51, 72],
    [27, 91, 49, 59, 50, 80],
    [27, 91, 49, 53, 59, 53, 126],
  ];
  if (JSON.stringify(result.inputBytes) !== JSON.stringify(expected)) {
    throw new Error(
      `function-key runtime smoke sent ${JSON.stringify(result.inputBytes)}, expected ${JSON.stringify(expected)}`,
    );
  }
  if (!result.prevented.every(Boolean)) {
    throw new Error(
      `function-key runtime smoke key events were not all handled: ${JSON.stringify(result)}`,
    );
  }

  return result.inputBytes;
}

async function runBrowserTerminalQueryReplySmoke(page) {
  const result = await page.evaluate(async () => {
    if (!window.wittySession) {
      throw new Error("witty session is missing");
    }
    if (!window.wittyPushGatewayOutput) {
      throw new Error("witty gateway output helper is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);
    await window.wittyGatewayIdle?.();
    const frameCountBefore = inputFrames().length;

    await window.wittyPushGatewayOutput([
      27, 91, 52, 59, 55, 72, // CSI 4;7 H
      27, 91, 99, // CSI c
      27, 91, 53, 110, // CSI 5 n
      27, 91, 54, 110, // CSI 6 n
      27, 91, 63, 49, 104, // CSI ? 1 h
      27, 91, 63, 49, 36, 112, // CSI ? 1 $ p
      27, 91, 63, 55, 108, // CSI ? 7 l
      27, 91, 63, 55, 36, 112, // CSI ? 7 $ p
      27, 91, 52, 104, // CSI 4 h
      27, 91, 52, 36, 112, // CSI 4 $ p
      27, 91, 49, 56, 116, // CSI 18 t
      27, 91, 63, 50, 48, 50, 54, 104, // CSI ? 2026 h
      27, 91, 63, 50, 48, 50, 54, 36, 112, // CSI ? 2026 $ p
      27, 91, 63, 50, 48, 50, 54, 108, // CSI ? 2026 l
    ]);
    await window.wittyGatewayIdle?.();

    return {
      gridRows: window.wittySession.grid_rows(),
      gridCols: window.wittySession.grid_cols(),
      inputBytes: inputFrames().slice(frameCountBefore),
      lastTerminalReplyFrame: window.wittyLastTerminalReplyFrame,
      screenText: window.wittySession.screen_text(),
    };
  });

  const expectedFrame = [
    27, 91, 63, 49, 59, 50, 99, // CSI ? 1 ; 2 c
    27, 91, 48, 110, // CSI 0 n
    27, 91, 52, 59, 55, 82, // CSI 4 ; 7 R
    27, 91, 63, 49, 59, 49, 36, 121, // CSI ? 1 ; 1 $ y
    27, 91, 63, 55, 59, 50, 36, 121, // CSI ? 7 ; 2 $ y
    27, 91, 52, 59, 49, 36, 121, // CSI 4 ; 1 $ y
    ...Array.from(new TextEncoder().encode(`\x1b[8;${result.gridRows};${result.gridCols}t`)), // CSI 8 ; rows ; cols t
    27, 91, 63, 50, 48, 50, 54, 59, 49, 36, 121, // CSI ? 2026 ; 1 $ y
  ];
  if (JSON.stringify(result.inputBytes) !== JSON.stringify([expectedFrame])) {
    throw new Error(
      `terminal query reply smoke sent ${JSON.stringify(result.inputBytes)}, expected ${JSON.stringify([expectedFrame])}`,
    );
  }
  if (
    !result.lastTerminalReplyFrame ||
    JSON.stringify(result.lastTerminalReplyFrame.bytes) !== JSON.stringify(expectedFrame)
  ) {
    throw new Error(`terminal query reply frame mismatch: ${JSON.stringify(result)}`);
  }
  if (
    result.screenText.includes("[?1;2c") ||
    result.screenText.includes("[0n") ||
    result.screenText.includes("[4;7R") ||
    result.screenText.includes("[?1;1$y") ||
    result.screenText.includes("[?7;2$y") ||
    result.screenText.includes("[4;1$y") ||
    result.screenText.includes(`[8;${result.gridRows};${result.gridCols}t`) ||
    result.screenText.includes("[?2026;1$y")
  ) {
    throw new Error(`terminal query reply leaked into screen text: ${JSON.stringify(result)}`);
  }

  return result.inputBytes;
}

async function runBrowserShellIntegrationSmoke(page) {
  const result = await page.evaluate(async () => {
    const canvas = document.getElementById("witty-canvas");
    if (!window.wittySession || !window.wittyPushGatewayOutput) {
      throw new Error("witty session or gateway output helper is missing");
    }
    if (!canvas) {
      throw new Error("witty canvas is missing");
    }
    if (!window.wittyCommandBlocks) {
      throw new Error("witty command block diagnostics are missing");
    }
    if (
      !window.wittySelectLatestCommandBlock ||
      !window.wittySelectPreviousCommandBlock ||
      !window.wittySelectNextCommandBlock ||
      !window.wittyToggleSelectedCommandBlockFold ||
      !window.wittyCommandBlockGutterHit ||
      !window.wittySelectCommandBlockGutterHit ||
      !window.wittyClearSelectedCommandBlock ||
      !window.wittyOpenCommandBlockActionMenu ||
      !window.wittyCommandBlockActionMenuState
    ) {
      throw new Error("witty command block navigation helpers are missing");
    }

    const encode = (text) => Array.from(new TextEncoder().encode(text));
    const dispatchKey = (key, options = {}) => {
      const event = new KeyboardEvent("keydown", {
        key,
        code: options.code ?? key,
        ctrlKey: options.ctrlKey ?? false,
        shiftKey: options.shiftKey ?? false,
        altKey: options.altKey ?? false,
        metaKey: options.metaKey ?? false,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };
    const before = window.wittyCommandBlocks();
    await window.wittyPushGatewayOutput(
      encode("\x1b[2J\x1b[H\x1b]133;A\x1b\\$\x1b]133;B\x1b\\ echo\x1b]133;C\x1b\\\r\nok\x1b]133;D;0\x1b\\"),
    );
    await window.wittyGatewayIdle?.();
    const after = window.wittyCommandBlocks();
    const block = after.completed[after.completed.length - 1] ?? null;
    const visibleBlock = after.visible[after.visible.length - 1] ?? null;
    const visibleRowSpan = after.visibleRowSpans[after.visibleRowSpans.length - 1] ?? null;
    const frameStatsBeforeSelection = window.wittyFrameStats();
    const latestSelection = window.wittySelectLatestCommandBlock();
    const frameStatsSelected = window.wittyFrameStats();
    const previousSelection = window.wittySelectPreviousCommandBlock();
    const nextSelection = window.wittySelectNextCommandBlock();
    const afterClearSelection = window.wittyClearSelectedCommandBlock();
    const frameStatsCleared = window.wittyFrameStats();
    const rect = canvas.getBoundingClientRect();
    const gutterClick = new PointerEvent("pointerdown", {
      pointerId: 16,
      pointerType: "mouse",
      button: 0,
      buttons: 1,
      clientX: rect.left + 9,
      clientY: rect.top + 9,
      bubbles: true,
      cancelable: true,
    });
    canvas.dispatchEvent(gutterClick);
    const gutterClickPrevented = gutterClick.defaultPrevented;
    const afterGutterClickBlocks = window.wittyCommandBlocks();
    const afterGutterClickSelection = afterGutterClickBlocks.selected;
    const afterGutterClickTextRanges = afterGutterClickBlocks.selectedTextRanges;
    const afterGutterClickText = afterGutterClickBlocks.selectedText;
    const paletteOpenPrevented = dispatchKey("P", {
      code: "KeyP",
      ctrlKey: true,
      shiftKey: true,
    });
    for (const ch of "latest") {
      dispatchKey(ch, { code: `Key${ch.toUpperCase()}` });
    }
    const beforePaletteConfirm = { ...window.wittyLastCommandPalette };
    const paletteEnterPrevented = dispatchKey("Enter", { code: "Enter" });
    await window.wittyGatewayIdle?.();
    const afterPaletteSelection = window.wittyCommandBlocks();
    const latestPaletteInvocation = { ...window.wittyLastCommandPaletteInvocation };
    const previousClipboardApi = window.wittyClipboardApi;
    const commandBlockClipboardWrites = [];
    let copyOutputPaletteOpenPrevented = false;
    let copyOutputEnterPrevented = false;
    let beforeCopyOutputConfirm = null;
    let copyOutputResult = null;
    let copyCommandPaletteOpenPrevented = false;
    let copyCommandEnterPrevented = false;
    let beforeCopyCommandConfirm = null;
    let copyCommandResult = null;
    let actionMenuRightClickPrevented = false;
    let actionMenuContextPrevented = false;
    let afterActionMenuRightClick = null;
    let actionMenuCopyOutputEnterPrevented = false;
    let actionMenuCopyOutputResult = null;
    let beforeActionMenuCopyCommand = null;
    let actionMenuArrowDownPrevented = false;
    let afterActionMenuArrowDown = null;
    let actionMenuCopyCommandEnterPrevented = false;
    let actionMenuCopyCommandResult = null;
    let beforeActionMenuClear = null;
    let actionMenuClearFirstArrowPrevented = false;
    let actionMenuClearSecondArrowPrevented = false;
    let beforeActionMenuClearConfirm = null;
    let actionMenuClearEnterPrevented = false;
    let actionMenuClearInvocation = null;
    let selectedAfterActionMenuClear = null;
    let afterActionMenuClear = null;
    try {
      window.wittyClipboardApi = {
        writeText: async (text) => {
          commandBlockClipboardWrites.push(String(text));
        },
      };

      copyOutputPaletteOpenPrevented = dispatchKey("P", {
        code: "KeyP",
        ctrlKey: true,
        shiftKey: true,
      });
      for (const ch of "copy_output") {
        dispatchKey(ch, { code: `Key${ch.toUpperCase()}` });
      }
      beforeCopyOutputConfirm = { ...window.wittyLastCommandPalette };
      copyOutputEnterPrevented = dispatchKey("Enter", { code: "Enter" });
      await window.wittyLastCommandBlockCopyPromise;
      copyOutputResult = { ...window.wittyLastCommandBlockCopy };

      copyCommandPaletteOpenPrevented = dispatchKey("P", {
        code: "KeyP",
        ctrlKey: true,
        shiftKey: true,
      });
      for (const ch of "copy_command") {
        dispatchKey(ch, { code: `Key${ch.toUpperCase()}` });
      }
      beforeCopyCommandConfirm = { ...window.wittyLastCommandPalette };
      copyCommandEnterPrevented = dispatchKey("Enter", { code: "Enter" });
      await window.wittyLastCommandBlockCopyPromise;
      copyCommandResult = { ...window.wittyLastCommandBlockCopy };

      const actionMenuRightClick = new PointerEvent("pointerdown", {
        pointerId: 17,
        pointerType: "mouse",
        button: 2,
        buttons: 2,
        clientX: rect.left + 9,
        clientY: rect.top + 9,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(actionMenuRightClick);
      actionMenuRightClickPrevented = actionMenuRightClick.defaultPrevented;
      afterActionMenuRightClick = { ...window.wittyLastCommandBlockActionMenu };
      const actionMenuContext = new MouseEvent("contextmenu", {
        button: 2,
        buttons: 2,
        clientX: rect.left + 9,
        clientY: rect.top + 9,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(actionMenuContext);
      actionMenuContextPrevented = actionMenuContext.defaultPrevented;
      actionMenuCopyOutputEnterPrevented = dispatchKey("Enter", { code: "Enter" });
      await window.wittyLastCommandBlockCopyPromise;
      actionMenuCopyOutputResult = { ...window.wittyLastCommandBlockCopy };

      beforeActionMenuCopyCommand = window.wittyOpenCommandBlockActionMenu();
      actionMenuArrowDownPrevented = dispatchKey("ArrowDown", { code: "ArrowDown" });
      afterActionMenuArrowDown = { ...window.wittyLastCommandBlockActionMenu };
      actionMenuCopyCommandEnterPrevented = dispatchKey("Enter", { code: "Enter" });
      await window.wittyLastCommandBlockCopyPromise;
      actionMenuCopyCommandResult = { ...window.wittyLastCommandBlockCopy };

      beforeActionMenuClear = window.wittyOpenCommandBlockActionMenu();
      actionMenuClearFirstArrowPrevented = dispatchKey("ArrowDown", { code: "ArrowDown" });
      actionMenuClearSecondArrowPrevented = dispatchKey("ArrowDown", { code: "ArrowDown" });
      beforeActionMenuClearConfirm = { ...window.wittyLastCommandBlockActionMenu };
      actionMenuClearEnterPrevented = dispatchKey("Enter", { code: "Enter" });
      actionMenuClearInvocation = { ...window.wittyLastCommandBlockActionMenuInvocation };
      selectedAfterActionMenuClear = window.wittyCommandBlocks().selected;
      afterActionMenuClear = window.wittyCommandBlockActionMenuState();
    } finally {
      window.wittyClipboardApi = previousClipboardApi;
    }

    const shellIntegrationScreenText = window.wittySession.screen_text();
    const foldedBefore = window.wittyCommandBlocks();
    await window.wittyPushGatewayOutput(
      encode("\x1b[2J\x1b[H\x1b]133;A\x1b\\$\x1b]133;B\x1b\\ first\x1b]133;C\x1b\\\r\none\r\ntwo\x1b]133;D;0\x1b\\\r\n\x1b]133;A\x1b\\$\x1b]133;B\x1b\\ second\x1b]133;C\x1b\\\r\nok\x1b]133;D;0\x1b\\"),
    );
    await window.wittyGatewayIdle?.();
    const foldedLoaded = window.wittyCommandBlocks();
    const foldedFirstBlock =
      foldedLoaded.completed[foldedLoaded.completed.length - 2] ?? null;
    const foldedSecondBlock =
      foldedLoaded.completed[foldedLoaded.completed.length - 1] ?? null;
    const foldedLatestSelection = window.wittySelectLatestCommandBlock();
    const foldedFirstSelection = window.wittySelectPreviousCommandBlock();
    const foldedToggleSelection = window.wittyToggleSelectedCommandBlockFold();
    const afterFoldToggle = window.wittyCommandBlocks();
    const foldedHiddenRowsForFirst = afterFoldToggle.foldedCompactRows.filter(
      (row) => row.hidden_by_block_id === foldedFirstBlock?.id,
    );
    const foldedHiddenSpanForFirst = afterFoldToggle.foldedHiddenRowSpans.find(
      (span) => span.id === foldedFirstBlock?.id,
    ) ?? null;
    const foldedSecondCompactRow = afterFoldToggle.foldedCompactRows.find(
      (row) => row.visible_row === 3,
    ) ?? null;
    const foldedCellHeight = window.wittySession.ime_cursor_height_css();
    const foldedOffsetX = 9;
    const foldedOffsetY = 9 + foldedCellHeight;
    const foldedHitBeforeClick = window.wittyCommandBlockGutterHit(
      foldedOffsetX,
      foldedOffsetY,
    );
    const foldedGutterClick = new PointerEvent("pointerdown", {
      pointerId: 18,
      pointerType: "mouse",
      button: 0,
      buttons: 1,
      clientX: rect.left + foldedOffsetX,
      clientY: rect.top + foldedOffsetY,
      bubbles: true,
      cancelable: true,
    });
    canvas.dispatchEvent(foldedGutterClick);
    const foldedClickPrevented = foldedGutterClick.defaultPrevented;
    const afterFoldedClick = window.wittyCommandBlocks();
    const foldedHitAfterClick = window.wittyCommandBlockGutterHit(
      foldedOffsetX,
      foldedOffsetY,
    );

    return {
      beforeCount: before.completedCount,
      afterCount: after.completedCount,
      activeScreen: after.activeScreen,
      activeScreenCompletedCount: after.activeScreenCompletedCount,
      visibleCount: after.visibleCount,
      block,
      visibleBlock,
      visibleRowSpan,
      last: after.last,
      selectedBefore: after.selected,
      latestSelection,
      previousSelection,
      nextSelection,
      selectedAfterClear: afterClearSelection.selected,
      gutterClickPrevented,
      selectedAfterGutterClick: afterGutterClickSelection,
      textRangesAfterGutterClick: afterGutterClickTextRanges,
      textAfterGutterClick: afterGutterClickText,
      selectionBackgroundRunsBefore: frameStatsBeforeSelection.backgroundRuns,
      selectionBackgroundRunsSelected: frameStatsSelected.backgroundRuns,
      selectionBackgroundRunsCleared: frameStatsCleared.backgroundRuns,
      paletteOpenPrevented,
      paletteEnterPrevented,
      beforePaletteConfirm,
      paletteInvocation: latestPaletteInvocation,
      selectedAfterPaletteCommand: afterPaletteSelection.selected,
      commandBlockClipboardWrites,
      copyOutputPaletteOpenPrevented,
      copyOutputEnterPrevented,
      beforeCopyOutputConfirm,
      copyOutputResult,
      copyCommandPaletteOpenPrevented,
      copyCommandEnterPrevented,
      beforeCopyCommandConfirm,
      copyCommandResult,
      actionMenuRightClickPrevented,
      actionMenuContextPrevented,
      afterActionMenuRightClick,
      actionMenuCopyOutputEnterPrevented,
      actionMenuCopyOutputResult,
      beforeActionMenuCopyCommand,
      actionMenuArrowDownPrevented,
      afterActionMenuArrowDown,
      actionMenuCopyCommandEnterPrevented,
      actionMenuCopyCommandResult,
      beforeActionMenuClear,
      actionMenuClearFirstArrowPrevented,
      actionMenuClearSecondArrowPrevented,
      beforeActionMenuClearConfirm,
      actionMenuClearEnterPrevented,
      actionMenuClearInvocation,
      selectedAfterActionMenuClear,
      afterActionMenuClear,
      foldedCompact: {
        beforeCount: foldedBefore.completedCount,
        afterCount: foldedLoaded.completedCount,
        firstBlock: foldedFirstBlock,
        secondBlock: foldedSecondBlock,
        latestSelection: foldedLatestSelection,
        firstSelection: foldedFirstSelection,
        toggleSelection: foldedToggleSelection,
        hiddenRowsForFirst: foldedHiddenRowsForFirst,
        hiddenSpanForFirst: foldedHiddenSpanForFirst,
        secondCompactRow: foldedSecondCompactRow,
        hitBeforeClick: foldedHitBeforeClick,
        clickPrevented: foldedClickPrevented,
        selectedAfterClick: afterFoldedClick.selected,
        hitAfterClick: foldedHitAfterClick,
        screenText: window.wittySession.screen_text(),
      },
      screenText: shellIntegrationScreenText,
    };
  });

  if (result.afterCount !== result.beforeCount + 1) {
    throw new Error(`shell integration block count mismatch: ${JSON.stringify(result)}`);
  }
  if (!result.block || result.block.exit_code !== 0) {
    throw new Error(`shell integration block missing exit code: ${JSON.stringify(result)}`);
  }
  if (
    result.activeScreen !== "main" ||
    result.activeScreenCompletedCount < 1 ||
    result.visibleCount < 1 ||
    result.visibleBlock?.id !== result.block.id ||
    result.last?.id !== result.block.id ||
    result.visibleRowSpan?.id !== result.block.id ||
    result.visibleRowSpan?.start_row !== 0 ||
    result.visibleRowSpan?.end_row !== 1
  ) {
    throw new Error(`shell integration visible block diagnostics mismatch: ${JSON.stringify(result)}`);
  }
  if (
    result.selectedBefore !== null ||
    result.latestSelection?.id !== result.block.id ||
    result.previousSelection?.id !== result.block.id ||
    result.nextSelection?.id !== result.block.id ||
    result.selectedAfterClear !== null ||
    !result.gutterClickPrevented ||
    result.selectedAfterGutterClick?.id !== result.block.id
  ) {
    throw new Error(`shell integration block navigation mismatch: ${JSON.stringify(result)}`);
  }
  if (
    result.textRangesAfterGutterClick?.id !== result.block.id ||
    result.textRangesAfterGutterClick?.command?.start?.row !== 0 ||
    result.textRangesAfterGutterClick?.command?.start?.col !== 1 ||
    result.textRangesAfterGutterClick?.command?.end_exclusive?.row !== 0 ||
    result.textRangesAfterGutterClick?.command?.end_exclusive?.col !== 6 ||
    result.textRangesAfterGutterClick?.output?.start?.row !== 0 ||
    result.textRangesAfterGutterClick?.output?.start?.col !== 6 ||
    result.textRangesAfterGutterClick?.output?.end_exclusive?.row !== 1 ||
    result.textRangesAfterGutterClick?.output?.end_exclusive?.col !== 2
  ) {
    throw new Error(`shell integration text range mismatch: ${JSON.stringify(result)}`);
  }
  if (
    result.textAfterGutterClick?.id !== result.block.id ||
    result.textAfterGutterClick?.command !== " echo" ||
    result.textAfterGutterClick?.output !== "\nok"
  ) {
    throw new Error(`shell integration extracted text mismatch: ${JSON.stringify(result)}`);
  }
  if (
    result.selectionBackgroundRunsSelected <= result.selectionBackgroundRunsBefore ||
    result.selectionBackgroundRunsCleared >= result.selectionBackgroundRunsSelected
  ) {
    throw new Error(`shell integration selection overlay mismatch: ${JSON.stringify(result)}`);
  }
  if (
    !result.paletteOpenPrevented ||
    !result.paletteEnterPrevented ||
    result.beforePaletteConfirm?.selectedId !== "witty.command_block.latest" ||
    result.paletteInvocation?.commandId !== "witty.command_block.latest" ||
    result.selectedAfterPaletteCommand?.id !== result.block.id
  ) {
    throw new Error(`shell integration command palette command mismatch: ${JSON.stringify(result)}`);
  }
  if (
    !result.copyOutputPaletteOpenPrevented ||
    !result.copyOutputEnterPrevented ||
    result.beforeCopyOutputConfirm?.selectedId !== "witty.command_block.copy_output" ||
    result.copyOutputResult?.commandId !== "witty.command_block.copy_output" ||
    result.copyOutputResult?.copied !== true ||
    !result.copyCommandPaletteOpenPrevented ||
    !result.copyCommandEnterPrevented ||
    result.beforeCopyCommandConfirm?.selectedId !== "witty.command_block.copy_command" ||
    result.copyCommandResult?.commandId !== "witty.command_block.copy_command" ||
    result.copyCommandResult?.copied !== true ||
    JSON.stringify(result.commandBlockClipboardWrites) !== JSON.stringify(["ok", " echo", "ok", " echo"])
  ) {
    throw new Error(`shell integration command block copy mismatch: ${JSON.stringify(result)}`);
  }
  if (
    !result.actionMenuRightClickPrevented ||
    !result.actionMenuContextPrevented ||
    !result.afterActionMenuRightClick?.open ||
    result.afterActionMenuRightClick?.selectedId !== "witty.command_block.copy_output" ||
    result.afterActionMenuRightClick?.visibleItems?.length !== 4 ||
    !result.actionMenuCopyOutputEnterPrevented ||
    result.actionMenuCopyOutputResult?.commandId !== "witty.command_block.copy_output" ||
    result.actionMenuCopyOutputResult?.copied !== true ||
    !result.beforeActionMenuCopyCommand?.open ||
    !result.actionMenuArrowDownPrevented ||
    result.afterActionMenuArrowDown?.selectedId !== "witty.command_block.copy_command" ||
    !result.actionMenuCopyCommandEnterPrevented ||
    result.actionMenuCopyCommandResult?.commandId !== "witty.command_block.copy_command" ||
    result.actionMenuCopyCommandResult?.copied !== true ||
    !result.beforeActionMenuClear?.open ||
    !result.actionMenuClearFirstArrowPrevented ||
    !result.actionMenuClearSecondArrowPrevented ||
    result.beforeActionMenuClearConfirm?.selectedId !== "witty.command_block.clear" ||
    !result.actionMenuClearEnterPrevented ||
    result.actionMenuClearInvocation?.commandId !== "witty.command_block.clear" ||
    result.selectedAfterActionMenuClear !== null ||
    result.afterActionMenuClear?.open
  ) {
    throw new Error(`shell integration command block action menu mismatch: ${JSON.stringify(result)}`);
  }
  if (
    result.block.screen !== "main" ||
    result.block.prompt_start?.row !== 0 ||
    result.block.prompt_start?.col !== 0 ||
    result.block.command_start?.row !== 0 ||
    result.block.command_start?.col !== 1 ||
    result.block.output_start?.row !== 0 ||
    result.block.output_start?.col !== 6 ||
    result.block.finished_at?.row !== 1 ||
    result.block.finished_at?.col !== 2
  ) {
    throw new Error(`shell integration block positions mismatch: ${JSON.stringify(result)}`);
  }
  if (
    !result.screenText.includes("$ echo") ||
    !result.screenText.includes("ok") ||
    result.screenText.includes("133;")
  ) {
    throw new Error(`shell integration markers leaked or output missing: ${JSON.stringify(result)}`);
  }
  if (
    result.foldedCompact.afterCount !== result.foldedCompact.beforeCount + 2 ||
    !result.foldedCompact.firstBlock ||
    !result.foldedCompact.secondBlock ||
    result.foldedCompact.firstBlock.id === result.foldedCompact.secondBlock.id ||
    result.foldedCompact.latestSelection?.id !== result.foldedCompact.secondBlock.id ||
    result.foldedCompact.firstSelection?.id !== result.foldedCompact.firstBlock.id ||
    result.foldedCompact.toggleSelection?.id !== result.foldedCompact.firstBlock.id ||
    result.foldedCompact.toggleSelection?.folded !== true ||
    result.foldedCompact.hiddenRowsForFirst?.length !== 2 ||
    result.foldedCompact.hiddenSpanForFirst?.hidden_start_row !== 1 ||
    result.foldedCompact.hiddenSpanForFirst?.hidden_end_row !== 2 ||
    result.foldedCompact.secondCompactRow?.compact_row !== 1 ||
    result.foldedCompact.hitBeforeClick?.id !== result.foldedCompact.secondBlock.id ||
    !result.foldedCompact.clickPrevented ||
    result.foldedCompact.selectedAfterClick?.id !== result.foldedCompact.secondBlock.id ||
    result.foldedCompact.hitAfterClick?.id !== result.foldedCompact.secondBlock.id ||
    result.foldedCompact.hitAfterClick?.selected !== true ||
    !result.foldedCompact.screenText.includes("$ second") ||
    result.foldedCompact.screenText.includes("133;")
  ) {
    throw new Error(`shell integration folded compact product mismatch: ${JSON.stringify(result)}`);
  }

  return result.block;
}

async function runBrowserSynchronizedOutputSmoke(page) {
  const result = await page.evaluate(async () => {
    if (!window.wittySession || !window.wittyPushGatewayOutput) {
      throw new Error("witty session or gateway output helper is missing");
    }

    const encode = (text) => Array.from(new TextEncoder().encode(text));
    const beforeStats = window.wittyFrameStats();
    await window.wittyPushGatewayOutput(encode("\x1b[?2026h\x1b[2J\x1b[HSYNC-HIDDEN"));
    await window.wittyGatewayIdle?.();
    const hiddenStats = window.wittyFrameStats();
    const hiddenScreenHasMarker = window.wittySession.screen_text().includes("SYNC-HIDDEN");

    await window.wittyPushGatewayOutput(encode("\x1b[?2026l"));
    await window.wittyGatewayIdle?.();
    const afterStats = window.wittyFrameStats();
    const afterScreenHasMarker = window.wittySession.screen_text().includes("SYNC-HIDDEN");

    return {
      beforeGlyphChars: beforeStats.glyphChars,
      hiddenGlyphChars: hiddenStats.glyphChars,
      afterGlyphChars: afterStats.glyphChars,
      hiddenScreenHasMarker,
      afterScreenHasMarker,
    };
  });

  if (!result.hiddenScreenHasMarker || !result.afterScreenHasMarker) {
    throw new Error(`synchronized output marker missing from terminal state: ${JSON.stringify(result)}`);
  }
  if (result.hiddenGlyphChars !== result.beforeGlyphChars) {
    throw new Error(`synchronized output rendered before disable: ${JSON.stringify(result)}`);
  }
  if (result.afterGlyphChars === result.beforeGlyphChars) {
    throw new Error(`synchronized output did not render after disable: ${JSON.stringify(result)}`);
  }

  return result;
}

async function runBrowserSynchronizedOutputTimeoutSmoke(page) {
  const result = await page.evaluate(async () => {
    if (!window.wittySession || !window.wittyPushGatewayOutput) {
      throw new Error("witty session or gateway output helper is missing");
    }

    const encode = (text) => Array.from(new TextEncoder().encode(text));
    const timeout = Number(window.wittySynchronizedOutputTimeoutMs ?? 150);
    const beforeStats = window.wittyFrameStats();
    await window.wittyPushGatewayOutput(encode("\x1b[?2026h\x1b[2J\x1b[HSYNC-TIMEOUT"));
    await window.wittyGatewayIdle?.();
    const hiddenStats = window.wittyFrameStats();
    await new Promise((resolve) => setTimeout(resolve, timeout + 75));
    const timeoutStats = window.wittyFrameStats();
    const screenHasMarker = window.wittySession.screen_text().includes("SYNC-TIMEOUT");
    const modeStillEnabled = window.wittySession.synchronized_output_enabled();
    await window.wittyPushGatewayOutput(encode("\x1b[?2026l"));
    await window.wittyGatewayIdle?.();

    return {
      beforeGlyphChars: beforeStats.glyphChars,
      hiddenGlyphChars: hiddenStats.glyphChars,
      timeoutGlyphChars: timeoutStats.glyphChars,
      screenHasMarker,
      modeStillEnabled,
      timeout,
    };
  });

  if (!result.screenHasMarker || !result.modeStillEnabled) {
    throw new Error(`synchronized output timeout did not preserve terminal state: ${JSON.stringify(result)}`);
  }
  if (result.hiddenGlyphChars !== result.beforeGlyphChars) {
    throw new Error(`synchronized output timeout smoke rendered before timeout: ${JSON.stringify(result)}`);
  }
  if (result.timeoutGlyphChars === result.beforeGlyphChars) {
    throw new Error(`synchronized output timeout did not flush frame: ${JSON.stringify(result)}`);
  }

  return result;
}

async function runBrowserMouseSmoke(page) {
  const result = await page.evaluate(async () => {
    const session = window.wittySession;
    const canvas = document.getElementById("witty-canvas");
    if (!session || !canvas) {
      throw new Error("witty session or canvas is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    const frameCountBefore = inputFrames().length;
    const pushModeOutput = (bytes) => {
      if (!window.wittyPushGatewayOutput) {
        throw new Error("witty gateway output helper is missing");
      }
      return window.wittyPushGatewayOutput(bytes);
    };
    const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
    const rect = canvas.getBoundingClientRect();
    const cellPoint = (col, row) => ({
      clientX: rect.left + 8 + col * 9 + 1,
      clientY: rect.top + 8 + row * 18 + 1,
    });
    const dispatchPointer = (type, col, row, options = {}) => {
      const point = cellPoint(col, row);
      const event = new PointerEvent(type, {
        pointerId: 11,
        pointerType: "mouse",
        button: options.button ?? (type === "pointermove" ? -1 : 0),
        buttons: options.buttons ?? 0,
        clientX: point.clientX,
        clientY: point.clientY,
        shiftKey: options.shiftKey ?? false,
        altKey: options.altKey ?? false,
        ctrlKey: options.ctrlKey ?? false,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };
    const dispatchWheel = (col, row, deltaY) => {
      const point = cellPoint(col, row);
      const event = new WheelEvent("wheel", {
        deltaY,
        clientX: point.clientX,
        clientY: point.clientY,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };

    const inactivePrevented = dispatchPointer("pointerdown", 2, 3, {
      button: 0,
      buttons: 1,
    });
    await settle();
    await pushModeOutput([
      27, 91, 63, 49, 48, 48, 48, 104, 27, 91, 63, 49, 48, 48, 54, 104,
    ]);
    const normalPressPrevented = dispatchPointer("pointerdown", 2, 3, {
      button: 0,
      buttons: 1,
    });
    const normalReleasePrevented = dispatchPointer("pointerup", 2, 3, {
      button: 0,
      buttons: 0,
    });
    await settle();
    await pushModeOutput([27, 91, 63, 49, 48, 48, 50, 104]);
    const dragPressPrevented = dispatchPointer("pointerdown", 2, 3, {
      button: 0,
      buttons: 1,
    });
    const duplicateDragPrevented = dispatchPointer("pointermove", 2, 3, {
      buttons: 1,
    });
    const dragMovePrevented = dispatchPointer("pointermove", 3, 3, {
      buttons: 1,
    });
    const wheelPrevented = dispatchWheel(3, 3, -120);

    return {
      inactivePrevented,
      prevented: [
        normalPressPrevented,
        normalReleasePrevented,
        dragPressPrevented,
        dragMovePrevented,
        wheelPrevented,
      ],
      duplicateDragPrevented,
      inputBytes: inputFrames().slice(frameCountBefore),
    };
  });

  const expected = [
    [27, 91, 60, 48, 59, 51, 59, 52, 77],
    [27, 91, 60, 48, 59, 51, 59, 52, 109],
    [27, 91, 60, 48, 59, 51, 59, 52, 77],
    [27, 91, 60, 51, 50, 59, 52, 59, 52, 77],
    [27, 91, 60, 54, 52, 59, 52, 59, 52, 77],
  ];
  if (JSON.stringify(result.inputBytes) !== JSON.stringify(expected)) {
    throw new Error(
      `mouse runtime smoke sent ${JSON.stringify(result.inputBytes)}, expected ${JSON.stringify(expected)}`,
    );
  }
  if (result.inactivePrevented) {
    throw new Error(`mouse runtime smoke handled inactive mouse event: ${JSON.stringify(result)}`);
  }
  if (!result.prevented.every(Boolean) || result.duplicateDragPrevented) {
    throw new Error(`mouse runtime smoke preventDefault mismatch: ${JSON.stringify(result)}`);
  }

  return result.inputBytes;
}

async function runBrowserLocalScrollbackWheelSmoke(page) {
  const result = await page.evaluate(async () => {
    const session = window.wittySession;
    const canvas = document.getElementById("witty-canvas");
    if (!session || !canvas) {
      throw new Error("witty session or canvas is missing");
    }
    if (
      typeof session.handle_mouse !== "function" ||
      typeof session.viewport_offset !== "function"
    ) {
      throw new Error("local scrollback wheel helpers are missing");
    }
    if (!window.wittyPushGatewayOutput || !window.wittySetMouseSelectionOverridePolicy) {
      throw new Error("gateway output or mouse policy helper is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);
    const encode = (text) => Array.from(new TextEncoder().encode(text));
    const pushOutputText = async (text) => {
      await window.wittyPushGatewayOutput(encode(text));
      await window.wittyGatewayIdle?.();
    };
    const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
    const rect = canvas.getBoundingClientRect();
    const cellPoint = (col, row) => ({
      clientX: rect.left + 8 + col * 9 + 1,
      clientY: rect.top + 8 + row * 18 + 1,
    });
    const dispatchWheel = (col, row, deltaY, options = {}) => {
      const point = cellPoint(col, row);
      const event = new WheelEvent("wheel", {
        deltaY,
        deltaMode: options.deltaMode ?? 0,
        clientX: point.clientX,
        clientY: point.clientY,
        shiftKey: options.shiftKey ?? false,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };

    const disableMouseReporting = "\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l";
    const enableButtonMouseReporting = "\x1b[?1002h\x1b[?1006h";
    const previousPolicy = window.wittyMouseSelectionOverridePolicy();
    const scrollbackLines = Array.from(
      { length: 72 },
      (_, index) => `wheel-scroll-${String(index).padStart(2, "0")}\r\n`,
    ).join("");

    try {
      await window.wittyGatewayIdle?.();
      await pushOutputText(`${disableMouseReporting}\x1b[2J\x1b[H${scrollbackLines}wheel-tail`);
      if (session.mouse_reporting_active()) {
        throw new Error("local wheel smoke failed to disable mouse reporting");
      }

      const inactiveFrameCount = inputFrames().length;
      const inactiveOffsetBefore = session.viewport_offset();
      const inactivePrevented = dispatchWheel(3, 3, -120);
      await settle();
      if (window.wittyLastLocalWheelError) {
        return { localWheelError: window.wittyLastLocalWheelError };
      }
      const inactiveOffsetAfter = session.viewport_offset();
      const inactiveInputBytes = inputFrames().slice(inactiveFrameCount);

      await pushOutputText(enableButtonMouseReporting);
      if (!session.mouse_reporting_active()) {
        throw new Error("local wheel smoke failed to enable mouse reporting");
      }
      window.wittySetMouseSelectionOverridePolicy("shift-select");

      const shiftFrameCount = inputFrames().length;
      const shiftOffsetBefore = session.viewport_offset();
      const shiftPrevented = dispatchWheel(3, 3, -120, { shiftKey: true });
      await settle();
      if (window.wittyLastLocalWheelError) {
        return { localWheelError: window.wittyLastLocalWheelError };
      }
      const shiftOffsetAfter = session.viewport_offset();
      const shiftInputBytes = inputFrames().slice(shiftFrameCount);

      const plainFrameCount = inputFrames().length;
      const plainPrevented = dispatchWheel(3, 3, -120);
      await settle();
      const plainInputBytes = inputFrames().slice(plainFrameCount);

      const result = {
        inactive: {
          prevented: inactivePrevented,
          offsetBefore: inactiveOffsetBefore,
          offsetAfter: inactiveOffsetAfter,
          inputBytes: inactiveInputBytes,
        },
        shift: {
          prevented: shiftPrevented,
          offsetBefore: shiftOffsetBefore,
          offsetAfter: shiftOffsetAfter,
          inputBytes: shiftInputBytes,
        },
        plain: {
          prevented: plainPrevented,
          inputBytes: plainInputBytes,
        },
      };
      window.wittySetMouseSelectionOverridePolicy(previousPolicy);
      await pushOutputText(disableMouseReporting);
      return result;
    } finally {
      window.wittySetMouseSelectionOverridePolicy(previousPolicy);
    }
  });

  if (result.localWheelError) {
    throw new Error(`local wheel scrollback threw in browser: ${result.localWheelError}`);
  }

  if (
    !result.inactive.prevented ||
    result.inactive.offsetAfter <= result.inactive.offsetBefore ||
    result.inactive.inputBytes.length !== 0
  ) {
    throw new Error(`inactive local wheel scrollback mismatch: ${JSON.stringify(result)}`);
  }
  if (
    !result.shift.prevented ||
    result.shift.offsetAfter <= result.shift.offsetBefore ||
    result.shift.inputBytes.length !== 0
  ) {
    throw new Error(`shift local wheel scrollback mismatch: ${JSON.stringify(result)}`);
  }
  const expectedPlainWheel = [[27, 91, 60, 54, 52, 59, 52, 59, 52, 77]];
  if (
    !result.plain.prevented ||
    JSON.stringify(result.plain.inputBytes) !== JSON.stringify(expectedPlainWheel)
  ) {
    throw new Error(`plain mouse-report wheel mismatch: ${JSON.stringify(result)}`);
  }

  return {
    inactiveOffset: result.inactive.offsetAfter,
    shiftOffset: result.shift.offsetAfter,
    plainWheel: result.plain.inputBytes,
  };
}

async function runBrowserHyperlinkSmoke(page) {
  const result = await page.evaluate(async () => {
    const session = window.wittySession;
    const canvas = document.getElementById("witty-canvas");
    if (!session || !canvas) {
      throw new Error("witty session or canvas is missing");
    }
    if (!window.wittyPushGatewayOutput) {
      throw new Error("witty gateway output helper is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    const frameCountBefore = inputFrames().length;
    const opened = [];
    const originalOpen = window.open;
    window.open = (uri, target, features) => {
      opened.push({ uri, target, features });
      return { closed: false };
    };

    try {
      const linkBytes = Array.from(
        new TextEncoder().encode(
          "\x1b[2J\x1b[H\x1b]8;;https://example.com/witty\x07Link\x1b]8;;\x07",
        ),
      );
      await window.wittyPushGatewayOutput(linkBytes);
      await window.wittyGatewayIdle?.();

      const rect = canvas.getBoundingClientRect();
      const point = {
        clientX: rect.left + 8 + 1 * 9 + 1,
        clientY: rect.top + 8 + 1,
      };
      const hover = new PointerEvent("pointermove", {
        pointerId: 15,
        pointerType: "mouse",
        button: -1,
        buttons: 0,
        clientX: point.clientX,
        clientY: point.clientY,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(hover);

      const click = new PointerEvent("pointerdown", {
        pointerId: 15,
        pointerType: "mouse",
        button: 0,
        buttons: 1,
        clientX: point.clientX,
        clientY: point.clientY,
        ctrlKey: true,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(click);
      await window.wittyPushGatewayOutput(
        Array.from(
          new TextEncoder().encode("\x1b[2J\x1b[HWitty web\r\nbrowser input ready\r\n> "),
        ),
      );
      await window.wittyGatewayIdle?.();

      return {
        hoverPrevented: hover.defaultPrevented,
        clickPrevented: click.defaultPrevented,
        opened,
        last: window.wittyLastHyperlinkOpen,
        inputBytes: inputFrames().slice(frameCountBefore),
      };
    } finally {
      window.open = originalOpen;
    }
  });

  if (result.hoverPrevented) {
    throw new Error(`hyperlink hover should not prevent pointer defaults: ${JSON.stringify(result)}`);
  }
  if (!result.clickPrevented) {
    throw new Error(`hyperlink activation did not prevent pointer default: ${JSON.stringify(result)}`);
  }
  if (
    result.opened.length !== 1 ||
    result.opened[0].uri !== "https://example.com/witty" ||
    result.opened[0].target !== "_blank" ||
    !String(result.opened[0].features).includes("noopener")
  ) {
    throw new Error(`hyperlink activation did not call window.open correctly: ${JSON.stringify(result)}`);
  }
  if (!result.last?.opened || result.last?.blocked || result.last?.uri !== "https://example.com/witty") {
    throw new Error(`hyperlink activation status mismatch: ${JSON.stringify(result)}`);
  }
  if (result.inputBytes.length !== 0) {
    throw new Error(`hyperlink activation sent terminal input bytes: ${JSON.stringify(result)}`);
  }

  return result.opened[0].uri;
}

async function runBrowserMouseProductSmoke(page) {
  const result = await page.evaluate(async () => {
    const session = window.wittySession;
    const canvas = document.getElementById("witty-canvas");
    if (!session || !canvas) {
      throw new Error("witty session or canvas is missing");
    }
    if (!window.wittyPushGatewayOutput) {
      throw new Error("witty gateway output helper is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    await window.wittyGatewayIdle?.();
    await window.wittyPushGatewayOutput([
      27, 91, 63, 49, 48, 48, 50, 104, 27, 91, 63, 49, 48, 48, 54, 104,
    ]);
    if (!session.mouse_reporting_active()) {
      throw new Error("product mouse runtime smoke failed to enable mouse reporting");
    }

    const frameCountBefore = inputFrames().length;
    const rect = canvas.getBoundingClientRect();
    const cellPoint = (col, row) => ({
      clientX: rect.left + 8 + col * 9 + 1,
      clientY: rect.top + 8 + row * 18 + 1,
    });
    const dispatchPointer = (type, col, row, options = {}) => {
      const point = cellPoint(col, row);
      const event = new PointerEvent(type, {
        pointerId: 12,
        pointerType: "mouse",
        button: options.button ?? (type === "pointermove" ? -1 : 0),
        buttons: options.buttons ?? 0,
        clientX: point.clientX,
        clientY: point.clientY,
        shiftKey: options.shiftKey ?? false,
        altKey: options.altKey ?? false,
        ctrlKey: options.ctrlKey ?? false,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };
    const dispatchWheel = (col, row, deltaY) => {
      const point = cellPoint(col, row);
      const event = new WheelEvent("wheel", {
        deltaY,
        clientX: point.clientX,
        clientY: point.clientY,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };

    const normalPressPrevented = dispatchPointer("pointerdown", 2, 3, {
      button: 0,
      buttons: 1,
    });
    const normalReleasePrevented = dispatchPointer("pointerup", 2, 3, {
      button: 0,
      buttons: 0,
    });
    const dragPressPrevented = dispatchPointer("pointerdown", 2, 3, {
      button: 0,
      buttons: 1,
    });
    const duplicateDragPrevented = dispatchPointer("pointermove", 2, 3, {
      buttons: 1,
    });
    const dragMovePrevented = dispatchPointer("pointermove", 3, 3, {
      buttons: 1,
    });
    const wheelPrevented = dispatchWheel(3, 3, -120);

    return {
      prevented: [
        normalPressPrevented,
        normalReleasePrevented,
        dragPressPrevented,
        dragMovePrevented,
        wheelPrevented,
      ],
      duplicateDragPrevented,
      inputBytes: inputFrames().slice(frameCountBefore),
    };
  });

  const expected = [
    [27, 91, 60, 48, 59, 51, 59, 52, 77],
    [27, 91, 60, 48, 59, 51, 59, 52, 109],
    [27, 91, 60, 48, 59, 51, 59, 52, 77],
    [27, 91, 60, 51, 50, 59, 52, 59, 52, 77],
    [27, 91, 60, 54, 52, 59, 52, 59, 52, 77],
  ];
  if (JSON.stringify(result.inputBytes) !== JSON.stringify(expected)) {
    throw new Error(
      `product mouse runtime smoke sent ${JSON.stringify(result.inputBytes)}, expected ${JSON.stringify(expected)}`,
    );
  }
  if (!result.prevented.every(Boolean) || result.duplicateDragPrevented) {
    throw new Error(`product mouse runtime smoke preventDefault mismatch: ${JSON.stringify(result)}`);
  }

  return result.inputBytes;
}

async function runBrowserMouseSelectionOverrideSmoke(page) {
  const result = await page.evaluate(() => {
    const canvas = document.getElementById("witty-canvas");
    if (!window.wittySession || !canvas) {
      throw new Error("witty session or canvas is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    const frameCountBefore = inputFrames().length;
    const rect = canvas.getBoundingClientRect();
    const cellPoint = (col, row) => ({
      clientX: rect.left + 8 + col * 9 + 1,
      clientY: rect.top + 8 + row * 18 + 1,
    });
    const dispatchPointer = (type, col, row, options = {}) => {
      const point = cellPoint(col, row);
      const event = new PointerEvent(type, {
        pointerId: 13,
        pointerType: "mouse",
        button: options.button ?? (type === "pointermove" ? -1 : 0),
        buttons: options.buttons ?? 0,
        clientX: point.clientX,
        clientY: point.clientY,
        shiftKey: options.shiftKey ?? false,
        bubbles: true,
        cancelable: true,
        detail: options.detail ?? 1,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };

    const prevented = [
      dispatchPointer("pointerdown", 0, 0, {
        button: 0,
        buttons: 1,
        shiftKey: true,
      }),
      dispatchPointer("pointermove", 4, 0, {
        buttons: 1,
        shiftKey: false,
      }),
      dispatchPointer("pointerup", 4, 0, {
        button: 0,
        buttons: 0,
        shiftKey: false,
      }),
    ];

    return {
      prevented,
      inputBytes: inputFrames().slice(frameCountBefore),
      selectedText: window.wittySession.selected_text(),
      selectionRange: window.wittySession.selection_range_text(),
    };
  });

  if (result.inputBytes.length !== 0) {
    throw new Error(`selection override sent mouse bytes: ${JSON.stringify(result)}`);
  }
  if (!result.prevented.every(Boolean)) {
    throw new Error(`selection override did not prevent pointer defaults: ${JSON.stringify(result)}`);
  }
  if (!result.selectedText || !result.selectionRange) {
    throw new Error(`selection override did not create a local selection: ${JSON.stringify(result)}`);
  }

  return {
    selectedText: result.selectedText,
    selectionRange: result.selectionRange,
  };
}

async function runBrowserCopyEmptySelectionSmoke(page) {
  const result = await page.evaluate(async () => {
    const canvas = document.getElementById("witty-canvas");
    if (!window.wittySession || !canvas) {
      throw new Error("witty session or canvas is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    const frameCountBefore = inputFrames().length;
    const event = new KeyboardEvent("keydown", {
      key: "C",
      code: "KeyC",
      ctrlKey: true,
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    canvas.dispatchEvent(event);
    const copied = await window.wittyLastClipboardCopyPromise;

    return {
      prevented: event.defaultPrevented,
      copied,
      lastCopy: window.wittyLastClipboardCopy,
      selectedText: window.wittySession.selected_text(),
      inputBytes: inputFrames().slice(frameCountBefore),
    };
  });

  if (!result.prevented) {
    throw new Error(`empty copy selection shortcut was not prevented: ${JSON.stringify(result)}`);
  }
  if (result.copied || result.lastCopy?.reason !== "empty-selection") {
    throw new Error(`empty copy selection shortcut copied unexpectedly: ${JSON.stringify(result)}`);
  }
  if (result.selectedText) {
    throw new Error(`empty copy selection smoke started with a selection: ${JSON.stringify(result)}`);
  }
  if (result.inputBytes.length !== 0) {
    throw new Error(`empty copy selection shortcut sent input bytes: ${JSON.stringify(result)}`);
  }

  return result.lastCopy;
}

async function runBrowserCopySelectionSmoke(page) {
  const result = await page.evaluate(async () => {
    const canvas = document.getElementById("witty-canvas");
    if (!window.wittySession || !canvas) {
      throw new Error("witty session or canvas is missing");
    }
    if (!navigator.clipboard || typeof navigator.clipboard.readText !== "function") {
      throw new Error("browser clipboard readText is unavailable");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    const selectedText = window.wittySession.selected_text();
    const frameCountBefore = inputFrames().length;
    const event = new KeyboardEvent("keydown", {
      key: "C",
      code: "KeyC",
      ctrlKey: true,
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    canvas.dispatchEvent(event);
    const copied = await window.wittyLastClipboardCopyPromise;
    const clipboardText = await navigator.clipboard.readText();

    return {
      prevented: event.defaultPrevented,
      copied,
      lastCopy: window.wittyLastClipboardCopy,
      selectedText,
      clipboardText,
      inputBytes: inputFrames().slice(frameCountBefore),
    };
  });

  if (!result.selectedText) {
    throw new Error(`copy selection smoke did not start with selected text: ${JSON.stringify(result)}`);
  }
  if (!result.prevented || !result.copied || !result.lastCopy?.copied) {
    throw new Error(`copy selection shortcut was not handled: ${JSON.stringify(result)}`);
  }
  if (result.clipboardText !== result.selectedText) {
    throw new Error(`copy selection wrote ${JSON.stringify(result.clipboardText)}, expected ${JSON.stringify(result.selectedText)}`);
  }
  if (result.inputBytes.length !== 0) {
    throw new Error(`copy selection shortcut sent input bytes: ${JSON.stringify(result)}`);
  }

  return {
    text: result.clipboardText,
    textLength: result.lastCopy.textLength,
  };
}

async function runBrowserPasteEmptyClipboardSmoke(page) {
  const result = await page.evaluate(async () => {
    const canvas = document.getElementById("witty-canvas");
    if (!window.wittySession || !canvas) {
      throw new Error("witty session or canvas is missing");
    }
    if (
      !navigator.clipboard ||
      typeof navigator.clipboard.writeText !== "function" ||
      typeof navigator.clipboard.readText !== "function"
    ) {
      throw new Error("browser clipboard read/write text is unavailable");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    await navigator.clipboard.writeText("");
    const frameCountBefore = inputFrames().length;
    const event = new KeyboardEvent("keydown", {
      key: "V",
      code: "KeyV",
      ctrlKey: true,
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    canvas.dispatchEvent(event);
    const pasted = await window.wittyLastClipboardPastePromise;

    return {
      prevented: event.defaultPrevented,
      pasted,
      lastPaste: window.wittyLastClipboardPaste,
      inputBytes: inputFrames().slice(frameCountBefore),
    };
  });

  if (!result.prevented) {
    throw new Error(`empty paste shortcut was not prevented: ${JSON.stringify(result)}`);
  }
  if (result.pasted || result.lastPaste?.reason !== "empty-clipboard") {
    throw new Error(`empty paste shortcut pasted unexpectedly: ${JSON.stringify(result)}`);
  }
  if (result.inputBytes.length !== 0) {
    throw new Error(`empty paste shortcut sent input bytes: ${JSON.stringify(result)}`);
  }

  return result.lastPaste;
}

async function runBrowserPasteClipboardSmoke(page) {
  const clipboardText = "paste ok\n";
  const expected = Array.from(Buffer.from(clipboardText, "utf8"));
  const result = await page.evaluate(async (text) => {
    const canvas = document.getElementById("witty-canvas");
    if (!window.wittySession || !canvas) {
      throw new Error("witty session or canvas is missing");
    }
    if (
      !navigator.clipboard ||
      typeof navigator.clipboard.writeText !== "function" ||
      typeof navigator.clipboard.readText !== "function"
    ) {
      throw new Error("browser clipboard read/write text is unavailable");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    await navigator.clipboard.writeText(text);
    const frameCountBefore = inputFrames().length;
    const event = new KeyboardEvent("keydown", {
      key: "V",
      code: "KeyV",
      ctrlKey: true,
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    canvas.dispatchEvent(event);
    const pasted = await window.wittyLastClipboardPastePromise;

    return {
      prevented: event.defaultPrevented,
      pasted,
      lastPaste: window.wittyLastClipboardPaste,
      inputBytes: inputFrames().slice(frameCountBefore),
    };
  }, clipboardText);

  if (!result.prevented || !result.pasted || !result.lastPaste?.pasted) {
    throw new Error(`paste shortcut was not handled: ${JSON.stringify(result)}`);
  }
  if (JSON.stringify(result.inputBytes) !== JSON.stringify([expected])) {
    throw new Error(
      `paste shortcut sent ${JSON.stringify(result.inputBytes)}, expected ${JSON.stringify([expected])}`,
    );
  }

  return {
    textLength: result.lastPaste.textLength,
    inputBytes: result.inputBytes,
  };
}

async function runBrowserBracketedPasteClipboardSmoke(page) {
  const clipboardText = "bracket\n";
  const expected = Array.from(Buffer.from(`\x1b[200~${clipboardText}\x1b[201~`, "utf8"));
  const result = await page.evaluate(async (text) => {
    const canvas = document.getElementById("witty-canvas");
    if (!window.wittySession || !canvas) {
      throw new Error("witty session or canvas is missing");
    }
    if (!window.wittyPushGatewayOutput) {
      throw new Error("witty gateway output helper is missing");
    }
    if (
      !navigator.clipboard ||
      typeof navigator.clipboard.writeText !== "function" ||
      typeof navigator.clipboard.readText !== "function"
    ) {
      throw new Error("browser clipboard read/write text is unavailable");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    await window.wittyPushGatewayOutput([27, 91, 63, 50, 48, 48, 52, 104]);
    await navigator.clipboard.writeText(text);
    const frameCountBefore = inputFrames().length;
    const event = new KeyboardEvent("keydown", {
      key: "V",
      code: "KeyV",
      ctrlKey: true,
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    canvas.dispatchEvent(event);
    const pasted = await window.wittyLastClipboardPastePromise;
    const inputBytes = inputFrames().slice(frameCountBefore);
    await window.wittyPushGatewayOutput([27, 91, 63, 50, 48, 48, 52, 108]);

    return {
      prevented: event.defaultPrevented,
      pasted,
      lastPaste: window.wittyLastClipboardPaste,
      inputBytes,
    };
  }, clipboardText);

  if (!result.prevented || !result.pasted || !result.lastPaste?.pasted) {
    throw new Error(`bracketed paste shortcut was not handled: ${JSON.stringify(result)}`);
  }
  if (JSON.stringify(result.inputBytes) !== JSON.stringify([expected])) {
    throw new Error(
      `bracketed paste sent ${JSON.stringify(result.inputBytes)}, expected ${JSON.stringify([expected])}`,
    );
  }

  return {
    textLength: result.lastPaste.textLength,
    inputBytes: result.inputBytes,
  };
}

async function runBrowserOsc52ClipboardSmoke(page) {
  const result = await page.evaluate(async () => {
    const session = window.wittySession;
    if (!session) {
      throw new Error("witty session is missing");
    }
    if (!window.wittyPushGatewayOutput || !window.wittySetOsc52ClipboardPolicy) {
      throw new Error("OSC 52 browser helpers are missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);
    const encodeOsc52 = (selection, text) => {
      const bytes = new TextEncoder().encode(text);
      let binary = "";
      for (const byte of bytes) {
        binary += String.fromCharCode(byte);
      }
      return Array.from(
        new TextEncoder().encode(`\x1b]52;${selection};${btoa(binary)}\x07`),
      );
    };
    const compactResults = (results) =>
      results.map((result) => ({
        status: result.status,
        reason: result.reason,
        selection: result.selection,
        textLength: result.textLength,
        decodedBytes: result.decodedBytes,
        hasPayloadField: Object.prototype.hasOwnProperty.call(result, "text"),
      }));

    const previousClipboardApi = window.wittyClipboardApi;
    const previousPolicy = window.wittyOsc52ClipboardPolicy();
    const writes = [];
    const disabledPayload = "osc52 disabled payload";
    const allowedPayload = "osc52 allowed payload";
    const primaryPayload = "osc52 primary payload";

    window.wittyClipboardApi = {
      writeText: async (text) => {
        writes.push(text);
      },
    };

    try {
      await window.wittyGatewayIdle?.();
      const frameCountBefore = inputFrames().length;
      const defaultPolicy = window.wittyOsc52ClipboardPolicy();

      window.wittySetOsc52ClipboardPolicy("disabled");
      await window.wittyPushGatewayOutput(encodeOsc52("c", disabledPayload));
      await window.wittyGatewayIdle?.();
      const disabledResults = compactResults(window.wittyLastOsc52ClipboardResults ?? []);
      const writesAfterDisabled = writes.length;

      window.wittySetOsc52ClipboardPolicy("allow");
      await window.wittyPushGatewayOutput(encodeOsc52("c", allowedPayload));
      await window.wittyGatewayIdle?.();
      const allowResults = compactResults(window.wittyLastOsc52ClipboardResults ?? []);
      const writesAfterAllow = writes.length;

      await window.wittyPushGatewayOutput(encodeOsc52("p", primaryPayload));
      await window.wittyGatewayIdle?.();
      const primaryResults = compactResults(window.wittyLastOsc52ClipboardResults ?? []);
      const writesAfterPrimary = writes.length;

      const screenText = session.screen_text();

      return {
        defaultPolicy,
        disabledResults,
        allowResults,
        primaryResults,
        writesAfterDisabled,
        writesAfterAllow,
        writesAfterPrimary,
        writeLengths: writes.map((text) => text.length),
        screenTextHasPayload:
          screenText.includes(disabledPayload) ||
          screenText.includes(allowedPayload) ||
          screenText.includes(primaryPayload),
        inputBytes: inputFrames().slice(frameCountBefore),
      };
    } finally {
      window.wittyClipboardApi = previousClipboardApi;
      window.wittySetOsc52ClipboardPolicy(previousPolicy);
    }
  });

  if (result.defaultPolicy !== "disabled") {
    throw new Error(`OSC 52 default policy mismatch: ${JSON.stringify(result)}`);
  }
  if (
    result.disabledResults.length !== 1 ||
    result.disabledResults[0].status !== "denied" ||
    result.disabledResults[0].reason !== "policy-disabled" ||
    result.disabledResults[0].selection !== "clipboard" ||
    result.disabledResults[0].hasPayloadField ||
    result.writesAfterDisabled !== 0
  ) {
    throw new Error(`OSC 52 disabled policy result mismatch: ${JSON.stringify(result)}`);
  }
  if (
    result.allowResults.length !== 1 ||
    result.allowResults[0].status !== "written" ||
    result.allowResults[0].reason !== "" ||
    result.allowResults[0].selection !== "clipboard" ||
    result.allowResults[0].hasPayloadField ||
    result.writesAfterAllow !== 1 ||
    result.writeLengths[0] !== result.allowResults[0].textLength
  ) {
    throw new Error(`OSC 52 allow policy result mismatch: ${JSON.stringify(result)}`);
  }
  if (
    result.primaryResults.length !== 1 ||
    result.primaryResults[0].status !== "unsupported" ||
    result.primaryResults[0].reason !== "unsupported-selection" ||
    result.primaryResults[0].selection !== "primary" ||
    result.primaryResults[0].hasPayloadField ||
    result.writesAfterPrimary !== 1
  ) {
    throw new Error(`OSC 52 primary policy result mismatch: ${JSON.stringify(result)}`);
  }
  if (result.screenTextHasPayload) {
    throw new Error(`OSC 52 payload leaked into screen text: ${JSON.stringify(result)}`);
  }
  if (result.inputBytes.length !== 0) {
    throw new Error(`OSC 52 clipboard smoke sent gateway input bytes: ${JSON.stringify(result)}`);
  }

  return {
    disabled: result.disabledResults[0],
    allow: result.allowResults[0],
    primary: result.primaryResults[0],
  };
}

async function runBrowserPasteProductSmoke(page, gatewayKind) {
  const token = `witty-${gatewayKind}-clipboard-paste-${Date.now()}`;
  const clipboardText = `${token}\n`;
  const result = await page.evaluate(async (text) => {
    const canvas = document.getElementById("witty-canvas");
    if (!window.wittySession || !canvas) {
      throw new Error("witty session or canvas is missing");
    }
    if (
      !navigator.clipboard ||
      typeof navigator.clipboard.writeText !== "function" ||
      typeof navigator.clipboard.readText !== "function"
    ) {
      return {
        skipped: true,
        reason: "clipboard-api-unavailable",
      };
    }

    const permissionReason = (error) => {
      const name = String(error?.name ?? "");
      const message = String(error?.message ?? error);
      if (
        name === "NotAllowedError" ||
        name === "SecurityError" ||
        /permission|denied|not allowed|secure context/i.test(message)
      ) {
        return message;
      }
      return "";
    };

    try {
      await navigator.clipboard.writeText(text);
    } catch (error) {
      const reason = permissionReason(error);
      if (reason) {
        return {
          skipped: true,
          reason: `clipboard-write-denied: ${reason}`,
        };
      }
      throw error;
    }

    const event = new KeyboardEvent("keydown", {
      key: "V",
      code: "KeyV",
      ctrlKey: true,
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    canvas.dispatchEvent(event);
    const pasted = await window.wittyLastClipboardPastePromise;
    const lastPaste = window.wittyLastClipboardPaste;
    if (!pasted) {
      const reason = permissionReason({ message: lastPaste?.reason ?? "" });
      if (reason) {
        return {
          skipped: true,
          reason: `clipboard-read-denied: ${reason}`,
        };
      }
    }

    return {
      skipped: false,
      prevented: event.defaultPrevented,
      pasted,
      lastPaste,
    };
  }, clipboardText);

  if (result.skipped) {
    return `skipped-${result.reason}`;
  }
  if (!result.prevented || !result.pasted || !result.lastPaste?.pasted) {
    throw new Error(`product paste shortcut was not handled: ${JSON.stringify(result)}`);
  }

  await page.waitForFunction(
    (expectedToken) => window.wittyGatewayOutputText?.includes(expectedToken),
    token,
    { timeout: 10000 },
  );
  await page.evaluate(() => window.wittyGatewayIdle?.());

  return {
    token,
    textLength: result.lastPaste.textLength,
  };
}

async function runBrowserMouseSelectionDisabledPolicySmoke(page) {
  const result = await page.evaluate(() => {
    const canvas = document.getElementById("witty-canvas");
    if (!window.wittySession || !canvas) {
      throw new Error("witty session or canvas is missing");
    }
    if (!window.wittySetMouseSelectionOverridePolicy) {
      throw new Error("mouse selection override policy setter is missing");
    }
    if (!window.wittySession.mouse_reporting_active()) {
      throw new Error("mouse selection disabled-policy smoke needs mouse reporting active");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    const frameCountBefore = inputFrames().length;
    const previousPolicy = window.wittyMouseSelectionOverridePolicy();
    const policy = window.wittySetMouseSelectionOverridePolicy("disabled");
    const rect = canvas.getBoundingClientRect();
    const point = {
      clientX: rect.left + 8 + 1,
      clientY: rect.top + 8 + 1,
    };
    const dispatchPointer = (type, button, buttons) => {
      const event = new PointerEvent(type, {
        pointerId: 14,
        pointerType: "mouse",
        button,
        buttons,
        clientX: point.clientX,
        clientY: point.clientY,
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      });
      canvas.dispatchEvent(event);
      return event.defaultPrevented;
    };

    const prevented = [
      dispatchPointer("pointerdown", 0, 1),
      dispatchPointer("pointerup", 0, 0),
    ];
    const inputBytes = inputFrames().slice(frameCountBefore);
    window.wittySetMouseSelectionOverridePolicy(previousPolicy);

    return {
      policy,
      prevented,
      inputBytes,
    };
  });

  const expected = [
    [27, 91, 60, 52, 59, 49, 59, 49, 77],
    [27, 91, 60, 52, 59, 49, 59, 49, 109],
  ];
  if (JSON.stringify(result.inputBytes) !== JSON.stringify(expected)) {
    throw new Error(
      `selection disabled policy sent ${JSON.stringify(result.inputBytes)}, expected ${JSON.stringify(expected)}`,
    );
  }
  if (!result.prevented.every(Boolean)) {
    throw new Error(`selection disabled policy did not prevent pointer defaults: ${JSON.stringify(result)}`);
  }
  if (result.policy !== "disabled") {
    throw new Error(`selection disabled policy was not applied: ${JSON.stringify(result)}`);
  }

  return result.inputBytes;
}

async function runBrowserFocusSmoke(page) {
  const result = await page.evaluate(async () => {
    const session = window.wittySession;
    const canvas = document.getElementById("witty-canvas");
    if (!session || !canvas) {
      throw new Error("witty session or canvas is missing");
    }

    const inputFrames = () =>
      window.wittyGatewayFrames
        .filter(({ direction, frame }) => direction === "client" && frame.type === "input")
        .map(({ frame }) => frame.bytes);

    const frameCountBefore = inputFrames().length;
    const pushModeOutput = (bytes) => {
      if (!window.wittyPushGatewayOutput) {
        throw new Error("witty gateway output helper is missing");
      }
      return window.wittyPushGatewayOutput(bytes);
    };
    const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
    const dispatchFocus = (type) => {
      const event = new FocusEvent(type);
      canvas.dispatchEvent(event);
      return true;
    };

    dispatchFocus("focus");
    await settle();
    await pushModeOutput([27, 91, 63, 49, 48, 48, 52, 104]);
    dispatchFocus("focus");
    dispatchFocus("blur");
    await settle();
    await pushModeOutput([27, 91, 63, 49, 48, 48, 52, 108]);
    dispatchFocus("focus");

    return {
      inputBytes: inputFrames().slice(frameCountBefore),
    };
  });

  const expected = [
    [27, 91, 73],
    [27, 91, 79],
  ];
  if (JSON.stringify(result.inputBytes) !== JSON.stringify(expected)) {
    throw new Error(
      `focus runtime smoke sent ${JSON.stringify(result.inputBytes)}, expected ${JSON.stringify(expected)}`,
    );
  }

  return result.inputBytes;
}

function completeWebSocketHandshake(socket, requestText, protocolGuid) {
  const keyLine = requestText
    .split("\r\n")
    .find((line) => line.toLowerCase().startsWith("sec-websocket-key:"));
  if (!keyLine) {
    throw new Error("missing Sec-WebSocket-Key in gateway smoke handshake");
  }

  const key = keyLine.split(":").slice(1).join(":").trim();
  const accept = createHash("sha1")
    .update(`${key}${protocolGuid}`)
    .digest("base64");

  socket.write(
    [
      "HTTP/1.1 101 Switching Protocols",
      "Upgrade: websocket",
      "Connection: Upgrade",
      `Sec-WebSocket-Accept: ${accept}`,
      "\r\n",
    ].join("\r\n"),
  );
}

function consumeWebSocketFrames(buffer, handleFrame) {
  let offset = 0;
  while (buffer.length - offset >= 2) {
    const first = buffer[offset];
    const second = buffer[offset + 1];
    const opcode = first & 0x0f;
    const masked = (second & 0x80) !== 0;
    let length = second & 0x7f;
    let headerLength = 2;

    if (length === 126) {
      if (buffer.length - offset < 4) {
        break;
      }
      length = buffer.readUInt16BE(offset + 2);
      headerLength = 4;
    } else if (length === 127) {
      if (buffer.length - offset < 10) {
        break;
      }
      const high = buffer.readUInt32BE(offset + 2);
      const low = buffer.readUInt32BE(offset + 6);
      if (high !== 0) {
        throw new Error("gateway smoke WebSocket frame is too large");
      }
      length = low;
      headerLength = 10;
    }

    const maskLength = masked ? 4 : 0;
    const frameLength = headerLength + maskLength + length;
    if (buffer.length - offset < frameLength) {
      break;
    }

    const mask = masked
      ? buffer.subarray(offset + headerLength, offset + headerLength + 4)
      : null;
    const payloadStart = offset + headerLength + maskLength;
    const payload = Buffer.from(buffer.subarray(payloadStart, payloadStart + length));

    if (mask) {
      for (let index = 0; index < payload.length; index += 1) {
        payload[index] ^= mask[index % 4];
      }
    }

    handleFrame(opcode, payload);
    offset += frameLength;
  }

  return buffer.subarray(offset);
}

function sendGatewayJson(socket, frame) {
  sendWebSocketFrame(socket, 0x1, Buffer.from(JSON.stringify(frame), "utf8"));
}

function sendWebSocketFrame(socket, opcode, payload) {
  let header;
  if (payload.length < 126) {
    header = Buffer.from([0x80 | opcode, payload.length]);
  } else if (payload.length <= 0xffff) {
    header = Buffer.alloc(4);
    header[0] = 0x80 | opcode;
    header[1] = 126;
    header.writeUInt16BE(payload.length, 2);
  } else {
    throw new Error("gateway smoke WebSocket payload is too large");
  }

  socket.write(Buffer.concat([header, payload]));
}

function bytePrefixEquals(actual, expected) {
  if (actual.length < expected.length) {
    return false;
  }
  return expected.every((byte, index) => actual[index] === byte);
}

function pngHasNonblackPixel(bytes) {
  const signature = "89504e470d0a1a0a";
  if (bytes.subarray(0, 8).toString("hex") !== signature) {
    throw new Error("screenshot is not a PNG");
  }

  let offset = 8;
  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  const idatChunks = [];

  while (offset < bytes.length) {
    const length = bytes.readUInt32BE(offset);
    const chunkType = bytes.subarray(offset + 4, offset + 8).toString("ascii");
    const chunkData = bytes.subarray(offset + 8, offset + 8 + length);
    offset += 12 + length;

    if (chunkType === "IHDR") {
      width = chunkData.readUInt32BE(0);
      height = chunkData.readUInt32BE(4);
      bitDepth = chunkData[8];
      colorType = chunkData[9];
      const interlace = chunkData[12];
      if (bitDepth !== 8 || interlace !== 0) {
        throw new Error(`unsupported PNG format: bitDepth=${bitDepth} interlace=${interlace}`);
      }
    } else if (chunkType === "IDAT") {
      idatChunks.push(chunkData);
    } else if (chunkType === "IEND") {
      break;
    }
  }

  const bytesPerPixel = { 0: 1, 2: 3, 6: 4 }[colorType];
  if (!bytesPerPixel) {
    throw new Error(`unsupported PNG colorType=${colorType}`);
  }

  const stride = width * bytesPerPixel;
  const inflated = inflateSync(Buffer.concat(idatChunks));
  const current = Buffer.alloc(stride);
  const previous = Buffer.alloc(stride);
  let readOffset = 0;

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
  if (upDistance <= upLeftDistance) {
    return up;
  }
  return upLeft;
}

const gateway = createGatewaySmokeHarness(gatewayMode, gatewayPort);
let browser;

try {
  const launchedUrl = await gateway.start();
  if (launchedUrl) {
    url = launchedUrl;
    if (launcherBackedGatewayModes.has(gateway.kind) && url.includes("token=")) {
      throw new Error(`launcher page URL must not include gateway token: ${url}`);
    }
  }
  await waitForServer();
  browser = await chromium.launch({
    executablePath,
    args: ["--enable-unsafe-webgpu"],
  });
  const page = await browser.newPage({ viewport: { width: 1000, height: 620 } });
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"], {
    origin: new URL(url).origin,
  });
  const pageEvents = [];

  page.on("console", (message) => {
    pageEvents.push(`[console:${message.type()}] ${message.text()}`);
  });
  page.on("pageerror", (error) => {
    pageEvents.push(`[pageerror] ${error.stack ?? error.message}`);
  });

  if (launcherBackedModes.has(gateway.kind)) {
    await runBrowserMalformedLauncherHashSmoke(browser, url);
    await runBrowserMalformedLauncherSessionConfigSmoke(browser, url);
    await runBrowserMalformedProfilePickerBootstrapSmoke(browser, url);
    await runBrowserMalformedProfileImportBootstrapSmoke(browser, url);
  }
  if (gateway.kind === "profile-picker") {
    await runBrowserMalformedPickerSelectionOkSmoke(browser, url);
  }
  if (gateway.kind === "profile-picker-import") {
    await runBrowserMalformedPickerImportOkSmoke(browser, url);
  }
  if (profileImportModes.has(gateway.kind)) {
    await runBrowserMalformedProfileImportOkSmoke(browser, url);
  }

  await page.goto(url);
  let profilePickerSmoke = "not-run";
  let profilePickerSelection = null;
  let profilePickerImportSelection = null;
  let profileImportSmoke = "not-run";
  let profileImportSelection = null;
  try {
    if (gateway.kind === "profile-picker") {
      await page.waitForFunction(
        () => document.documentElement.dataset.wittySmoke === "profile_picker_ready",
        { timeout: 30000 },
      );
      profilePickerSelection = await runBrowserProfilePickerReadySmoke(page, url);
      await assertProfilePickerBootstrapIsStale(url);
      await assertProfilePickerSelectionRejectsNonJsonContentType(url, profilePickerSelection);
      const invalidSelectionState = await page.evaluate(async () => {
        const beforeLastSelection =
          window.wittyLastProfilePickerSelectionPromise ?? null;
        const missingSelected = await window.wittySelectProfile("missing");
        const afterMissingSelection =
          window.wittyLastProfilePickerSelectionPromise ?? null;
        const vaultedSelected = await window.wittySelectProfile("vaulted");
        const afterVaultedSelection =
          window.wittyLastProfilePickerSelectionPromise ?? null;
        return {
          missingSelected,
          vaultedSelected,
          missingLastPreserved: afterMissingSelection === beforeLastSelection,
          vaultedLastPreserved: afterVaultedSelection === beforeLastSelection,
          pickerState: window.wittyProfilePickerState ?? "",
          pickerError: window.wittyProfilePickerLastError ?? null,
          pickerToken: window.wittyProfilePickerBootstrap?.ui_token ?? null,
          launchButtonDisabled:
            document.querySelector(".profile-picker-option[data-profile-id='prod']")
              ?.disabled ?? true,
        };
      });
      if (
        invalidSelectionState.missingSelected !== false ||
        invalidSelectionState.vaultedSelected !== false ||
        invalidSelectionState.missingLastPreserved !== true ||
        invalidSelectionState.vaultedLastPreserved !== true ||
        invalidSelectionState.pickerState !== "profile_picker_ready" ||
        invalidSelectionState.pickerError !== null ||
        invalidSelectionState.pickerToken !== profilePickerSelection.uiToken ||
        invalidSelectionState.launchButtonDisabled !== false
      ) {
        throw new Error(
          `profile picker invalid selection helper call mutated state: ${JSON.stringify(
            invalidSelectionState,
          )}`,
        );
      }
      const selectionRaceState = await page.evaluate(async () => {
        const firstSelectionPromise = window.wittySelectProfile("prod");
        const firstSelectionWasLast =
          window.wittyLastProfilePickerSelectionPromise === firstSelectionPromise;
        const inFlightDuplicateSelected = await window.wittySelectProfile("staging");
        const lastSelectionPreserved =
          window.wittyLastProfilePickerSelectionPromise === firstSelectionPromise;
        const selected = await firstSelectionPromise;
        return {
          selected,
          inFlightDuplicateSelected,
          firstSelectionWasLast,
          lastSelectionPreserved,
          pickerState: window.wittyProfilePickerState ?? "",
          pickerError: window.wittyProfilePickerLastError ?? null,
          pickerToken: window.wittyProfilePickerBootstrap?.ui_token ?? null,
        };
      });
      if (
        !selectionRaceState.selected ||
        selectionRaceState.inFlightDuplicateSelected !== false ||
        selectionRaceState.firstSelectionWasLast !== true ||
        selectionRaceState.lastSelectionPreserved !== true ||
        selectionRaceState.pickerState !== "terminal_connected" ||
        selectionRaceState.pickerError !== null ||
        selectionRaceState.pickerToken !== ""
      ) {
        throw new Error(
          `profile picker selection race did not settle cleanly: ${JSON.stringify(
            selectionRaceState,
          )}`,
        );
      }
      const duplicateSelectionState = await page.evaluate(async () => {
        const attempted = await window.wittySelectProfile("staging");
        return {
          attempted,
          pickerState: window.wittyProfilePickerState ?? "",
          pickerError: window.wittyProfilePickerLastError ?? null,
          pickerToken: window.wittyProfilePickerBootstrap?.ui_token ?? null,
        };
      });
      if (
        duplicateSelectionState.attempted !== false ||
        duplicateSelectionState.pickerState !== "terminal_connected" ||
        duplicateSelectionState.pickerError !== null ||
        duplicateSelectionState.pickerToken !== ""
      ) {
        throw new Error(
          `profile picker duplicate selection was not ignored: ${JSON.stringify(
            duplicateSelectionState,
          )}`,
        );
      }
    }

    if (gateway.kind === "profile-picker-import") {
      await page.waitForFunction(
        () => document.documentElement.dataset.wittySmoke === "profile_picker_ready",
        { timeout: 30000 },
      );
      profilePickerImportSelection = await runBrowserProfilePickerImportReadySmoke(page, url);
      await assertProfilePickerBootstrapIsStale(url);
      await assertProfilePickerImportRejectsNonJsonContentType(
        url,
        profilePickerImportSelection,
      );
      const importEntryState = await page.evaluate(async () => {
        const beforeLastImport = window.wittyLastProfilePickerImportPromise ?? null;
        const badImportOpened = await window.wittyStartProfileImport("missing-action");
        const afterBadImport = window.wittyLastProfilePickerImportPromise ?? null;
        const badImportState = {
          pickerState: window.wittyProfilePickerState ?? "",
          pickerError: window.wittyProfilePickerLastError ?? null,
          pickerToken: window.wittyProfilePickerBootstrap?.ui_token ?? null,
          lastImportPreserved: afterBadImport === beforeLastImport,
          importButtonDisabled:
            document.querySelector(".profile-picker-import-action")?.disabled ?? true,
        };
        const firstImportPromise = window.wittyStartProfileImport("openssh-config");
        const firstImportWasLast =
          window.wittyLastProfilePickerImportPromise === firstImportPromise;
        const inFlightDuplicateOpened =
          await window.wittyStartProfileImport("openssh-config");
        const lastImportPreserved =
          window.wittyLastProfilePickerImportPromise === firstImportPromise;
        const opened = await firstImportPromise;
        const entry = window.wittyProfilePickerImportEntry;
        let mutationError = "";
        try {
          if (entry) {
            entry.import_url = "/index.html#profile_import=not-authorized";
          }
        } catch (error) {
          mutationError = String(error?.message ?? error);
        }
        const duplicateOpened = await window.wittyStartProfileImport("openssh-config");
        return {
          opened,
          inFlightDuplicateOpened,
          firstImportWasLast,
          lastImportPreserved,
          duplicateOpened,
          mutationError,
          entryFrozen: Object.isFrozen(entry),
          kind: entry?.kind ?? "",
          protocol: entry?.protocol ?? 0,
          importUrl: entry?.import_url ?? "",
          pickerState: window.wittyProfilePickerState ?? "",
          pickerError: window.wittyProfilePickerLastError ?? null,
          pickerToken: window.wittyProfilePickerBootstrap?.ui_token ?? null,
          badImportOpened,
          badImportState,
        };
      });
      if (
        importEntryState.badImportOpened !== false ||
        importEntryState.badImportState?.pickerState !== "profile_picker_ready" ||
        importEntryState.badImportState?.pickerError !== null ||
        importEntryState.badImportState?.pickerToken !== profilePickerImportSelection.uiToken ||
        importEntryState.badImportState?.lastImportPreserved !== true ||
        importEntryState.badImportState?.importButtonDisabled !== false ||
        !importEntryState.opened ||
        importEntryState.inFlightDuplicateOpened !== false ||
        importEntryState.firstImportWasLast !== true ||
        importEntryState.lastImportPreserved !== true ||
        importEntryState.duplicateOpened !== false ||
        importEntryState.entryFrozen !== true ||
        importEntryState.kind !== "profile_import_entry" ||
        importEntryState.protocol !== 1 ||
        !profileImportPageUrlPattern.test(importEntryState.importUrl) ||
        importEntryState.importUrl.includes("not-authorized") ||
        importEntryState.pickerState !== "profile_picker_import_ready" ||
        importEntryState.pickerError !== null ||
        importEntryState.pickerToken !== ""
      ) {
        throw new Error(
          `profile picker import action entry mismatch: ${JSON.stringify(
            importEntryState,
          )}`,
        );
      }
      await page.waitForFunction(
        () => document.documentElement.dataset.wittySmoke === "profile_import_ready",
        { timeout: 30000 },
      );
      if (page.url().includes("not-authorized")) {
        throw new Error(`profile picker import followed mutated entry URL: ${page.url()}`);
      }
      await assertProfilePickerImportIsStale(url, profilePickerImportSelection);
    }

    if (profileImportModes.has(gateway.kind)) {
      await page.waitForFunction(
        () => document.documentElement.dataset.wittySmoke === "profile_import_ready",
        { timeout: 30000 },
      );
      const importPageUrl = page.url();
      profileImportSelection = await runBrowserProfileImportReadySmoke(page, importPageUrl);
      await assertProfileImportBootstrapIsStale(importPageUrl);
      await assertProfileImportConfirmRejectsNonJsonContentType(
        importPageUrl,
        profileImportSelection,
      );
      const emptySelectionState = await page.evaluate(() => {
        const stagingInput = document.querySelector(
          ".profile-import-option input[data-profile-id='staging']",
        );
        const importButton = document.querySelector(".profile-import-confirm");
        if (stagingInput) {
          stagingInput.click();
        }
        const emptyDisabled = importButton?.disabled ?? false;
        if (stagingInput) {
          stagingInput.click();
        }
        return {
          inputExists: Boolean(stagingInput),
          importExists: Boolean(importButton),
          emptyDisabled,
          restoredChecked: stagingInput?.checked ?? false,
          restoredDisabled: stagingInput?.disabled ?? true,
          restoredImportDisabled: importButton?.disabled ?? true,
        };
      });
      if (
        !emptySelectionState.inputExists ||
        !emptySelectionState.importExists ||
        emptySelectionState.emptyDisabled !== true ||
        emptySelectionState.restoredChecked !== true ||
        emptySelectionState.restoredDisabled !== false ||
        emptySelectionState.restoredImportDisabled !== false
      ) {
        throw new Error(
          `profile import empty selection disable mismatch: ${JSON.stringify(
            emptySelectionState,
          )}`,
        );
      }
      const importConflictFlow = gateway.kind === "profile-import-reject" ? "reject" : "replace";
      if (importConflictFlow === "replace") {
        const conflictPolicyState = await page.evaluate(() => {
          const replaceButton = document.querySelector(
            ".profile-import-conflict-option[data-conflict-policy='replace']",
          );
          const rejectButton = document.querySelector(
            ".profile-import-conflict-option[data-conflict-policy='reject']",
          );
          replaceButton?.click();
          const firstChanged = window.wittyProfileImportConflictPolicy ?? "";
          const prodInput = document.querySelector(
            ".profile-import-option input[data-profile-id='prod']",
          );
          const stagingInput = document.querySelector(
            ".profile-import-option input[data-profile-id='staging']",
          );
          if (prodInput && !prodInput.checked) {
            prodInput.click();
          }
          if (stagingInput && !stagingInput.checked) {
            stagingInput.click();
          }
          rejectButton?.click();
          const rejectChanged = window.wittyProfileImportConflictPolicy ?? "";
          const rejectState = {
            policy: window.wittyProfileImportConflictPolicy ?? "",
            prodChecked: prodInput?.checked ?? true,
            prodDisabled: prodInput?.disabled ?? false,
            stagingChecked: stagingInput?.checked ?? false,
            stagingDisabled: stagingInput?.disabled ?? true,
          };
          replaceButton?.click();
          const finalChanged = window.wittyProfileImportConflictPolicy ?? "";
          if (prodInput && !prodInput.checked) {
            prodInput.click();
          }
          if (stagingInput && !stagingInput.checked) {
            stagingInput.click();
          }
          const importButton = document.querySelector(".profile-import-confirm");
          const controls = [...document.querySelectorAll(".profile-import-conflict-option")].map(
            (button) => ({
              policy: button.dataset.conflictPolicy,
              active: button.classList.contains("is-active"),
              pressed: button.getAttribute("aria-pressed"),
            }),
          );
          return {
            firstChanged,
            rejectChanged,
            finalChanged,
            policy: window.wittyProfileImportConflictPolicy ?? "",
            controls,
            rejectState,
            replaceButtonExists: Boolean(replaceButton),
            rejectButtonExists: Boolean(rejectButton),
            prodChecked: prodInput?.checked ?? false,
            prodDisabled: prodInput?.disabled ?? true,
            stagingChecked: stagingInput?.checked ?? false,
            stagingDisabled: stagingInput?.disabled ?? true,
            importDisabled: importButton?.disabled ?? true,
          };
        });
        const conflictStateByPolicy = new Map(
          conflictPolicyState.controls.map((control) => [control.policy, control]),
        );
        if (
          !conflictPolicyState.replaceButtonExists ||
          !conflictPolicyState.rejectButtonExists ||
          conflictPolicyState.firstChanged !== "replace" ||
          conflictPolicyState.rejectChanged !== "reject" ||
          conflictPolicyState.finalChanged !== "replace" ||
          conflictPolicyState.policy !== "replace" ||
          conflictStateByPolicy.get("replace")?.active !== true ||
          conflictStateByPolicy.get("replace")?.pressed !== "true" ||
          conflictStateByPolicy.get("reject")?.active !== false ||
          conflictStateByPolicy.get("reject")?.pressed !== "false" ||
          conflictPolicyState.rejectState?.policy !== "reject" ||
          conflictPolicyState.rejectState?.prodChecked !== false ||
          conflictPolicyState.rejectState?.prodDisabled !== true ||
          conflictPolicyState.rejectState?.stagingChecked !== true ||
          conflictPolicyState.rejectState?.stagingDisabled !== false ||
          conflictPolicyState.prodChecked !== true ||
          conflictPolicyState.prodDisabled !== false ||
          conflictPolicyState.stagingChecked !== true ||
          conflictPolicyState.stagingDisabled !== false ||
          conflictPolicyState.importDisabled !== false
        ) {
          throw new Error(
            `profile import conflict policy did not switch to replace: ${JSON.stringify(
              conflictPolicyState,
            )}`,
          );
        }
      } else {
        const rejectPolicyState = await page.evaluate(() => {
          const prodInput = document.querySelector(
            ".profile-import-option input[data-profile-id='prod']",
          );
          const stagingInput = document.querySelector(
            ".profile-import-option input[data-profile-id='staging']",
          );
          const importButton = document.querySelector(".profile-import-confirm");
          return {
            policy: window.wittyProfileImportConflictPolicy ?? "",
            prodChecked: prodInput?.checked ?? true,
            prodDisabled: prodInput?.disabled ?? false,
            stagingChecked: stagingInput?.checked ?? false,
            stagingDisabled: stagingInput?.disabled ?? true,
            importDisabled: importButton?.disabled ?? true,
          };
        });
        if (
          rejectPolicyState.policy !== "reject" ||
          rejectPolicyState.prodChecked !== false ||
          rejectPolicyState.prodDisabled !== true ||
          rejectPolicyState.stagingChecked !== true ||
          rejectPolicyState.stagingDisabled !== false ||
          rejectPolicyState.importDisabled !== false
        ) {
          throw new Error(
            `profile import reject policy default state mismatch: ${JSON.stringify(
              rejectPolicyState,
            )}`,
          );
        }
      }
      const preConfirmState = await page.evaluate(async (conflict) => {
        const importButton = document.querySelector(".profile-import-confirm");
        if (!importButton) {
          return {
            missingButton: true,
            importState: window.wittyProfileImportState ?? "",
            importError: window.wittyProfileImportLastError ?? null,
          };
        }
        const directRejectConflictResponse = await fetch(
          window.wittyProfileImportBootstrap?.confirm_url ?? "",
          {
            method: "POST",
            cache: "no-store",
            headers: {
              "Content-Type": "application/json",
            },
            body: JSON.stringify({
              ui_token: window.wittyProfileImportBootstrap?.ui_token ?? "",
              profile_ids: ["prod"],
              conflict: "reject",
            }),
          },
        );
        const directRejectConflictAttempt = {
          status: directRejectConflictResponse.status,
          text: await directRejectConflictResponse.text(),
          importState: window.wittyProfileImportState ?? "",
          importError: window.wittyProfileImportLastError ?? null,
          bootstrapToken: window.wittyProfileImportBootstrap?.ui_token ?? null,
          importButtonDisabled: importButton.disabled,
          report: window.wittyProfileImportReport ?? null,
        };
        const invalidConfirmCases = [
          { name: "invalid-conflict", profileIds: ["staging"], conflict: "invalid" },
          { name: "empty-selection", profileIds: [], conflict },
          { name: "duplicate-selection", profileIds: ["staging", "staging"], conflict },
          { name: "unknown-selection", profileIds: ["missing"], conflict },
          { name: "reject-conflict", profileIds: ["prod"], conflict: "reject" },
        ];
        const invalidConfirmAttempts = [];
        for (const confirmCase of invalidConfirmCases) {
          const beforeLastConfirm =
            window.wittyLastProfileImportConfirmPromise ?? null;
          const result = await window.wittyConfirmProfileImport(
            confirmCase.profileIds,
            confirmCase.conflict,
          );
          const afterLastConfirm =
            window.wittyLastProfileImportConfirmPromise ?? null;
          invalidConfirmAttempts.push({
            name: confirmCase.name,
            result,
            lastConfirmPreserved: afterLastConfirm === beforeLastConfirm,
            importState: window.wittyProfileImportState ?? "",
            importError: window.wittyProfileImportLastError ?? null,
            bootstrapToken: window.wittyProfileImportBootstrap?.ui_token ?? null,
            importButtonDisabled: importButton.disabled,
            report: window.wittyProfileImportReport ?? null,
          });
        }
        return {
          directRejectConflictAttempt,
          invalidConfirmAttempts,
          importState: window.wittyProfileImportState ?? "",
          importError: window.wittyProfileImportLastError ?? null,
        };
      }, importConflictFlow);
      let applyFailureAttempt = null;
      await gateway.setImportConfig(profileImportProdOnlyConfigText);
      try {
        applyFailureAttempt = await page.evaluate(async (conflict) => {
          const importButton = document.querySelector(".profile-import-confirm");
          const result = await window.wittyConfirmProfileImport(["staging"], conflict);
          return {
            result,
            importState: window.wittyProfileImportState ?? "",
            importError: window.wittyProfileImportLastError ?? null,
            bootstrapToken: window.wittyProfileImportBootstrap?.ui_token ?? null,
            importButtonDisabled: importButton?.disabled ?? true,
            report: window.wittyProfileImportReport ?? null,
          };
        }, importConflictFlow);
      } finally {
        await gateway.setImportConfig(profileImportConfigText);
      }
      const finishConfirmState = await page.evaluate(async (conflict) => {
        const importButton = document.querySelector(".profile-import-confirm");
        if (!importButton) {
          return {
            report: null,
            inFlightDuplicateConfirm: "missing-button",
            importState: window.wittyProfileImportState ?? "",
            importError: window.wittyProfileImportLastError ?? null,
          };
        }
        importButton.click();
        const firstConfirmPromise = window.wittyLastProfileImportConfirmPromise;
        const firstConfirmWasLast = Boolean(firstConfirmPromise?.then);
        const inFlightDuplicateConfirm = await window.wittyConfirmProfileImport(
          ["staging"],
          conflict,
        );
        const lastConfirmPreserved =
          window.wittyLastProfileImportConfirmPromise === firstConfirmPromise;
        const report = await firstConfirmPromise;
        return {
          report,
          inFlightDuplicateConfirm,
          firstConfirmWasLast,
          lastConfirmPreserved,
          importState: window.wittyProfileImportState ?? "",
          importError: window.wittyProfileImportLastError ?? null,
        };
      }, importConflictFlow);
      const confirmationState = {
        ...preConfirmState,
        applyFailureAttempt,
        ...finishConfirmState,
      };
      const report = confirmationState.report;
      if (!report) {
        const errorState = await page.evaluate(() => ({
          importState: window.wittyProfileImportState ?? "",
          importError: window.wittyProfileImportLastError ?? null,
        }));
        throw new Error(`profile import confirmation did not complete: ${JSON.stringify(errorState)}`);
      }
      if (
        confirmationState.directRejectConflictAttempt?.status !== 400 ||
        !confirmationState.directRejectConflictAttempt?.text?.includes(
          "reject policy cannot select conflicting profile ids",
        ) ||
        confirmationState.directRejectConflictAttempt?.importState !== "profile_import_ready" ||
        confirmationState.directRejectConflictAttempt?.importError !== null ||
        confirmationState.directRejectConflictAttempt?.bootstrapToken !==
          profileImportSelection.uiToken ||
        confirmationState.directRejectConflictAttempt?.importButtonDisabled !== false ||
        confirmationState.directRejectConflictAttempt?.report !== null ||
        confirmationState.invalidConfirmAttempts?.length !== 5 ||
        confirmationState.invalidConfirmAttempts.some(
          (attempt) =>
            attempt.result !== null ||
            attempt.lastConfirmPreserved !== true ||
            attempt.importState !== "profile_import_ready" ||
            attempt.importError !== null ||
            attempt.bootstrapToken !== profileImportSelection.uiToken ||
            attempt.importButtonDisabled !== false ||
            attempt.report !== null,
        ) ||
        confirmationState.applyFailureAttempt?.result !== null ||
        confirmationState.applyFailureAttempt?.importState !== "profile_import_error" ||
        confirmationState.applyFailureAttempt?.importError?.status !== 409 ||
        !confirmationState.applyFailureAttempt?.importError?.message?.includes(
          "profile import confirmation failed",
        ) ||
        confirmationState.applyFailureAttempt?.bootstrapToken !==
          profileImportSelection.uiToken ||
        confirmationState.applyFailureAttempt?.importButtonDisabled !== false ||
        confirmationState.applyFailureAttempt?.report !== null ||
        confirmationState.inFlightDuplicateConfirm !== null ||
        confirmationState.firstConfirmWasLast !== true ||
        confirmationState.lastConfirmPreserved !== true ||
        confirmationState.importError !== null
      ) {
        throw new Error(
          `profile import invalid, apply-failure, or in-flight confirmation guard mismatch: ${JSON.stringify(
            confirmationState,
          )}`,
        );
      }
      await page.waitForFunction(
        () => document.documentElement.dataset.wittySmoke === "profile_import_done",
        { timeout: 30000 },
      );
      const doneState = await page.evaluate(() => ({
        importState: window.wittyProfileImportState ?? "",
        smoke: document.documentElement.dataset.wittySmoke ?? "",
        report: window.wittyProfileImportReport ?? null,
        reportFrozen: Object.isFrozen(window.wittyProfileImportReport),
        bodyText: document.body.textContent ?? "",
        href: window.location.href,
        bootstrapToken: window.wittyProfileImportBootstrap?.ui_token ?? null,
        nextPickerUrl: window.wittyProfileImportNextPickerUrl ?? "",
        resultSummary: window.wittyProfileImportResultSummary ?? null,
        resultSummaryFrozen: Object.isFrozen(window.wittyProfileImportResultSummary),
        resultText: document.querySelector(".profile-import-result")?.textContent ?? "",
        completedPolicy: window.wittyProfileImportConflictPolicy ?? "",
        completedConflictControls: [
          ...document.querySelectorAll(".profile-import-conflict-option"),
        ].map((button) => ({
          policy: button.dataset.conflictPolicy,
          active: button.classList.contains("is-active"),
          pressed: button.getAttribute("aria-pressed"),
          disabled: button.disabled,
        })),
        completedCandidateControls: [
          ...document.querySelectorAll(".profile-import-option input[type='checkbox']"),
        ].map((input) => ({
          profileId: input.dataset.profileId,
          checked: input.checked,
          disabled: input.disabled,
        })),
        importButtonDisabled: document.querySelector(".profile-import-confirm")?.disabled ?? false,
        nextPickerButton: (() => {
          const button = document.querySelector(".profile-import-next-picker");
          return {
            exists: Boolean(button),
            hidden: button?.hidden ?? true,
            disabled: button?.disabled ?? true,
            text: button?.textContent ?? "",
          };
        })(),
      }));
      const completedHelperState = await page.evaluate(() => {
        const attemptedPolicy =
          window.wittySetProfileImportConflictPolicy?.(
            window.wittyProfileImportConflictPolicy === "reject" ? "replace" : "reject",
          ) ?? "";
        const attemptedEnable = window.wittySetProfileImportDisabled?.(false);
        return {
          attemptedPolicy,
          attemptedEnable,
          policy: window.wittyProfileImportConflictPolicy ?? "",
          importButtonDisabled:
            document.querySelector(".profile-import-confirm")?.disabled ?? false,
          controls: [
            ...document.querySelectorAll(".profile-import-conflict-option"),
          ].map((button) => ({
            policy: button.dataset.conflictPolicy,
            active: button.classList.contains("is-active"),
            pressed: button.getAttribute("aria-pressed"),
            disabled: button.disabled,
          })),
          candidates: [
            ...document.querySelectorAll(".profile-import-option input[type='checkbox']"),
          ].map((input) => ({
            profileId: input.dataset.profileId,
            disabled: input.disabled,
          })),
        };
      });
      const unauthorizedNextPickerState = await page.evaluate(() => {
        const button = document.querySelector(".profile-import-next-picker");
        const invalidUrls = [
          "/index.html#profile_picker=",
          "/index.html#profile_picker=not-authorized",
          "/index.html#profile_picker=0123456789abcdef0123456789abcdef&profile_import=bad",
          "http://127.0.0.1/index.html#profile_picker=0123456789abcdef0123456789abcdef",
        ];
        const attempts = invalidUrls.map((url) => ({
          url,
          accepted: window.wittySetProfileImportNextPicker?.(url) ?? null,
        }));
        return {
          attempts,
          nextPickerUrl: window.wittyProfileImportNextPickerUrl ?? "",
          button: {
            exists: Boolean(button),
            hidden: button?.hidden ?? true,
            disabled: button?.disabled ?? true,
            text: button?.textContent ?? "",
          },
        };
      });
      const expectedReport = {
        changed: true,
        profiles: 2,
        default_changed: false,
        selected: importConflictFlow === "reject" ? 1 : 2,
        added: 1,
        replaced: importConflictFlow === "reject" ? 0 : 1,
        warning_count: importConflictFlow === "reject" ? 0 : 2,
        global_warning_count: 0,
      };
      const duplicateConfirmState = await page.evaluate(async (conflict) => {
        const attempted = await window.wittyConfirmProfileImport(["staging"], conflict);
        return {
          attempted,
          importState: window.wittyProfileImportState ?? "",
          smoke: document.documentElement.dataset.wittySmoke ?? "",
          importError: window.wittyProfileImportLastError ?? null,
          bootstrapToken: window.wittyProfileImportBootstrap?.ui_token ?? null,
          report: window.wittyProfileImportReport ?? null,
          resultText: document.querySelector(".profile-import-result")?.textContent ?? "",
        };
      }, importConflictFlow);
      for (const [key, value] of Object.entries(expectedReport)) {
        if (doneState.report?.[key] !== value) {
          throw new Error(`profile import report ${key} mismatch: ${JSON.stringify(doneState)}`);
        }
        if (duplicateConfirmState.report?.[key] !== value) {
          throw new Error(
            `profile import duplicate confirmation mutated ${key}: ${JSON.stringify(
              duplicateConfirmState,
            )}`,
          );
        }
      }
      const expectedResultSummary = {
        selected: expectedReport.selected,
        added: expectedReport.added,
        replaced: expectedReport.replaced,
        warnings: expectedReport.warning_count,
        globalWarnings: expectedReport.global_warning_count,
      };
      for (const [key, value] of Object.entries(expectedResultSummary)) {
        if (doneState.resultSummary?.[key] !== value) {
          throw new Error(`profile import result summary ${key} mismatch: ${JSON.stringify(doneState)}`);
        }
      }
      const expectedResultText =
        `${expectedResultSummary.selected} selected - ` +
        `${expectedResultSummary.added} added - ` +
        `${expectedResultSummary.replaced} replaced - ` +
        `${expectedResultSummary.warnings} warnings - ` +
        `${expectedResultSummary.globalWarnings} global`;
      if (doneState.resultText !== expectedResultText) {
        throw new Error(`profile import result text mismatch: ${JSON.stringify(doneState)}`);
      }
      if (
        duplicateConfirmState.attempted !== null ||
        duplicateConfirmState.importState !== "profile_import_done" ||
        duplicateConfirmState.smoke !== "profile_import_done" ||
        duplicateConfirmState.importError !== null ||
        duplicateConfirmState.bootstrapToken !== "" ||
        duplicateConfirmState.resultText !== expectedResultText
      ) {
        throw new Error(
          `profile import duplicate confirmation was not ignored: ${JSON.stringify(
            duplicateConfirmState,
          )}`,
        );
      }
      const replayedReportState = await page.evaluate(() => {
        const report = window.wittyProfileImportReport;
        let mutationError = "";
        if (report && typeof report === "object") {
          try {
            report.selected = 99;
            report.added = 98;
            report.replaced = 97;
            report.warning_count = 96;
            report.global_warning_count = 95;
            report.next_picker_url = "/index.html#profile_picker=not-authorized";
          } catch (error) {
            mutationError = String(error?.message ?? error);
          }
        }
        const attemptedReport =
          typeof window.wittySetProfileImportReport === "function"
            ? window.wittySetProfileImportReport(report)
            : "missing";
        const invalidUrls = [
          "/index.html#profile_picker=",
          "/index.html#profile_picker=not-authorized",
          "/index.html#profile_picker=0123456789abcdef0123456789abcdef&profile_import=bad",
          "http://127.0.0.1/index.html#profile_picker=0123456789abcdef0123456789abcdef",
        ];
        const attemptedNextPicker =
          typeof window.wittySetProfileImportNextPicker === "function"
            ? invalidUrls.map((url) => ({
                url,
                accepted: window.wittySetProfileImportNextPicker(url),
              }))
            : "missing";
        const exposedSummary = window.wittyProfileImportResultSummary;
        let summaryMutationError = "";
        if (exposedSummary && typeof exposedSummary === "object") {
          try {
            exposedSummary.selected = 199;
            exposedSummary.added = 198;
            exposedSummary.replaced = 197;
            exposedSummary.warnings = 196;
            exposedSummary.globalWarnings = 195;
          } catch (error) {
            summaryMutationError = String(error?.message ?? error);
          }
        }
        const button = document.querySelector(".profile-import-next-picker");
        return {
          attemptedReport,
          attemptedNextPicker,
          mutationError,
          reportFrozen: Object.isFrozen(report),
          reportSnapshot: report,
          summaryMutationError,
          resultSummaryFrozen: Object.isFrozen(exposedSummary),
          resultSummary: window.wittyProfileImportResultSummary ?? null,
          resultText: document.querySelector(".profile-import-result")?.textContent ?? "",
          nextPickerUrl: window.wittyProfileImportNextPickerUrl ?? "",
          button: {
            exists: Boolean(button),
            hidden: button?.hidden ?? true,
            disabled: button?.disabled ?? true,
            text: button?.textContent ?? "",
          },
        };
      });
      for (const [key, value] of Object.entries(expectedResultSummary)) {
        if (
          replayedReportState.attemptedReport?.[key] !== value ||
          replayedReportState.resultSummary?.[key] !== value
        ) {
          throw new Error(
            `profile import replayed report mutated ${key}: ${JSON.stringify(
              replayedReportState,
            )}`,
          );
        }
      }
      for (const [key, value] of Object.entries(expectedReport)) {
        if (replayedReportState.reportSnapshot?.[key] !== value) {
          throw new Error(
            `profile import frozen report mutated ${key}: ${JSON.stringify(
              replayedReportState,
            )}`,
          );
        }
      }
      if (
        doneState.reportFrozen !== true ||
        doneState.resultSummaryFrozen !== true ||
        replayedReportState.reportFrozen !== true ||
        replayedReportState.resultSummaryFrozen !== true ||
        !Array.isArray(replayedReportState.attemptedNextPicker) ||
        !replayedReportState.attemptedNextPicker.every((attempt) => attempt.accepted === false) ||
        replayedReportState.resultText !== expectedResultText ||
        replayedReportState.nextPickerUrl !== doneState.nextPickerUrl
      ) {
        throw new Error(
          `profile import replayed report altered completion state: ${JSON.stringify(
            { doneState, replayedReportState },
          )}`,
        );
      }
      const completedCandidateById = new Map(
        doneState.completedCandidateControls.map((control) => [control.profileId, control]),
      );
      const completedConflictByPolicy = new Map(
        doneState.completedConflictControls.map((control) => [control.policy, control]),
      );
      const helperConflictByPolicy = new Map(
        completedHelperState.controls.map((control) => [control.policy, control]),
      );
      const helperCandidateById = new Map(
        completedHelperState.candidates.map((control) => [control.profileId, control]),
      );
      if (
        doneState.completedPolicy !== importConflictFlow ||
        completedHelperState.attemptedPolicy !== importConflictFlow ||
        completedHelperState.attemptedEnable !== true ||
        completedHelperState.policy !== importConflictFlow ||
        completedHelperState.importButtonDisabled !== true ||
        doneState.importButtonDisabled !== true ||
        completedCandidateById.get("prod")?.disabled !== true ||
        completedCandidateById.get("staging")?.disabled !== true ||
        helperCandidateById.get("prod")?.disabled !== true ||
        helperCandidateById.get("staging")?.disabled !== true ||
        completedConflictByPolicy.get("reject")?.disabled !== true ||
        completedConflictByPolicy.get("replace")?.disabled !== true ||
        helperConflictByPolicy.get("reject")?.disabled !== true ||
        helperConflictByPolicy.get("replace")?.disabled !== true ||
        completedConflictByPolicy.get(importConflictFlow)?.active !== true ||
        completedConflictByPolicy.get(importConflictFlow)?.pressed !== "true" ||
        helperConflictByPolicy.get(importConflictFlow)?.active !== true ||
        helperConflictByPolicy.get(importConflictFlow)?.pressed !== "true"
      ) {
        throw new Error(
          `profile import completed controls mismatch: ${JSON.stringify({
            doneState,
            completedHelperState,
          })}`,
        );
      }
      if (
        !Array.isArray(unauthorizedNextPickerState.attempts) ||
        !unauthorizedNextPickerState.attempts.every((attempt) => attempt.accepted === false)
      ) {
        throw new Error(
          `profile import accepted unauthorized next picker URL: ${JSON.stringify(
            unauthorizedNextPickerState,
          )}`,
        );
      }
      if (doneState.bootstrapToken !== "") {
        throw new Error(`profile import token was not cleared after confirmation: ${JSON.stringify(doneState)}`);
      }
      const serializedReport = JSON.stringify(doneState.report);
      for (const sensitive of profileImportSensitiveValues) {
        if (
          serializedReport.includes(sensitive) ||
          doneState.bodyText.includes(sensitive) ||
          doneState.href.includes(sensitive)
        ) {
          throw new Error(`profile import leaked sensitive value ${sensitive}: ${JSON.stringify(doneState)}`);
        }
      }
      profileImportSmoke = JSON.stringify({
        candidates: profileImportSelection.candidates,
        report: doneState.report,
      });
      if (gateway.kind === "profile-picker-import") {
        if (
          typeof doneState.report?.next_picker_url !== "string" ||
          !profilePickerPageUrlPattern.test(doneState.report.next_picker_url) ||
          doneState.nextPickerUrl !== doneState.report.next_picker_url ||
          unauthorizedNextPickerState.nextPickerUrl !== doneState.report.next_picker_url
        ) {
          throw new Error(`profile picker import did not return next picker URL: ${JSON.stringify(doneState)}`);
        }
        const nextPickerButtonState = doneState.nextPickerButton;
        if (
          !nextPickerButtonState.exists ||
          nextPickerButtonState.hidden ||
          nextPickerButtonState.disabled ||
          nextPickerButtonState.text !== "Profiles"
        ) {
          throw new Error(
            `profile picker import next picker button mismatch: ${JSON.stringify(
              nextPickerButtonState,
            )}`,
          );
        }
        if (
          unauthorizedNextPickerState.button.hidden ||
          unauthorizedNextPickerState.button.disabled ||
          replayedReportState.button.hidden ||
          replayedReportState.button.disabled ||
          unauthorizedNextPickerState.button.text !== "Profiles" ||
          replayedReportState.button.text !== "Profiles"
        ) {
          throw new Error(
            `profile picker import helper replay altered next picker button: ${JSON.stringify(
              { unauthorizedNextPickerState, replayedReportState },
            )}`,
          );
        }
        const clickedNextPicker = await page.evaluate(() => {
          const button = document.querySelector(".profile-import-next-picker");
          if (!button) {
            return false;
          }
          button.click();
          return true;
        });
        if (!clickedNextPicker) {
          throw new Error("profile picker import next picker button click did not start");
        }
        await page.waitForFunction(
          () => document.documentElement.dataset.wittySmoke === "profile_picker_ready",
          { timeout: 30000 },
        );
        if (page.url().includes("not-authorized")) {
          throw new Error(`profile picker import followed unauthorized URL: ${page.url()}`);
        }
        profilePickerSelection = await runBrowserPostImportProfilePickerReadySmoke(page, page.url());
        await assertProfilePickerBootstrapIsStale(page.url());
        const selected = await page.evaluate(async () => window.wittySelectProfile("prod"));
        if (!selected) {
          throw new Error("post-import profile picker selection did not complete");
        }
      } else if (doneState.report?.next_picker_url !== undefined) {
        throw new Error(`standalone profile import unexpectedly returned next picker URL: ${JSON.stringify(doneState)}`);
      } else if (
        !doneState.nextPickerButton.exists ||
        !doneState.nextPickerButton.hidden ||
        unauthorizedNextPickerState.nextPickerUrl !== "" ||
        !unauthorizedNextPickerState.button.hidden ||
        replayedReportState.nextPickerUrl !== "" ||
        !replayedReportState.button.hidden
      ) {
        throw new Error(
          `standalone profile import unexpectedly showed next picker button: ${JSON.stringify(
            { doneState, unauthorizedNextPickerState, replayedReportState },
          )}`,
        );
      }
    } else {
      await page.waitForFunction(
        () => document.documentElement.dataset.wittySmoke === "ok",
        { timeout: 30000 },
      );
    }
  } catch (error) {
    let pageState;
    try {
      pageState = await page.evaluate(() => ({
        smoke: document.documentElement.dataset.wittySmoke ?? "",
        status: document.getElementById("status")?.textContent ?? "",
        pickerState: window.wittyProfilePickerState ?? "",
        pickerError: window.wittyProfilePickerLastError ?? null,
        importState: window.wittyProfileImportState ?? "",
        importError: window.wittyProfileImportLastError ?? null,
      }));
    } catch (stateError) {
      pageState = {
        smoke: "unavailable-during-navigation",
        status: String(stateError?.message ?? stateError),
        url: page.url(),
      };
    }
    const details = [
      `page smoke state: ${JSON.stringify(pageState)}`,
      ...pageEvents,
    ].join("\n");
    throw new Error(`${error.message}\n${details}`);
  }

  if (profileImportOnlyModes.has(gateway.kind)) {
    const importedStore = await gateway.assertImportedStore();
    await browser.close();
    browser = null;
    const exitStatus = await gateway.waitForExit();
    console.log(
      `Witty profile import smoke completed from ${gateway.kind} picker actions ${JSON.stringify(profilePickerImportSelection?.actions ?? [])}, import ${profileImportSmoke}, wrote store ${JSON.stringify(importedStore)}, observed clean launcher exit code ${exitStatus.code}`,
    );
  } else {
  if (gateway.kind === "node" || gateway.kind === "rust") {
    await page.evaluate((targetUrl) => window.wittyConnectGateway(targetUrl), gatewayUrl);
  } else if (gateway.kind === "launcher") {
    await assertLauncherSessionConfigIsStale(url);
  } else if (gateway.kind === "profile-picker") {
    await assertProfilePickerSelectionIsStale(url, profilePickerSelection);
    await gateway.assertFakeSshArgs();
    profilePickerSmoke = JSON.stringify(profilePickerSelection.profiles);
  } else if (gateway.kind === "profile-picker-import") {
    await assertProfilePickerSelectionIsStale(page.url(), profilePickerSelection);
    await gateway.assertImportedStore();
    await gateway.assertFakeSshArgs();
    profilePickerSmoke = JSON.stringify({
      afterImport: profilePickerSelection.profiles,
      importActions: profilePickerSelection.actions,
      import: JSON.parse(profileImportSmoke),
    });
  }
  const scrollbackConfigSmoke = JSON.stringify(
    await runBrowserScrollbackConfigSmoke(
      page,
      launcherBackedGatewayModes.has(gateway.kind) ? launcherScrollbackLines : 10000,
    ),
  );
  let initialResize;
  let resizeCountBeforeManualResize;
  if (gateway.kind === "node") {
    await gateway.waitForFrame("hello", (frame) => frame.protocol === 1);
    initialResize = await gateway.waitForFrame(
      "resize",
      (frame) => frame.rows > 0 && frame.cols > 0,
    );
    resizeCountBeforeManualResize = gateway.frameCount("resize");
  } else {
    await page.waitForFunction(
      () =>
        window.wittyGatewayFrames.some(
          ({ direction, frame }) => direction === "server" && frame.type === "ready",
        ),
      { timeout: 5000 },
    );
    await page.waitForFunction(
      () => window.wittyGatewayOutputText?.includes("shell ready"),
      { timeout: 10000 },
    );
    initialResize = await page.evaluate(() =>
      window.wittyGatewayFrames.find(
        ({ direction, frame }) => direction === "client" && frame.type === "resize",
      )?.frame,
    );
    if (!initialResize || initialResize.rows <= 0 || initialResize.cols <= 0) {
      throw new Error(`invalid initial browser resize frame: ${JSON.stringify(initialResize)}`);
    }
    resizeCountBeforeManualResize = await page.evaluate(
      () =>
        window.wittyGatewayFrames.filter(
          ({ direction, frame }) => direction === "client" && frame.type === "resize",
        ).length,
    );
  }

  await page.waitForFunction(
    (expectedTitle) =>
      document.title === expectedTitle && window.wittySession?.title?.() === expectedTitle,
    gatewaySmokeTitle,
    { timeout: 5000 },
  );
  await page.waitForFunction(
    (expectedText) => window.wittySession?.screen_text?.().includes(expectedText),
    gatewayAltScreenText,
    { timeout: 5000 },
  );

  await page.locator("#witty-canvas").focus();
  await page.keyboard.type("xy");
  await page.keyboard.press("Enter");
  let inputResult;
  if (gateway.kind === "node") {
    inputResult = JSON.stringify(await gateway.waitForInputBytes([120, 121, 13]));
    await page.waitForFunction(
      () => window.wittyGatewayOutputText?.includes("gateway ws ok"),
      { timeout: 5000 },
    );
  } else {
    inputResult = launcherBackedGatewayModes.has(gateway.kind) ? `${gateway.kind} pty saw:xy` : "pty saw:xy";
    await page.waitForFunction(
      () => window.wittyGatewayOutputText?.includes("pty saw:xy"),
      { timeout: 10000 },
    );
  }
  const browserGatewayFrames = await page.evaluate(() =>
    window.wittyGatewayFrames.map(({ direction, frame }) => ({
      direction,
      type: frame.type,
    })),
  );
  if (
    !browserGatewayFrames.some(
      (entry) => entry.direction === "server" && entry.type === "output",
    )
  ) {
    throw new Error(`browser did not record gateway output: ${JSON.stringify(browserGatewayFrames)}`);
  }
  await page.waitForFunction(
    (expectedText) => window.wittySession?.screen_text?.().includes(expectedText),
    gatewayMainScreenText,
    { timeout: 5000 },
  );
  const restoredScreenText = await page.evaluate(() => window.wittySession.screen_text());
  if (restoredScreenText.includes(gatewayAltScreenText)) {
    throw new Error(`alternate-screen text leaked after restore: ${JSON.stringify(restoredScreenText)}`);
  }
  let commandPaletteSmoke = "not-run";
  const keypadSmokeBytes = JSON.stringify(await runBrowserKeypadSmoke(page));

  const resizeResult = await page.evaluate(() => {
    const canvas = document.getElementById("witty-canvas");
    canvas.style.width = "720px";
    canvas.style.height = "360px";
    return window.wittySyncCanvasSize();
  });
  if (
    resizeResult.backingWidth < 720 ||
    resizeResult.backingHeight < 360 ||
    resizeResult.gridRows <= 0 ||
    resizeResult.gridCols <= 0 ||
    resizeResult.transportGrid !== `${resizeResult.gridRows}x${resizeResult.gridCols}`
  ) {
    throw new Error(`invalid browser resize result: ${JSON.stringify(resizeResult)}`);
  }
  if (gateway.kind === "node") {
    await gateway.waitForResizeCount(resizeCountBeforeManualResize + 1);
  } else {
    await page.waitForFunction(
      (expectedCount) =>
        window.wittyGatewayFrames.filter(
          ({ direction, frame }) => direction === "client" && frame.type === "resize",
        ).length >= expectedCount,
      resizeCountBeforeManualResize + 1,
      { timeout: 5000 },
    );
  }

  const screenshot = await page.locator("#witty-canvas").screenshot({ type: "png" });
  await writeFile(canvasScreenshot, screenshot);
  const nonblank = pngHasNonblackPixel(screenshot);

  if (!nonblank) {
    throw new Error(`witty canvas rendered blank pixels in ${canvasScreenshot.pathname}`);
  }

  const functionKeySmokeBytes = JSON.stringify(await runBrowserFunctionKeySmoke(page));
  const queryReplySmoke =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserTerminalQueryReplySmoke(page))
      : "skipped-non-node-gateway";
  const shellIntegrationSmoke =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserShellIntegrationSmoke(page))
      : "skipped-non-node-gateway";
  const synchronizedOutputSmoke =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserSynchronizedOutputSmoke(page))
      : "skipped-non-node-gateway";
  const synchronizedOutputTimeoutSmoke =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserSynchronizedOutputTimeoutSmoke(page))
      : "skipped-non-node-gateway";
  const focusSmokeBytes =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserFocusSmoke(page))
      : "skipped-pty-gateway";
  let localScrollbackWheelSmoke;
  try {
    localScrollbackWheelSmoke = JSON.stringify(await runBrowserLocalScrollbackWheelSmoke(page));
  } catch (error) {
    const details = pageEvents.length > 0 ? `\n${pageEvents.join("\n")}` : "";
    throw new Error(`local scrollback wheel smoke failed: ${String(error?.stack ?? error)}${details}`);
  }
  const frameStatsSmoke = JSON.stringify(await runBrowserFrameStatsSmoke(page));
  const mouseSmokeBytes = JSON.stringify(
    gateway.kind === "node"
      ? await runBrowserMouseSmoke(page)
      : await runBrowserMouseProductSmoke(page),
  );
  const hyperlinkSmoke =
    gateway.kind === "node"
      ? await runBrowserHyperlinkSmoke(page)
      : "skipped-non-node-gateway";
  const emptyCopySmoke =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserCopyEmptySelectionSmoke(page))
      : "skipped-non-node-gateway";
  const selectionOverrideResult =
    gateway.kind === "node"
      ? await runBrowserMouseSelectionOverrideSmoke(page)
      : "skipped-non-node-gateway";
  const selectionOverrideSmoke =
    typeof selectionOverrideResult === "string"
      ? selectionOverrideResult
      : JSON.stringify(selectionOverrideResult);
  const copySelectionSmoke =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserCopySelectionSmoke(page))
      : "skipped-non-node-gateway";
  const emptyPasteSmoke =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserPasteEmptyClipboardSmoke(page))
      : "skipped-non-node-gateway";
  const pasteSmoke =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserPasteClipboardSmoke(page))
      : "skipped-non-node-gateway";
  const bracketedPasteSmoke =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserBracketedPasteClipboardSmoke(page))
      : "skipped-non-node-gateway";
  const osc52ClipboardSmoke =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserOsc52ClipboardSmoke(page))
      : "skipped-non-node-gateway";
  const selectionDisabledPolicySmoke =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserMouseSelectionDisabledPolicySmoke(page))
      : "skipped-non-node-gateway";
  const productPasteResult =
    gateway.kind === "node"
      ? "skipped-node-gateway"
      : await runBrowserPasteProductSmoke(page, gateway.kind);
  const productPasteSmoke =
    typeof productPasteResult === "string"
      ? productPasteResult
      : JSON.stringify(productPasteResult);
  const searchSmoke =
    gateway.kind === "node"
      ? JSON.stringify(await runBrowserSearchSmoke(page))
      : "skipped-non-node-gateway";
  await page.evaluate(async () => {
    await window.wittyGatewayIdle?.();
    await new Promise((resolve) => setTimeout(resolve, 150));
    await window.wittyGatewayIdle?.();
  });
  const imeSmoke = JSON.stringify(await runBrowserImeSmoke(page));
  try {
    commandPaletteSmoke = JSON.stringify(await runBrowserCommandPaletteSmoke(page, gateway.kind));
  } catch (error) {
    const details = pageEvents.length > 0 ? `\n${pageEvents.join("\n")}` : "";
    throw new Error(`command palette smoke failed: ${String(error?.stack ?? error)}${details}`);
  }
  const commandShortcutSmoke = JSON.stringify(await runBrowserCommandShortcutSmoke(page));

  let launcherLifecycleResult = "";
  if (launcherBackedGatewayModes.has(gateway.kind)) {
    await browser.close();
    browser = null;
    const exitStatus = await gateway.waitForExit();
    launcherLifecycleResult = `, and observed clean launcher exit code ${exitStatus.code} after browser close`;
  }

  console.log(
    `Witty browser smoke connected ${gateway.kind} WebSocket gateway ${launcherBackedGatewayModes.has(gateway.kind) ? url : gatewayUrl}, profile picker ${profilePickerSmoke}, scrollback config ${scrollbackConfigSmoke}, local scrollback wheel ${localScrollbackWheelSmoke}, frame stats ${frameStatsSmoke}, observed ${inputResult} after initial resize ${initialResize.rows}x${initialResize.cols}, verified gateway title and alternate-screen restore, accepted gateway output, search ${searchSmoke}, command palette ${commandPaletteSmoke}, command shortcuts ${commandShortcutSmoke}, ime ${imeSmoke}, hyperlink ${hyperlinkSmoke}, product paste ${productPasteSmoke}, keypad bytes ${keypadSmokeBytes}, function-key bytes ${functionKeySmokeBytes}, terminal query replies ${queryReplySmoke}, shell integration ${shellIntegrationSmoke}, synchronized output ${synchronizedOutputSmoke}, synchronized timeout ${synchronizedOutputTimeoutSmoke}, focus bytes ${focusSmokeBytes}, mouse bytes ${mouseSmokeBytes}, empty copy ${emptyCopySmoke}, selection override ${selectionOverrideSmoke}, copy selection ${copySelectionSmoke}, empty paste ${emptyPasteSmoke}, paste ${pasteSmoke}, bracketed paste ${bracketedPasteSmoke}, osc52 clipboard ${osc52ClipboardSmoke}, disabled selection policy ${selectionDisabledPolicySmoke}, resized canvas, and rendered a nonblank canvas at ${canvasScreenshot.pathname}${launcherLifecycleResult}`,
  );
  }
} finally {
  if (browser) {
    await browser.close();
  }
  gateway.stop();
  stopServer();
}
