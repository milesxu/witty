import init, {
  witty_browser_keyboard_protocol_diagnostic_report_json,
  witty_create_session,
  witty_web_mock_replay_glyph_chars,
  witty_web_session_written_bytes,
} from "./pkg/witty_web.js";

const status = document.getElementById("status");
const gatewayFrames = [];
let gatewaySocket = null;
const DEFAULT_MOUSE_SELECTION_OVERRIDE_POLICY = "shift-select";
const DEFAULT_OSC52_CLIPBOARD_POLICY = "disabled";
const DEFAULT_SCROLLBACK_LINES = 10000;
const COMMAND_PALETTE_VISIBLE_ITEM_LIMIT = 3;
const SYNCHRONIZED_OUTPUT_TIMEOUT_MS = 150;
const COMMAND_BLOCK_ACTION_MENU_COMMAND_ID = "witty.command_block.actions";
const COMMAND_BLOCK_COPY_COMMAND_ID = "witty.command_block.copy_command";
const COMMAND_BLOCK_COPY_OUTPUT_ID = "witty.command_block.copy_output";
const authorizedProfileImportReports = new WeakSet();

function parseMouseSelectionOverridePolicy(value) {
  if (value === undefined || value === null || value === "") {
    return DEFAULT_MOUSE_SELECTION_OVERRIDE_POLICY;
  }
  if (value === "shift-select" || value === "disabled") {
    return value;
  }
  throw new Error(`unsupported mouse selection override policy: ${value}`);
}

function parseOsc52ClipboardPolicy(value) {
  if (value === undefined || value === null || value === "") {
    return DEFAULT_OSC52_CLIPBOARD_POLICY;
  }
  if (value === "disabled" || value === "confirm" || value === "allow") {
    return value;
  }
  throw new Error(`unsupported OSC 52 clipboard policy: ${value}`);
}

function parseScrollbackLines(value) {
  if (value === undefined || value === null || value === "") {
    return DEFAULT_SCROLLBACK_LINES;
  }
  const lines = Number(value);
  if (!Number.isSafeInteger(lines) || lines < 0 || lines > 0xffffffff) {
    throw new Error(`unsupported scrollback line limit: ${value}`);
  }
  return lines;
}

function setStatus(state, message) {
  document.documentElement.dataset.wittySmoke = state;
  status.dataset.smoke = state;
  status.textContent = message;
}

function browserClipboard() {
  return window.wittyClipboardApi ?? navigator.clipboard;
}

function isCommandBlockCopyCommand(commandId) {
  return (
    commandId === COMMAND_BLOCK_COPY_COMMAND_ID ||
    commandId === COMMAND_BLOCK_COPY_OUTPUT_ID
  );
}

function isCommandBlockActionMenuCommand(commandId) {
  return commandId === COMMAND_BLOCK_ACTION_MENU_COMMAND_ID;
}

function ensureImeInput(canvas) {
  let input = document.getElementById("witty-ime-input");
  if (!input) {
    input = document.createElement("input");
    input.id = "witty-ime-input";
    input.autocomplete = "off";
    input.spellcheck = false;
    canvas.insertAdjacentElement("afterend", input);
  }
  input.type = "text";
  input.setAttribute("aria-hidden", "true");
  input.setAttribute("autocapitalize", "off");
  input.setAttribute("autocorrect", "off");
  input.inputMode = "text";
  input.enterKeyHint = "done";
  return input;
}

function canvasLayout(canvas) {
  const rect = canvas.getBoundingClientRect();
  const devicePixelRatio = window.devicePixelRatio || 1;
  const cssWidth = Math.max(1, rect.width || canvas.clientWidth || canvas.width);
  const cssHeight = Math.max(1, rect.height || canvas.clientHeight || canvas.height);
  canvas.width = Math.max(1, Math.ceil(cssWidth * devicePixelRatio));
  canvas.height = Math.max(1, Math.ceil(cssHeight * devicePixelRatio));
  return { cssWidth, cssHeight, devicePixelRatio };
}

function syncImeInputPosition(session, canvas, input) {
  if (!input) {
    return null;
  }

  const cursorRect = JSON.parse(session.ime_cursor_rect_json());
  const canvasRect = canvas.getBoundingClientRect();
  const parentRect =
    input.offsetParent?.getBoundingClientRect() ?? document.body.getBoundingClientRect();
  const requestedLeft = canvasRect.left - parentRect.left + cursorRect.left;
  const requestedTop = canvasRect.top - parentRect.top + cursorRect.top;
  const width = Math.max(1, cursorRect.width);
  const height = Math.max(1, cursorRect.height);
  const clamped = clampImeInputRect(requestedLeft, requestedTop, width, height, parentRect);
  const left = clamped.left;
  const top = clamped.top;
  input.style.left = `${left}px`;
  input.style.top = `${top}px`;
  input.style.width = `${width}px`;
  input.style.height = `${height}px`;
  const rect = {
    left,
    top,
    width,
    height,
    requestedLeft,
    requestedTop,
    clamped: clamped.clamped,
    target: cursorRect.target,
  };
  window.wittyLastImeCursorRect = rect;
  return rect;
}

function clampImeInputRect(left, top, width, height, parentRect) {
  const viewport = window.visualViewport;
  const viewportLeft = (viewport?.offsetLeft ?? 0) - parentRect.left;
  const viewportTop = (viewport?.offsetTop ?? 0) - parentRect.top;
  const viewportWidth = viewport?.width ?? window.innerWidth;
  const viewportHeight = viewport?.height ?? window.innerHeight;
  const maxLeft = Math.max(viewportLeft, viewportLeft + viewportWidth - width);
  const maxTop = Math.max(viewportTop, viewportTop + viewportHeight - height);
  const clampedLeft = Math.min(Math.max(left, viewportLeft), maxLeft);
  const clampedTop = Math.min(Math.max(top, viewportTop), maxTop);
  return {
    left: clampedLeft,
    top: clampedTop,
    clamped: clampedLeft !== left || clampedTop !== top,
  };
}

function focusTerminalInput(session, canvas, input) {
  syncImeInputPosition(session, canvas, input);
  input.focus({ preventScroll: true });
}

function sendGatewayFrame(json) {
  if (!gatewaySocket || gatewaySocket.readyState !== WebSocket.OPEN) {
    return null;
  }

  const frame = JSON.parse(json);
  gatewayFrames.push({ direction: "client", frame });
  gatewaySocket.send(json);
  return frame;
}

function flushGatewayInput(session) {
  const json = session.drain_outbound_message_json();
  if (!json) {
    return null;
  }
  return sendGatewayFrame(json);
}

function sendGatewayResize(session) {
  return sendGatewayFrame(session.resize_message_json());
}

function hashParam(name) {
  const hash = window.location.hash.startsWith("#")
    ? window.location.hash.slice(1)
    : window.location.hash;
  if (!hash) {
    return "";
  }
  const entries = [...new URLSearchParams(hash).entries()];
  const matches = entries.filter((entry) => entry[0] === name);
  if (matches.length === 0) {
    return "";
  }
  if (matches.length !== 1 || entries.length !== 1) {
    throw new Error(`launcher hash is invalid for ${name}`);
  }
  return matches[0][1] ?? "";
}

function sessionIdFromHash() {
  return hashParam("session");
}

function profilePickerIdFromHash() {
  return hashParam("profile_picker");
}

function profileImportIdFromHash() {
  return hashParam("profile_import");
}

function validLauncherSessionId(id) {
  return typeof id === "string" && /^[0-9a-f]{32}$/.test(id);
}

function validLauncherToken(token) {
  return typeof token === "string" && /^[0-9a-f]{64}$/.test(token);
}

function validProfilePickerPageUrl(url) {
  if (typeof url !== "string" || !url.startsWith("/index.html#")) {
    return false;
  }
  const hash = url.slice("/index.html#".length);
  const entries = [...new URLSearchParams(hash).entries()];
  return (
    entries.length === 1 &&
    entries[0][0] === "profile_picker" &&
    validLauncherSessionId(entries[0][1])
  );
}

function validProfileImportPageUrl(url) {
  if (typeof url !== "string" || !url.startsWith("/index.html#")) {
    return false;
  }
  const hash = url.slice("/index.html#".length);
  const entries = [...new URLSearchParams(hash).entries()];
  return (
    entries.length === 1 &&
    entries[0][0] === "profile_import" &&
    validLauncherSessionId(entries[0][1])
  );
}

function openProfilePickerUrl(url) {
  if (!validProfilePickerPageUrl(url)) {
    throw new Error("profile picker URL is invalid");
  }
  window.history.replaceState(null, "", url);
  window.location.reload();
  return true;
}

async function loadSessionConfigFromHash() {
  const sessionId = sessionIdFromHash();
  if (!sessionId) {
    return null;
  }
  if (!validLauncherSessionId(sessionId)) {
    throw new Error("launcher session id is invalid");
  }
  const response = await fetch(`/session/${encodeURIComponent(sessionId)}.json`, {
    cache: "no-store",
  });
  if (!response.ok) {
    throw new Error(`failed to load session config: ${response.status}`);
  }
  const config = await response.json();
  return normalizeSessionConfig(config);
}

function normalizeSessionConfig(config) {
  if (!validRecord(config)) {
    throw new Error("session config is missing");
  }
  assertKnownFields("session config", config, [
    "protocol",
    "gateway_url",
    "token",
    "mouse_selection_override",
    "scrollback_lines",
    "expires_at_ms",
  ]);
  if (config.protocol !== 1) {
    throw new Error(`unsupported gateway protocol: ${config.protocol}`);
  }
  if (!validLauncherGatewayUrl(config.gateway_url)) {
    throw new Error("session config has invalid gateway_url");
  }
  if (!validLauncherToken(config.token)) {
    throw new Error("session config is missing token");
  }
  if (!validNonNegativeSafeInteger(config.expires_at_ms)) {
    throw new Error("session config has invalid expires_at_ms");
  }
  config.mouse_selection_override = parseMouseSelectionOverridePolicy(
    config.mouse_selection_override,
  );
  config.scrollback_lines = parseScrollbackLines(config.scrollback_lines);
  return config;
}

function validLoopbackHostname(hostname) {
  if (hostname === "[::1]") {
    return true;
  }
  const parts = String(hostname ?? "").split(".");
  return (
    parts.length === 4 &&
    parts[0] === "127" &&
    parts.every((part) => {
      if (!/^[0-9]{1,3}$/.test(part)) {
        return false;
      }
      const value = Number(part);
      return Number.isInteger(value) && value >= 0 && value <= 255;
    })
  );
}

function validLauncherGatewayUrl(url) {
  if (typeof url !== "string" || url.length === 0) {
    return false;
  }
  try {
    const parsed = new URL(url);
    return (
      parsed.protocol === "ws:" &&
      validLoopbackHostname(parsed.hostname) &&
      parsed.pathname === "/witty" &&
      parsed.search === "" &&
      parsed.hash === "" &&
      parsed.username === "" &&
      parsed.password === "" &&
      parsed.port !== ""
    );
  } catch {
    return false;
  }
}

function validProfilePickerActionUrl(url, pickerId, action) {
  return (
    validLauncherSessionId(pickerId) &&
    typeof url === "string" &&
    url === `/profile-picker/${pickerId}/${action}`
  );
}

function validProfileImportConfirmUrl(url, importId) {
  return (
    validLauncherSessionId(importId) &&
    typeof url === "string" &&
    url === `/profile-import/${importId}/confirm`
  );
}

function validRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function assertKnownFields(label, value, allowedFields) {
  const allowed = new Set(allowedFields);
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) {
      throw new Error(`${label} has unsupported field ${key}`);
    }
  }
}

function validNonNegativeSafeInteger(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

function hasControlCharacter(value) {
  return /[\u0000-\u001f\u007f]/.test(value);
}

function validDisplayText(value) {
  return typeof value === "string" && value.trim().length > 0 && !hasControlCharacter(value);
}

function validDisplayAtom(value) {
  return (
    typeof value === "string" &&
    value.trim().length > 0 &&
    !/\s/.test(value) &&
    !hasControlCharacter(value)
  );
}

function validateDisplayTags(label, tags) {
  if (!Array.isArray(tags)) {
    throw new Error(`${label} is missing tags`);
  }
  for (const tag of tags) {
    if (!validDisplayAtom(tag)) {
      throw new Error(`${label} has an invalid tag`);
    }
  }
}

function validateProfilePickerSummary(summary) {
  if (!validRecord(summary)) {
    throw new Error("profile picker summary is missing");
  }
  assertKnownFields("profile picker summary", summary, [
    "profiles",
    "default_profile_id",
    "launchable_profiles",
    "credential_resolver_required_profiles",
  ]);
  if (!Array.isArray(summary.profiles)) {
    throw new Error("profile picker summary is missing profiles");
  }
  if (!validNonNegativeSafeInteger(summary.launchable_profiles)) {
    throw new Error("profile picker summary has invalid launchable count");
  }
  if (!validNonNegativeSafeInteger(summary.credential_resolver_required_profiles)) {
    throw new Error("profile picker summary has invalid credential count");
  }
  if (
    summary.default_profile_id !== null &&
    !validDisplayAtom(summary.default_profile_id)
  ) {
    throw new Error("profile picker summary has invalid default profile id");
  }

  const ids = new Set();
  let launchableProfiles = 0;
  let credentialResolverRequiredProfiles = 0;
  let defaultProfiles = 0;
  let defaultProfileId = "";
  for (const profile of summary.profiles) {
    if (!validRecord(profile)) {
      throw new Error("profile picker summary contains an invalid profile");
    }
    assertKnownFields("profile picker profile", profile, [
      "id",
      "name",
      "tags",
      "launchability",
      "is_default",
    ]);
    if (!validDisplayAtom(profile.id)) {
      throw new Error("profile picker profile is missing id");
    }
    if (ids.has(profile.id)) {
      throw new Error("profile picker summary contains duplicate profile ids");
    }
    ids.add(profile.id);
    if (!validDisplayText(profile.name)) {
      throw new Error("profile picker profile is missing name");
    }
    validateDisplayTags("profile picker profile", profile.tags);
    if (
      profile.launchability !== "launchable" &&
      profile.launchability !== "requires_credential_resolver"
    ) {
      throw new Error("profile picker profile has unsupported launchability");
    }
    if (typeof profile.is_default !== "boolean") {
      throw new Error("profile picker profile has invalid default flag");
    }
    if (profile.launchability === "launchable") {
      launchableProfiles += 1;
    } else {
      credentialResolverRequiredProfiles += 1;
    }
    if (profile.is_default) {
      defaultProfiles += 1;
      defaultProfileId = profile.id;
    }
  }

  if (launchableProfiles !== summary.launchable_profiles) {
    throw new Error("profile picker summary launchable count mismatch");
  }
  if (
    credentialResolverRequiredProfiles !==
    summary.credential_resolver_required_profiles
  ) {
    throw new Error("profile picker summary credential count mismatch");
  }
  if (summary.default_profile_id === null) {
    if (defaultProfiles !== 0) {
      throw new Error("profile picker summary default flag mismatch");
    }
  } else if (
    !ids.has(summary.default_profile_id) ||
    defaultProfiles !== 1 ||
    defaultProfileId !== summary.default_profile_id
  ) {
    throw new Error("profile picker summary default id mismatch");
  }
}

function normalizeProfilePickerImportActions(bootstrap, pickerId) {
  const actions = bootstrap.import_actions ?? [];
  if (!Array.isArray(actions)) {
    throw new Error("profile picker import actions are invalid");
  }
  if (actions.length === 0) {
    if (bootstrap.import_url !== undefined && bootstrap.import_url !== null) {
      throw new Error("profile picker bootstrap has import_url without import actions");
    }
    bootstrap.import_url = "";
    return [];
  }
  if (!validProfilePickerActionUrl(bootstrap.import_url, pickerId, "import")) {
    throw new Error("profile picker bootstrap is missing import_url");
  }
  const ids = new Set();
  return actions.map((action) => {
    if (!validRecord(action)) {
      throw new Error("profile picker import action is invalid");
    }
    assertKnownFields("profile picker import action", action, ["id", "kind", "label"]);
    if (!validDisplayAtom(action.id)) {
      throw new Error("profile picker import action is missing id");
    }
    if (ids.has(action.id)) {
      throw new Error("profile picker import actions contain duplicate ids");
    }
    ids.add(action.id);
    if (action.kind !== "openssh_config") {
      throw new Error("profile picker import action has unsupported kind");
    }
    if (!validDisplayText(action.label)) {
      throw new Error("profile picker import action is missing label");
    }
    return {
      id: action.id,
      kind: action.kind,
      label: action.label,
    };
  });
}

async function loadProfilePickerBootstrapFromHash() {
  const pickerId = profilePickerIdFromHash();
  if (!pickerId) {
    return null;
  }
  if (!validLauncherSessionId(pickerId)) {
    throw new Error("profile picker id is invalid");
  }
  const response = await fetch(`/profile-picker/${encodeURIComponent(pickerId)}.json`, {
    cache: "no-store",
  });
  if (!response.ok) {
    throw new Error(`failed to load profile picker: ${response.status}`);
  }
  const bootstrap = await response.json();
  if (!validRecord(bootstrap)) {
    throw new Error("profile picker bootstrap is missing");
  }
  assertKnownFields("profile picker bootstrap", bootstrap, [
    "kind",
    "protocol",
    "ui_token",
    "selection_url",
    "import_url",
    "expires_at_ms",
    "summary",
    "import_actions",
  ]);
  if (bootstrap.kind !== "profile_picker") {
    throw new Error(`unsupported launcher bootstrap kind: ${bootstrap.kind}`);
  }
  if (bootstrap.protocol !== 1) {
    throw new Error(`unsupported gateway protocol: ${bootstrap.protocol}`);
  }
  if (!validLauncherToken(bootstrap.ui_token)) {
    throw new Error("profile picker bootstrap is missing ui_token");
  }
  if (!validNonNegativeSafeInteger(bootstrap.expires_at_ms)) {
    throw new Error("profile picker bootstrap has invalid expires_at_ms");
  }
  if (!validProfilePickerActionUrl(bootstrap.selection_url, pickerId, "select")) {
    throw new Error("profile picker bootstrap is missing selection_url");
  }
  validateProfilePickerSummary(bootstrap.summary);
  bootstrap.import_actions = normalizeProfilePickerImportActions(bootstrap, pickerId);
  return bootstrap;
}

function validateProfileImportReview(review) {
  if (!validRecord(review)) {
    throw new Error("profile import review is missing");
  }
  assertKnownFields("profile import review", review, [
    "candidates",
    "selected_by_default",
    "warning_count",
    "global_warning_count",
    "conflict_count",
  ]);
  if (!Array.isArray(review.candidates)) {
    throw new Error("profile import review is missing candidates");
  }
  if (!Array.isArray(review.selected_by_default)) {
    throw new Error("profile import review is missing default selection");
  }
  if (!validNonNegativeSafeInteger(review.warning_count)) {
    throw new Error("profile import review has invalid warning count");
  }
  if (!validNonNegativeSafeInteger(review.global_warning_count)) {
    throw new Error("profile import review has invalid global warning count");
  }
  if (!validNonNegativeSafeInteger(review.conflict_count)) {
    throw new Error("profile import review has invalid conflict count");
  }

  const idCounts = new Map();
  const candidateById = new Map();
  let candidateWarningCount = 0;
  let conflictCount = 0;
  for (const candidate of review.candidates) {
    if (!validRecord(candidate)) {
      throw new Error("profile import review contains an invalid candidate");
    }
    assertKnownFields("profile import candidate", candidate, [
      "id",
      "name",
      "tags",
      "warning_count",
      "has_conflict",
    ]);
    if (!validDisplayAtom(candidate.id)) {
      throw new Error("profile import candidate is missing id");
    }
    if (!validDisplayText(candidate.name)) {
      throw new Error("profile import candidate is missing name");
    }
    validateDisplayTags("profile import candidate", candidate.tags);
    if (
      !Number.isSafeInteger(candidate.warning_count) ||
      candidate.warning_count < 0 ||
      typeof candidate.has_conflict !== "boolean"
    ) {
      throw new Error("profile import candidate has invalid state");
    }
    idCounts.set(candidate.id, (idCounts.get(candidate.id) ?? 0) + 1);
    candidateById.set(candidate.id, candidate);
    candidateWarningCount += candidate.warning_count;
    if (candidate.has_conflict) {
      conflictCount += 1;
    }
  }
  if (review.warning_count !== candidateWarningCount + review.global_warning_count) {
    throw new Error("profile import review warning count mismatch");
  }
  if (review.conflict_count !== conflictCount) {
    throw new Error("profile import review conflict count mismatch");
  }
  const selectedIds = new Set();
  for (const id of review.selected_by_default) {
    if (!validDisplayAtom(id)) {
      throw new Error("profile import default selection contains an invalid id");
    }
    if (selectedIds.has(id)) {
      throw new Error("profile import default selection contains duplicate ids");
    }
    selectedIds.add(id);
    if ((idCounts.get(id) ?? 0) !== 1) {
      throw new Error("profile import default selection contains unknown or duplicate ids");
    }
    if (candidateById.get(id)?.has_conflict) {
      throw new Error("profile import default selection contains a conflict");
    }
  }
}

function profileImportConfirmRequestAllowed(bootstrap, profileIds, conflict) {
  if (conflict !== "reject" && conflict !== "replace") {
    return false;
  }
  if (!Array.isArray(profileIds) || profileIds.length === 0) {
    return false;
  }

  const idCounts = new Map();
  const candidateById = new Map();
  for (const candidate of bootstrap.review.candidates) {
    idCounts.set(candidate.id, (idCounts.get(candidate.id) ?? 0) + 1);
    candidateById.set(candidate.id, candidate);
  }

  const selectedIds = new Set();
  for (const id of profileIds) {
    if (!validDisplayAtom(id) || selectedIds.has(id)) {
      return false;
    }
    selectedIds.add(id);
    if ((idCounts.get(id) ?? 0) !== 1) {
      return false;
    }
    if (conflict === "reject" && candidateById.get(id)?.has_conflict) {
      return false;
    }
  }
  return true;
}

function validateProfileImportConfirmReport(report) {
  if (!validRecord(report)) {
    throw new Error("profile import report is missing");
  }
  assertKnownFields("profile import report", report, [
    "changed",
    "profiles",
    "default_changed",
    "bytes",
    "created_parent_dir",
    "selected",
    "added",
    "replaced",
    "warning_count",
    "global_warning_count",
    "next_picker_url",
  ]);
  if (
    typeof report.changed !== "boolean" ||
    typeof report.default_changed !== "boolean" ||
    typeof report.created_parent_dir !== "boolean"
  ) {
    throw new Error("profile import report has invalid boolean fields");
  }
  for (const key of [
    "profiles",
    "bytes",
    "selected",
    "added",
    "replaced",
    "warning_count",
    "global_warning_count",
  ]) {
    if (!validNonNegativeSafeInteger(report[key])) {
      throw new Error(`profile import report has invalid ${key}`);
    }
  }
  if (report.selected !== report.added + report.replaced) {
    throw new Error("profile import report selected count mismatch");
  }
  if (report.warning_count < report.global_warning_count) {
    throw new Error("profile import report warning count mismatch");
  }
  if (
    report.next_picker_url !== undefined &&
    !validProfilePickerPageUrl(report.next_picker_url)
  ) {
    throw new Error("profile import report has invalid next picker URL");
  }
  return report;
}

function validateProfilePickerImportEntry(entry) {
  if (!validRecord(entry)) {
    throw new Error("profile picker import entry is missing");
  }
  assertKnownFields("profile picker import entry", entry, [
    "kind",
    "protocol",
    "import_url",
  ]);
  if (
    entry.kind !== "profile_import_entry" ||
    entry.protocol !== 1 ||
    !validProfileImportPageUrl(entry.import_url)
  ) {
    throw new Error("profile picker import response is invalid");
  }
  return entry;
}

async function loadProfileImportBootstrapFromHash() {
  const importId = profileImportIdFromHash();
  if (!importId) {
    return null;
  }
  if (!validLauncherSessionId(importId)) {
    throw new Error("profile import id is invalid");
  }
  const response = await fetch(`/profile-import/${encodeURIComponent(importId)}.json`, {
    cache: "no-store",
  });
  if (!response.ok) {
    throw new Error(`failed to load profile import review: ${response.status}`);
  }
  const bootstrap = await response.json();
  if (!validRecord(bootstrap)) {
    throw new Error("profile import bootstrap is missing");
  }
  assertKnownFields("profile import bootstrap", bootstrap, [
    "kind",
    "protocol",
    "ui_token",
    "confirm_url",
    "expires_at_ms",
    "review",
  ]);
  if (bootstrap.kind !== "profile_import") {
    throw new Error(`unsupported launcher bootstrap kind: ${bootstrap.kind}`);
  }
  if (bootstrap.protocol !== 1) {
    throw new Error(`unsupported gateway protocol: ${bootstrap.protocol}`);
  }
  if (!validLauncherToken(bootstrap.ui_token)) {
    throw new Error("profile import bootstrap is missing ui_token");
  }
  if (!validNonNegativeSafeInteger(bootstrap.expires_at_ms)) {
    throw new Error("profile import bootstrap has invalid expires_at_ms");
  }
  if (!validProfileImportConfirmUrl(bootstrap.confirm_url, importId)) {
    throw new Error("profile import bootstrap is missing confirm_url");
  }
  validateProfileImportReview(bootstrap.review);
  return bootstrap;
}

function gatewayUrlFromSessionConfig(config) {
  const url = new URL(config.gateway_url);
  url.searchParams.set("token", config.token);
  return url.toString();
}

function profilePickerLaunchable(profile) {
  return profile.launchability === "launchable";
}

function profilePickerSelectionAllowed(bootstrap, profileId) {
  return bootstrap.summary.profiles.some(
    (profile) => profile.id === profileId && profilePickerLaunchable(profile),
  );
}

function profilePickerImportActionAllowed(bootstrap, actionId) {
  return (
    typeof bootstrap.import_url === "string" &&
    bootstrap.import_url.length > 0 &&
    bootstrap.import_actions.some((action) => action.id === actionId)
  );
}

function profilePickerLabel(profile) {
  const parts = [profile.name || profile.id];
  if (profile.is_default) {
    parts.push("default");
  }
  if (!profilePickerLaunchable(profile)) {
    parts.push("needs credentials");
  }
  return parts.join(" - ");
}

function removeProfilePickerElement() {
  const picker = document.getElementById("witty-profile-picker");
  if (picker) {
    picker.remove();
  }
  document.body.dataset.wittyPicker = "inactive";
}

function exposeProfilePickerBootstrap(bootstrap) {
  window.wittyProfilePickerBootstrap = Object.freeze({
    kind: bootstrap.kind,
    protocol: bootstrap.protocol,
    selection_url: bootstrap.selection_url,
    import_url: typeof bootstrap.import_url === "string" ? bootstrap.import_url : "",
    ui_token: bootstrap.ui_token,
  });
  return window.wittyProfilePickerBootstrap;
}

function renderProfilePicker(bootstrap, selectProfile, startImport) {
  removeProfilePickerElement();
  document.body.dataset.wittyPicker = "active";
  exposeProfilePickerBootstrap(bootstrap);
  window.wittyProfilePickerState = "profile_picker_ready";
  window.wittyProfilePickerLastError = null;
  window.wittyProfilePickerProfiles = Object.freeze(
    bootstrap.summary.profiles.map((profile) =>
      Object.freeze({
        id: profile.id,
        name: profile.name,
        tags: Object.freeze([...profile.tags]),
        launchability: profile.launchability,
        isDefault: Boolean(profile.is_default),
      }),
    ),
  );
  window.wittyProfilePickerImportActions = Object.freeze(
    bootstrap.import_actions.map((action) =>
      Object.freeze({
        id: action.id,
        kind: action.kind,
        label: action.label,
      }),
    ),
  );

  const picker = document.createElement("section");
  picker.id = "witty-profile-picker";
  picker.setAttribute("aria-label", "SSH profiles");

  const header = document.createElement("div");
  header.className = "profile-picker-header";
  const title = document.createElement("h1");
  title.textContent = "SSH Profiles";
  const count = document.createElement("span");
  count.className = "profile-picker-count";
  count.textContent = `${bootstrap.summary.launchable_profiles} ready`;
  header.append(title, count);
  picker.append(header);

  const list = document.createElement("div");
  list.className = "profile-picker-list";
  const buttons = [];
  const launchableProfileIds = new Set();
  for (const profile of bootstrap.summary.profiles) {
    if (profilePickerLaunchable(profile)) {
      launchableProfileIds.add(profile.id);
    }
    const button = document.createElement("button");
    button.type = "button";
    button.className = "profile-picker-option";
    button.dataset.profileId = profile.id;
    button.disabled = !profilePickerLaunchable(profile);
    button.setAttribute("aria-label", profilePickerLabel(profile));

    const main = document.createElement("span");
    main.className = "profile-picker-option-main";
    const name = document.createElement("span");
    name.className = "profile-picker-name";
    name.textContent = profile.name;
    const meta = document.createElement("span");
    meta.className = "profile-picker-meta";
    const tags = profile.tags.length > 0 ? profile.tags.join(" ") : profile.id;
    meta.textContent = profile.is_default ? `${tags} default` : tags;
    main.append(name, meta);

    const state = document.createElement("span");
    state.className = "profile-picker-state";
    state.textContent = profilePickerLaunchable(profile) ? "Launch" : "Locked";
    button.append(main, state);
    button.addEventListener("click", () => selectProfile(profile.id));
    buttons.push(button);
    list.append(button);
  }

  if (buttons.length === 0) {
    const empty = document.createElement("p");
    empty.className = "profile-picker-empty";
    empty.textContent = "No profiles";
    list.append(empty);
  }

  const actionButtons = [];
  const actions = document.createElement("div");
  actions.className = "profile-picker-actions";
  for (const action of bootstrap.import_actions) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "profile-picker-import-action";
    button.dataset.actionId = action.id;
    button.textContent = action.label;
    button.addEventListener("click", () => startImport(action.id));
    actionButtons.push(button);
    actions.append(button);
  }

  const message = document.createElement("div");
  message.className = "profile-picker-message";
  message.setAttribute("role", "status");

  picker.append(list);
  if (actionButtons.length > 0) {
    picker.append(actions);
  }
  picker.append(message);
  document.querySelector("main").prepend(picker);

  const firstLaunchable = buttons.find((button) => !button.disabled);
  if (firstLaunchable) {
    firstLaunchable.focus();
  }

  window.wittySetProfilePickerMessage = (text) => {
    message.textContent = String(text ?? "");
    return message.textContent;
  };
  window.wittySetProfilePickerDisabled = (disabled) => {
    for (const button of buttons) {
      if (launchableProfileIds.has(button.dataset.profileId)) {
        button.disabled = disabled;
      }
    }
    for (const button of actionButtons) {
      button.disabled = disabled;
    }
    return disabled;
  };
  return picker;
}

function removeProfileImportElement() {
  const element = document.getElementById("witty-profile-import");
  if (element) {
    element.remove();
  }
  document.body.dataset.wittyImport = "inactive";
}

function exposeProfileImportBootstrap(bootstrap) {
  window.wittyProfileImportBootstrap = Object.freeze({
    kind: bootstrap.kind,
    protocol: bootstrap.protocol,
    confirm_url: bootstrap.confirm_url,
    ui_token: bootstrap.ui_token,
  });
  return window.wittyProfileImportBootstrap;
}

function renderProfileImportReview(bootstrap, confirmImport) {
  removeProfileImportElement();
  document.body.dataset.wittyImport = "active";
  exposeProfileImportBootstrap(bootstrap);
  window.wittyProfileImportState = "profile_import_ready";
  window.wittyProfileImportLastError = null;
  window.wittyProfileImportCandidates = Object.freeze(
    bootstrap.review.candidates.map((candidate) =>
      Object.freeze({
        id: candidate.id,
        name: candidate.name,
        tags: Object.freeze([...candidate.tags]),
        warningCount: candidate.warning_count,
        hasConflict: Boolean(candidate.has_conflict),
      }),
    ),
  );
  window.wittyProfileImportReviewSummary = Object.freeze({
    candidateCount: bootstrap.review.candidates.length,
    warningCount: bootstrap.review.warning_count,
    globalWarningCount: bootstrap.review.global_warning_count,
    conflictCount: bootstrap.review.conflict_count,
  });
  window.wittyProfileImportReport = null;
  window.wittyProfileImportResultSummary = null;
  window.wittyProfileImportNextPickerUrl = "";
  window.wittyOpenNextProfilePicker = null;

  const selectedIds = new Set(bootstrap.review.selected_by_default);
  const panel = document.createElement("section");
  panel.id = "witty-profile-import";
  panel.setAttribute("aria-label", "OpenSSH import review");

  const header = document.createElement("div");
  header.className = "profile-picker-header";
  const title = document.createElement("h1");
  title.textContent = "OpenSSH Import";
  const count = document.createElement("span");
  count.className = "profile-picker-count";
  count.textContent = `${bootstrap.review.candidates.length} profiles`;
  header.append(title, count);
  panel.append(header);

  const summary = document.createElement("div");
  summary.className = "profile-import-summary";
  for (const item of [
    `${bootstrap.review.warning_count} warnings`,
    `${bootstrap.review.conflict_count} conflicts`,
    `${bootstrap.review.global_warning_count} global`,
  ]) {
    const element = document.createElement("span");
    element.className = "profile-import-summary-item";
    element.textContent = item;
    summary.append(element);
  }
  panel.append(summary);

  const list = document.createElement("div");
  list.className = "profile-picker-list";
  const checkboxes = [];
  for (const candidate of bootstrap.review.candidates) {
    const label = document.createElement("label");
    label.className = "profile-import-option";
    label.dataset.profileId = candidate.id;

    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = selectedIds.has(candidate.id);
    checkbox.dataset.profileId = candidate.id;
    checkbox.dataset.hasConflict = candidate.has_conflict ? "true" : "false";
    checkboxes.push(checkbox);

    const main = document.createElement("span");
    main.className = "profile-picker-option-main";
    const name = document.createElement("span");
    name.className = "profile-picker-name";
    name.textContent = candidate.name;
    const meta = document.createElement("span");
    meta.className = "profile-picker-meta";
    const tags = candidate.tags.length > 0 ? candidate.tags.join(" ") : candidate.id;
    const states = [];
    if (candidate.has_conflict) {
      states.push("conflict");
    }
    if (candidate.warning_count > 0) {
      states.push(`${candidate.warning_count} warnings`);
    }
    meta.textContent = [tags, ...states].join(" - ");
    main.append(name, meta);

    label.append(checkbox, main);
    list.append(label);
  }

  if (checkboxes.length === 0) {
    const empty = document.createElement("p");
    empty.className = "profile-picker-empty";
    empty.textContent = "No profiles";
    list.append(empty);
  }

  const controls = document.createElement("div");
  controls.className = "profile-import-controls";
  const conflict = document.createElement("div");
  conflict.className = "profile-import-conflict";
  conflict.setAttribute("role", "group");
  conflict.setAttribute("aria-label", "Conflict policy");
  const conflictButtons = [];
  let importDisabled = false;
  let importLocked = false;
  let allowedNextPickerUrl = "";
  let acceptedReportSummary = null;
  let selectedConflictPolicy = "reject";
  for (const value of ["reject", "replace"]) {
    const option = document.createElement("button");
    option.type = "button";
    option.className = "profile-import-conflict-option";
    option.dataset.conflictPolicy = value;
    option.textContent = value;
    option.addEventListener("click", () => setConflictPolicy(value));
    conflictButtons.push(option);
    conflict.append(option);
  }
  const button = document.createElement("button");
  button.type = "button";
  button.className = "profile-import-confirm";
  button.textContent = "Import";
  const nextPickerButton = document.createElement("button");
  nextPickerButton.type = "button";
  nextPickerButton.className = "profile-import-next-picker";
  nextPickerButton.textContent = "Profiles";
  nextPickerButton.hidden = true;
  controls.append(conflict, button, nextPickerButton);

  const message = document.createElement("div");
  message.className = "profile-picker-message";
  message.setAttribute("role", "status");
  const resultSummary = document.createElement("div");
  resultSummary.className = "profile-import-result";
  resultSummary.setAttribute("aria-label", "Import result");
  resultSummary.setAttribute("aria-live", "polite");
  resultSummary.hidden = true;

  function selectedProfileIds() {
    return checkboxes
      .filter((checkbox) => checkbox.checked)
      .filter((checkbox) => !checkbox.disabled)
      .map((checkbox) => checkbox.dataset.profileId);
  }

  function syncControls() {
    for (const checkbox of checkboxes) {
      const hasConflict = checkbox.dataset.hasConflict === "true";
      if (hasConflict && selectedConflictPolicy !== "replace") {
        checkbox.checked = false;
      }
      checkbox.disabled = importDisabled || (hasConflict && selectedConflictPolicy !== "replace");
    }
    for (const option of conflictButtons) {
      option.disabled = importDisabled;
    }
    button.disabled = importDisabled || selectedProfileIds().length === 0;
  }

  function setConflictPolicy(value) {
    if (value !== "reject" && value !== "replace") {
      return selectedConflictPolicy;
    }
    if (importDisabled) {
      return selectedConflictPolicy;
    }
    selectedConflictPolicy = value;
    for (const option of conflictButtons) {
      const selected = option.dataset.conflictPolicy === value;
      option.classList.toggle("is-active", selected);
      option.setAttribute("aria-pressed", selected ? "true" : "false");
    }
    window.wittyProfileImportConflictPolicy = selectedConflictPolicy;
    syncControls();
    return selectedConflictPolicy;
  }

  for (const checkbox of checkboxes) {
    checkbox.addEventListener("change", syncControls);
  }
  window.wittyProfileImportConflictPolicy = selectedConflictPolicy;
  for (const option of conflictButtons) {
    const selected = option.dataset.conflictPolicy === selectedConflictPolicy;
    option.classList.toggle("is-active", selected);
    option.setAttribute("aria-pressed", selected ? "true" : "false");
  }
  syncControls();

  button.addEventListener("click", () =>
    confirmImport(selectedProfileIds(), selectedConflictPolicy),
  );
  panel.append(list, controls, message, resultSummary);
  document.querySelector("main").prepend(panel);

  window.wittySetProfileImportMessage = (text) => {
    message.textContent = String(text ?? "");
    return message.textContent;
  };
  window.wittySetProfileImportDisabled = (disabled) => {
    if (!disabled && importLocked) {
      syncControls();
      return importDisabled;
    }
    importDisabled = Boolean(disabled);
    syncControls();
    return importDisabled;
  };
  window.wittySetProfileImportConflictPolicy = setConflictPolicy;
  window.wittySetProfileImportReport = (report) => {
    if (
      report === null ||
      typeof report !== "object" ||
      !authorizedProfileImportReports.has(report)
    ) {
      return null;
    }
    if (acceptedReportSummary !== null) {
      return acceptedReportSummary;
    }
    importLocked = true;
    importDisabled = true;
    syncControls();
    const summary = Object.freeze({
      selected: Number.isSafeInteger(report?.selected) ? report.selected : 0,
      added: Number.isSafeInteger(report?.added) ? report.added : 0,
      replaced: Number.isSafeInteger(report?.replaced) ? report.replaced : 0,
      warnings: Number.isSafeInteger(report?.warning_count) ? report.warning_count : 0,
      globalWarnings: Number.isSafeInteger(report?.global_warning_count)
        ? report.global_warning_count
        : 0,
    });
    window.wittyProfileImportResultSummary = summary;
    acceptedReportSummary = summary;
    allowedNextPickerUrl =
      typeof report?.next_picker_url === "string" &&
      validProfilePickerPageUrl(report.next_picker_url)
        ? report.next_picker_url
        : "";
    resultSummary.textContent = `${summary.selected} selected - ${summary.added} added - ${summary.replaced} replaced - ${summary.warnings} warnings - ${summary.globalWarnings} global`;
    resultSummary.hidden = false;
    return summary;
  };
  window.wittySetProfileImportNextPicker = (nextPickerUrl) => {
    if (
      typeof nextPickerUrl !== "string" ||
      !validProfilePickerPageUrl(nextPickerUrl) ||
      nextPickerUrl !== allowedNextPickerUrl
    ) {
      return false;
    }
    nextPickerButton.hidden = false;
    nextPickerButton.onclick = () => openProfilePickerUrl(nextPickerUrl);
    return true;
  };
  return panel;
}

async function websocketMessageText(data) {
  if (typeof data === "string") {
    return data;
  }
  if (data instanceof Blob) {
    return data.text();
  }
  return new Response(data).text();
}

function isCopySelectionShortcut(event) {
  return (
    event.ctrlKey &&
    event.shiftKey &&
    !event.altKey &&
    !event.metaKey &&
    typeof event.key === "string" &&
    event.key.toLowerCase() === "c"
  );
}

function isPasteClipboardShortcut(event) {
  return (
    event.ctrlKey &&
    event.shiftKey &&
    !event.altKey &&
    !event.metaKey &&
    typeof event.key === "string" &&
    event.key.toLowerCase() === "v"
  );
}

function isSearchShortcut(event) {
  return (
    event.ctrlKey &&
    event.shiftKey &&
    !event.altKey &&
    !event.metaKey &&
    typeof event.key === "string" &&
    event.key.toLowerCase() === "f"
  );
}

function isCommandPaletteShortcut(event) {
  return (
    event.ctrlKey &&
    event.shiftKey &&
    !event.altKey &&
    !event.metaKey &&
    typeof event.key === "string" &&
    event.key.toLowerCase() === "p"
  );
}

function captureSearchState(session, statusText) {
  const state = {
    open: session.search_is_open(),
    query: session.search_query(),
    status: statusText ?? session.search_status_text(),
    matchCount: session.search_match_count(),
    activeIndex: session.search_active_index(),
    visibleHighlights: session.search_visible_highlight_count(),
    activeVisible: session.search_active_visible(),
    caseSensitive: session.search_case_sensitive(),
    regex: session.search_regex_enabled(),
    wholeWord: session.search_whole_word_enabled(),
    normalizeNfc: session.search_normalize_nfc_enabled(),
    error: session.search_error_text(),
  };
  window.wittyLastSearch = state;
  return state;
}

function updateSearchStatus(session, statusText) {
  const state = captureSearchState(session, statusText);
  setStatus(
    "ok",
    `rendered; search=${state.status}; highlights=${state.visibleHighlights}`,
  );
  return state;
}

function captureCommandPaletteState(session, statusText) {
  const state = {
    open: session.command_palette_is_open(),
    query: session.command_palette_query(),
    filteredCount: session.command_palette_filtered_count(),
    selectedIndex: session.command_palette_selected_index(),
    selectedId: session.command_palette_selected_id(),
    status: statusText ?? session.command_palette_status_text(),
    visibleItems: JSON.parse(
      session.command_palette_visible_items_json(COMMAND_PALETTE_VISIBLE_ITEM_LIMIT),
    ),
  };
  window.wittyLastCommandPalette = state;
  return state;
}

function updateCommandPaletteStatus(session, statusText) {
  const state = captureCommandPaletteState(session, statusText);
  setStatus(
    "ok",
    `rendered; command_palette=${state.status}; items=${state.visibleItems.length}`,
  );
  return state;
}

function captureCommandBlockActionMenuState(session, statusText) {
  const state = {
    open: session.command_block_action_menu_is_open(),
    selectedIndex: session.command_block_action_menu_selected_index(),
    selectedId: session.command_block_action_menu_selected_id(),
    status: statusText ?? session.command_block_action_menu_status_text(),
    visibleItems: JSON.parse(session.command_block_action_menu_visible_items_json()),
  };
  window.wittyLastCommandBlockActionMenu = state;
  return state;
}

function updateCommandBlockActionMenuStatus(session, statusText) {
  const state = captureCommandBlockActionMenuState(session, statusText);
  setStatus(
    "ok",
    `rendered; command_block_actions=${state.status}; items=${state.visibleItems.length}`,
  );
  return state;
}

function captureCommandBlocks(session) {
  const blocks = JSON.parse(session.completed_command_blocks_json());
  const activeScreenBlocks = JSON.parse(
    session.completed_command_blocks_for_active_screen_json(),
  );
  const visibleBlocks = JSON.parse(session.visible_command_blocks_json());
  const visibleRowSpans = JSON.parse(session.visible_command_block_row_spans_json());
  const foldedHiddenRowSpans = JSON.parse(
    session.folded_command_block_hidden_row_spans_json(),
  );
  const foldedCompactRows = JSON.parse(
    session.folded_command_block_compact_rows_json(),
  );
  const state = {
    activeScreen: session.active_screen(),
    completedCount: session.completed_command_block_count(),
    activeScreenCompletedCount:
      session.completed_command_block_count_for_active_screen(),
    completed: blocks,
    activeScreenCompleted: activeScreenBlocks,
    visibleCount: visibleBlocks.length,
    visible: visibleBlocks,
    visibleRowSpans,
    foldedHiddenRowSpans,
    foldedCompactRows,
    last: JSON.parse(session.last_completed_command_block_json()),
    selected: JSON.parse(session.selected_command_block_json()),
    selectedTextRanges: JSON.parse(session.selected_command_block_text_ranges_json()),
    selectedText: JSON.parse(session.selected_command_block_text_json()),
  };
  window.wittyLastCommandBlocks = state;
  return state;
}

function selectCommandBlock(session, action) {
  const selected = JSON.parse(action());
  captureCommandBlocks(session);
  return selected;
}

function selectCommandBlockGutterHit(session, offsetX, offsetY) {
  const selected = JSON.parse(
    session.select_command_block_gutter_hit_json(offsetX, offsetY),
  );
  captureCommandBlocks(session);
  return selected;
}

function openCommandPalette(session) {
  const state = updateCommandPaletteStatus(session, session.open_command_palette());
  captureSearchState(session);
  return state;
}

function commandShortcutKey(event) {
  if (event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) {
    return "";
  }
  return event.key === "F1" || event.key === "F2" ? event.key : "";
}

function captureImeState(session, extra = {}) {
  const imeState = JSON.parse(session.ime_state_json());
  const state = {
    ...imeState,
    inputMode: window.wittyImeInput?.inputMode ?? "",
    activeElement: document.activeElement?.id ?? "",
    cursorRect: window.wittyLastImeCursorRect ?? null,
    ...extra,
  };
  window.wittyLastIme = state;
  return state;
}

function handleSearchKeydown(event, session) {
  let statusText = null;
  if (event.key === "Escape") {
    statusText = session.close_search();
  } else if (event.key === "Enter" && event.shiftKey) {
    statusText = session.search_previous();
  } else if (event.key === "Enter") {
    statusText = session.search_next();
  } else if (event.key === "ArrowUp") {
    statusText = session.search_history_previous();
  } else if (event.key === "ArrowDown") {
    statusText = session.search_history_next();
  } else if (event.key === "Backspace") {
    statusText = session.search_backspace();
  } else if (isSearchOptionShortcut(event, "c")) {
    statusText = session.toggle_search_case_sensitive();
  } else if (isSearchOptionShortcut(event, "r")) {
    statusText = session.toggle_search_regex();
  } else if (isSearchOptionShortcut(event, "w")) {
    statusText = session.toggle_search_whole_word();
  } else if (isSearchOptionShortcut(event, "n")) {
    statusText = session.toggle_search_normalize_nfc();
  } else if (
    typeof event.key === "string" &&
    event.key.length === 1 &&
    !event.ctrlKey &&
    !event.altKey &&
    !event.metaKey
  ) {
    statusText = session.search_input_text(event.key);
  } else {
    statusText = session.search_status_text();
  }

  event.preventDefault();
  return updateSearchStatus(session, statusText);
}

function handleCommandPaletteKeydown(event, session) {
  let statusText = null;
  const shortcutKey = commandShortcutKey(event);
  if (shortcutKey) {
    const commandId = session.invoke_command_shortcut(shortcutKey);
    if (commandId) {
      flushGatewayInput(session);
      event.preventDefault();
      window.wittyLastCommandShortcutInvocation = {
        key: shortcutKey,
        commandId,
        search: captureSearchState(session),
        writtenBytes: session.written_bytes(),
      };
      const state = updateCommandPaletteStatus(session, session.command_palette_status_text());
      return state;
    }
  }

  if (event.key === "Escape") {
    statusText = session.close_command_palette();
  } else if (event.key === "Enter") {
    const commandId = session.confirm_command_palette();
    flushGatewayInput(session);
    statusText = session.command_palette_status_text();
    window.wittyLastCommandPaletteInvocation = {
      commandId,
      search: captureSearchState(session),
      writtenBytes: session.written_bytes(),
    };
    if (isCommandBlockCopyCommand(commandId)) {
      window.wittyLastCommandBlockCopyPromise = copyCommandBlockToClipboard(
        session,
        commandId,
      ).catch((error) => {
        window.wittyLastCommandBlockCopy = {
          copied: false,
          commandId,
          reason: String(error?.message ?? error),
          textLength: 0,
        };
        console.error(error);
        setStatus("failed", `command block copy failed: ${String(error?.message ?? error)}`);
        return false;
      });
    }
    if (isCommandBlockActionMenuCommand(commandId)) {
      captureCommandBlockActionMenuState(session);
    }
  } else if (event.key === "Backspace") {
    statusText = session.command_palette_backspace();
  } else if (event.key === "ArrowUp") {
    statusText = session.command_palette_move_selection(-1);
  } else if (event.key === "ArrowDown") {
    statusText = session.command_palette_move_selection(1);
  } else if (event.key === "PageUp") {
    statusText = session.command_palette_move_selection(-5);
  } else if (event.key === "PageDown") {
    statusText = session.command_palette_move_selection(5);
  } else if (
    typeof event.key === "string" &&
    event.key.length === 1 &&
    !event.ctrlKey &&
    !event.altKey &&
    !event.metaKey
  ) {
    statusText = session.command_palette_input_text(event.key);
  } else {
    statusText = session.command_palette_status_text();
  }

  event.preventDefault();
  const state = updateCommandPaletteStatus(session, statusText);
  if (session.search_is_open()) {
    captureSearchState(session);
  }
  return state;
}

function handleCommandBlockActionMenuKeydown(event, session) {
  let statusText = null;

  if (event.key === "Escape") {
    statusText = session.close_command_block_action_menu();
  } else if (event.key === "Enter") {
    const commandId = session.confirm_command_block_action_menu();
    statusText = session.command_block_action_menu_status_text();
    window.wittyLastCommandBlockActionMenuInvocation = {
      commandId,
      writtenBytes: session.written_bytes(),
    };
    if (isCommandBlockCopyCommand(commandId)) {
      window.wittyLastCommandBlockCopyPromise = copyCommandBlockToClipboard(
        session,
        commandId,
      ).catch((error) => {
        window.wittyLastCommandBlockCopy = {
          copied: false,
          commandId,
          reason: String(error?.message ?? error),
          textLength: 0,
        };
        console.error(error);
        setStatus("failed", `command block copy failed: ${String(error?.message ?? error)}`);
        return false;
      });
    }
  } else if (event.key === "ArrowUp") {
    statusText = session.command_block_action_menu_move_selection(-1);
  } else if (event.key === "ArrowDown") {
    statusText = session.command_block_action_menu_move_selection(1);
  } else {
    statusText = session.command_block_action_menu_status_text();
  }

  event.preventDefault();
  return updateCommandBlockActionMenuStatus(session, statusText);
}

function isSearchOptionShortcut(event, key) {
  return (
    event.altKey &&
    !event.ctrlKey &&
    !event.metaKey &&
    typeof event.key === "string" &&
    event.key.toLowerCase() === key
  );
}

async function copySelectionToClipboard(session) {
  const text = session.selected_text();
  if (!text) {
    window.wittyLastClipboardCopy = {
      copied: false,
      reason: "empty-selection",
      textLength: 0,
    };
    setStatus("ok", "rendered; clipboard_copy=empty-selection");
    return false;
  }

  const clipboard = browserClipboard();
  if (!clipboard || typeof clipboard.writeText !== "function") {
    throw new Error("browser clipboard writeText is unavailable");
  }

  await clipboard.writeText(text);
  window.wittyLastClipboardCopy = {
    copied: true,
    reason: "",
    textLength: text.length,
  };
  setStatus("ok", `rendered; clipboard_copy=${text.length}`);
  return true;
}

async function copyCommandBlockToClipboard(session, commandId) {
  const text = session.command_block_copy_text(commandId);
  if (!text) {
    window.wittyLastCommandBlockCopy = {
      copied: false,
      commandId,
      reason: "empty-command-block",
      textLength: 0,
    };
    setStatus("ok", "rendered; command_block_copy=empty-command-block");
    return false;
  }

  const clipboard = browserClipboard();
  if (!clipboard || typeof clipboard.writeText !== "function") {
    throw new Error("browser clipboard writeText is unavailable");
  }

  await clipboard.writeText(text);
  window.wittyLastCommandBlockCopy = {
    copied: true,
    commandId,
    reason: "",
    textLength: text.length,
  };
  setStatus("ok", `rendered; command_block_copy=${text.length}`);
  return true;
}

async function pasteClipboardToTerminal(session) {
  const clipboard = browserClipboard();
  if (!clipboard || typeof clipboard.readText !== "function") {
    throw new Error("browser clipboard readText is unavailable");
  }

  const text = await clipboard.readText();
  if (!text) {
    window.wittyLastClipboardPaste = {
      pasted: false,
      reason: "empty-clipboard",
      textLength: 0,
    };
    setStatus("ok", "rendered; clipboard_paste=empty-clipboard");
    return false;
  }

  const pasted = session.paste_text(text);
  if (pasted) {
    flushGatewayInput(session);
  }
  window.wittyLastClipboardPaste = {
    pasted,
    reason: pasted ? "" : "empty-clipboard",
    textLength: text.length,
  };
  setStatus("ok", `rendered; clipboard_paste=${text.length}`);
  return pasted;
}

async function applyOsc52ClipboardActions(session, policy) {
  const json = session.drain_clipboard_write_actions_json();
  const actions = JSON.parse(json);
  const results = [];

  for (const action of actions) {
    const result = {
      status: "denied",
      reason: "",
      selection: String(action.selection ?? ""),
      textLength: typeof action.text === "string" ? action.text.length : 0,
      decodedBytes: Number(action.decoded_bytes ?? 0),
    };

    if (policy === "disabled") {
      result.reason = "policy-disabled";
    } else if (policy === "confirm") {
      result.reason = "policy-confirm-unimplemented";
    } else if (action.selection !== "clipboard") {
      result.status = "unsupported";
      result.reason = "unsupported-selection";
    } else {
      const clipboard = browserClipboard();
      if (!clipboard || typeof clipboard.writeText !== "function") {
        result.status = "unsupported";
        result.reason = "clipboard-writeText-unavailable";
      } else {
        try {
          await clipboard.writeText(String(action.text ?? ""));
          result.status = "written";
          result.reason = "";
        } catch (error) {
          result.status = "permission-error";
          result.reason = String(error?.message ?? error);
        }
      }
    }

    results.push(result);
  }

  window.wittyLastOsc52ClipboardResults = results;
  window.wittyOsc52ClipboardResults.push(...results);
  if (results.length > 0) {
    setStatus("ok", `rendered; osc52_clipboard=${results.map((result) => result.status).join(",")}`);
  }
  return results;
}

async function main() {
  await init();

  const glyphChars = witty_web_mock_replay_glyph_chars();
  const writtenBytes = witty_web_session_written_bytes();
  const canvas = document.getElementById("witty-canvas");
  const imeInput = ensureImeInput(canvas);
  const initialLayout = canvasLayout(canvas);
  const fontResponse = await fetch("./fonts/witty-mono.ttf");
  if (!fontResponse.ok) {
    throw new Error(`failed to load smoke font: ${fontResponse.status}`);
  }
  const fontData = new Uint8Array(await fontResponse.arrayBuffer());
  if (fontData.length === 0) {
    throw new Error("smoke font is empty");
  }
  const session = await witty_create_session(
    "witty-canvas",
    fontData,
    initialLayout.cssWidth,
    initialLayout.cssHeight,
    initialLayout.devicePixelRatio,
  );
  window.wittySession = session;
  window.wittyGatewayFrames = gatewayFrames;
  window.wittyLastGatewayOutput = "";
  window.wittyGatewayOutputText = "";
  window.wittyLastRenderedScreenText = "";
  window.wittyLastRenderedScreenTextError = null;
  window.wittyLastClipboardCopy = null;
  window.wittyLastClipboardCopyPromise = null;
  window.wittyLastClipboardPaste = null;
  window.wittyLastClipboardPastePromise = null;
  window.wittyClipboardApi = null;
  window.wittyOsc52ClipboardResults = [];
  window.wittyLastOsc52ClipboardResults = [];
  window.wittyLastTerminalReplyFrame = null;
  window.wittyLastSearch = captureSearchState(session);
  window.wittyLastCommandPalette = captureCommandPaletteState(session);
  window.wittyLastCommandPaletteInvocation = null;
  window.wittyLastCommandBlockActionMenu =
    captureCommandBlockActionMenuState(session);
  window.wittyLastCommandBlockActionMenuInvocation = null;
  window.wittyImeInput = imeInput;
  window.wittyImeComposing = false;
  window.wittyLastIme = captureImeState(session, { source: "init" });
  let mouseSelectionOverridePolicy = parseMouseSelectionOverridePolicy();
  let osc52ClipboardPolicy = parseOsc52ClipboardPolicy();
  let scrollbackLines = parseScrollbackLines();
  window.wittyMouseSelectionOverridePolicy = () => mouseSelectionOverridePolicy;
  window.wittySetMouseSelectionOverridePolicy = (policy) => {
    mouseSelectionOverridePolicy = parseMouseSelectionOverridePolicy(policy);
    return mouseSelectionOverridePolicy;
  };
  window.wittyOsc52ClipboardPolicy = () => osc52ClipboardPolicy;
  window.wittySetOsc52ClipboardPolicy = (policy) => {
    osc52ClipboardPolicy = parseOsc52ClipboardPolicy(policy);
    return osc52ClipboardPolicy;
  };
  window.wittyScrollbackLines = () => scrollbackLines;
  window.wittySetScrollbackLines = (lines) => {
    scrollbackLines = parseScrollbackLines(lines);
    session.set_scrollback_lines(scrollbackLines);
    return scrollbackLines;
  };
  window.wittySyncImeInputPosition = () =>
    syncImeInputPosition(session, canvas, imeInput);
  window.wittyFocusTerminalInput = () => {
    focusTerminalInput(session, canvas, imeInput);
    return true;
  };
  window.wittySetImePreedit = (text, caretStart = -1, caretEnd = -1) => {
    const changed = session.set_ime_preedit(String(text ?? ""), caretStart, caretEnd);
    const rect = syncImeInputPosition(session, canvas, imeInput);
    captureImeState(session, {
      source: "test-preedit",
      changed,
      cursorRect: rect,
    });
    if (session.search_is_open()) {
      captureSearchState(session);
    }
    if (session.command_palette_is_open()) {
      captureCommandPaletteState(session);
    }
    return changed;
  };
  window.wittyCommitImeText = (text) => {
    const committed = session.commit_ime_text(String(text ?? ""));
    if (committed) {
      flushGatewayInput(session);
    }
    imeInput.value = "";
    const rect = syncImeInputPosition(session, canvas, imeInput);
    captureImeState(session, {
      source: "test-commit",
      committed,
      cursorRect: rect,
    });
    if (session.search_is_open()) {
      captureSearchState(session);
    }
    if (session.command_palette_is_open()) {
      captureCommandPaletteState(session);
    }
    return committed;
  };
  window.wittyClearImePreedit = () => {
    const changed = session.clear_ime_preedit();
    imeInput.value = "";
    const rect = syncImeInputPosition(session, canvas, imeInput);
    captureImeState(session, { source: "test-clear", changed, cursorRect: rect });
    return changed;
  };
  window.wittyImeDiagnostics = () => {
    const rect = syncImeInputPosition(session, canvas, imeInput);
    return captureImeState(session, { source: "diagnostic", cursorRect: rect });
  };
  window.wittyFrameStats = () => JSON.parse(session.frame_stats_json());
  window.wittyOpenCommandPalette = () => openCommandPalette(session);
  window.wittyCommandPaletteState = () => captureCommandPaletteState(session);
  window.wittyOpenCommandBlockActionMenu = () =>
    updateCommandBlockActionMenuStatus(
      session,
      session.open_command_block_action_menu(),
    );
  window.wittyCommandBlockActionMenuState = () =>
    captureCommandBlockActionMenuState(session);
  window.wittyCommandBlocks = () => captureCommandBlocks(session);
  window.wittySelectLatestCommandBlock = () =>
    selectCommandBlock(session, () =>
      session.select_latest_command_block_for_active_screen_json(),
    );
  window.wittySelectPreviousCommandBlock = () =>
    selectCommandBlock(session, () =>
      session.select_previous_command_block_for_active_screen_json(),
    );
  window.wittySelectNextCommandBlock = () =>
    selectCommandBlock(session, () =>
      session.select_next_command_block_for_active_screen_json(),
    );
  window.wittyToggleSelectedCommandBlockFold = () =>
    selectCommandBlock(session, () =>
      session.toggle_selected_command_block_fold_json(),
    );
  window.wittyCommandBlockGutterHit = (offsetX, offsetY) =>
    JSON.parse(session.command_block_gutter_hit_json(offsetX, offsetY));
  window.wittySelectCommandBlockGutterHit = (offsetX, offsetY) =>
    selectCommandBlockGutterHit(session, offsetX, offsetY);
  window.wittyClearSelectedCommandBlock = () => {
    session.clear_selected_command_block();
    return captureCommandBlocks(session);
  };
  window.wittySynchronizedOutputTimeoutMs = SYNCHRONIZED_OUTPUT_TIMEOUT_MS;
  let gatewayMessageQueue = Promise.resolve();
  let synchronizedOutputTimer = null;

  function captureRenderedScreenText() {
    try {
      window.wittyLastRenderedScreenText = session.screen_text();
      window.wittyLastRenderedScreenTextError = null;
    } catch (error) {
      window.wittyLastRenderedScreenTextError = String(error?.stack ?? error);
    }
  }

  function clearSynchronizedOutputTimer() {
    if (synchronizedOutputTimer !== null) {
      clearTimeout(synchronizedOutputTimer);
      synchronizedOutputTimer = null;
    }
  }

  function scheduleSynchronizedOutputFlush() {
    if (!session.synchronized_output_enabled()) {
      clearSynchronizedOutputTimer();
      return;
    }
    if (synchronizedOutputTimer !== null) {
      return;
    }

    synchronizedOutputTimer = setTimeout(() => {
      synchronizedOutputTimer = null;
      if (!session.synchronized_output_enabled()) {
        return;
      }
      const flushed = session.flush_synchronized_output();
      if (flushed) {
        captureRenderedScreenText();
        setStatus("ok", `rendered; synchronized_output_timeout=${SYNCHRONIZED_OUTPUT_TIMEOUT_MS}`);
      }
    }, SYNCHRONIZED_OUTPUT_TIMEOUT_MS);
  }

  async function applyGatewayMessageJson(json) {
    const frame = JSON.parse(json);
    gatewayFrames.push({ direction: "server", frame });
    if (frame.type === "output" && Array.isArray(frame.bytes)) {
      const text = String.fromCharCode(...frame.bytes);
      window.wittyLastGatewayOutput = text;
      window.wittyGatewayOutputText += text;
    }
    session.push_gateway_message_json(json);
    captureRenderedScreenText();
    const clipboardActions = applyOsc52ClipboardActions(session, osc52ClipboardPolicy);
    window.wittyLastTerminalReplyFrame = flushGatewayInput(session);
    syncImeInputPosition(session, canvas, imeInput);
    scheduleSynchronizedOutputFlush();
    await clipboardActions;
    captureCommandBlocks(session);
    setStatus("ok", `rendered; gateway_frame=${frame.type}; grid=${session.grid_cols()}x${session.grid_rows()}`);
    return frame;
  }

  function enqueueGatewayMessageJson(json) {
    const next = gatewayMessageQueue.then(() => applyGatewayMessageJson(json));
    gatewayMessageQueue = next.catch(() => {});
    return next;
  }

  const waitForGatewayQuiescent = async () => {
    for (;;) {
      const pending = gatewayMessageQueue;
      await pending;
      if (pending === gatewayMessageQueue) {
        await new Promise((resolve) => setTimeout(resolve, 25));
        if (pending === gatewayMessageQueue) {
          return;
        }
      }
    }
  };
  window.wittyGatewayIdle = () => waitForGatewayQuiescent();
  window.wittyReadScreenText = async () => {
    await waitForGatewayQuiescent();
    return window.wittyLastRenderedScreenText;
  };
  window.wittyPushGatewayOutput = (bytes) =>
    enqueueGatewayMessageJson(JSON.stringify({ type: "output", bytes }));

  window.wittySyncCanvasSize = () => {
    const layout = canvasLayout(canvas);
    session.resize(layout.cssWidth, layout.cssHeight, layout.devicePixelRatio);
    const imeCursorRect = syncImeInputPosition(session, canvas, imeInput);
    sendGatewayResize(session);
    if (
      document.body.dataset.wittyPicker !== "active" &&
      document.body.dataset.wittyImport !== "active"
    ) {
      setStatus(
        "ok",
        `rendered; grid=${session.grid_cols()}x${session.grid_rows()}; dpr=${session.device_pixel_ratio().toFixed(2)}`,
      );
    }
    return {
      backingWidth: session.backing_width(),
      backingHeight: session.backing_height(),
      gridRows: session.grid_rows(),
      gridCols: session.grid_cols(),
      transportGrid: session.transport_grid_text(),
      devicePixelRatio: session.device_pixel_ratio(),
      imeCursorRect,
    };
  };

  window.wittyFlushGatewayInput = () => flushGatewayInput(session);
  window.wittySendGatewayInputBytes = (bytes) =>
    sendGatewayFrame(JSON.stringify({ type: "input", bytes }));

  window.wittyConnectGateway = (gatewayUrl) =>
    new Promise((resolve, reject) => {
      const socket = new WebSocket(gatewayUrl);
      gatewaySocket = socket;
      let opened = false;

      socket.addEventListener(
        "open",
        () => {
          opened = true;
          sendGatewayFrame(JSON.stringify({ type: "hello", protocol: 1 }));
          sendGatewayResize(session);
          syncImeInputPosition(session, canvas, imeInput);
          setStatus("ok", `rendered; gateway=connected; grid=${session.grid_cols()}x${session.grid_rows()}`);
          resolve(true);
        },
        { once: true },
      );

      socket.addEventListener("message", async (event) => {
        const json = await websocketMessageText(event.data);
        await enqueueGatewayMessageJson(json);
      });

      socket.addEventListener(
        "error",
        () => {
          if (!opened) {
            reject(new Error(`gateway websocket failed: ${gatewayUrl}`));
          }
        },
        { once: true },
      );

      socket.addEventListener(
        "close",
        () => {
          if (!opened) {
            reject(new Error(`gateway websocket closed before opening: ${gatewayUrl}`));
          }
        },
        { once: true },
      );
    });

  let profilePickerRequestInFlight = false;
  let profileImportRequestInFlight = false;

  function hasLiveUiToken(bootstrap) {
    return typeof bootstrap.ui_token === "string" && bootstrap.ui_token.length > 0;
  }

  function consumeProfilePickerUiToken(bootstrap) {
    bootstrap.ui_token = "";
    exposeProfilePickerBootstrap(bootstrap);
  }

  function consumeProfileImportUiToken(bootstrap) {
    bootstrap.ui_token = "";
    exposeProfileImportBootstrap(bootstrap);
  }

  async function connectSessionConfig(config) {
    window.wittySetMouseSelectionOverridePolicy(config.mouse_selection_override);
    window.wittySetScrollbackLines(config.scrollback_lines);
    await window.wittyConnectGateway(gatewayUrlFromSessionConfig(config));
    return true;
  }

  async function selectProfileFromPicker(bootstrap, profileId) {
    if (
      profilePickerRequestInFlight ||
      !hasLiveUiToken(bootstrap) ||
      !profilePickerSelectionAllowed(bootstrap, profileId)
    ) {
      return false;
    }
    profilePickerRequestInFlight = true;
    window.wittyProfilePickerLastError = null;
    window.wittyProfilePickerState = "profile_picker_selecting";
    window.wittySetProfilePickerDisabled?.(true);
    window.wittySetProfilePickerMessage?.("Launching");
    setStatus("profile_picker_selecting", "profile picker selecting");

    try {
      const response = await fetch(bootstrap.selection_url, {
        method: "POST",
        cache: "no-store",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          ui_token: bootstrap.ui_token,
          profile_id: profileId,
        }),
      });
      if (!response.ok) {
        const text = await response.text();
        const message = text || `profile picker selection failed: ${response.status}`;
        window.wittyProfilePickerState = "profile_picker_error";
        window.wittyProfilePickerLastError = {
          status: response.status,
          message,
        };
        window.wittySetProfilePickerMessage?.(message);
        setStatus("profile_picker_error", message);
        if ([401, 410].includes(response.status)) {
          consumeProfilePickerUiToken(bootstrap);
        } else {
          window.wittySetProfilePickerDisabled?.(false);
        }
        profilePickerRequestInFlight = false;
        return false;
      }

      consumeProfilePickerUiToken(bootstrap);
      const sessionConfig = normalizeSessionConfig(await response.json());
      window.wittyProfilePickerState = "profile_picker_connecting";
      window.wittyProfilePickerSelectedId = profileId;
      window.wittySetProfilePickerMessage?.("Connecting");
      setStatus("profile_picker_connecting", "profile picker connecting");
      profilePickerRequestInFlight = false;
      removeProfilePickerElement();
      await connectSessionConfig(sessionConfig);
      focusTerminalInput(session, canvas, imeInput);
      window.wittyProfilePickerState = "terminal_connected";
      return true;
    } catch (error) {
      profilePickerRequestInFlight = false;
      throw error;
    }
  }

  async function startImportFromPicker(bootstrap, actionId) {
    if (
      profilePickerRequestInFlight ||
      !hasLiveUiToken(bootstrap) ||
      !profilePickerImportActionAllowed(bootstrap, actionId)
    ) {
      return false;
    }
    profilePickerRequestInFlight = true;
    window.wittyProfilePickerLastError = null;
    window.wittyProfilePickerState = "profile_picker_importing";
    window.wittySetProfilePickerDisabled?.(true);
    window.wittySetProfilePickerMessage?.("Opening import");
    setStatus("profile_picker_importing", "profile picker importing");

    try {
      const response = await fetch(bootstrap.import_url, {
        method: "POST",
        cache: "no-store",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          ui_token: bootstrap.ui_token,
          action_id: actionId,
        }),
      });
      if (!response.ok) {
        const text = await response.text();
        const message = text || `profile picker import failed: ${response.status}`;
        window.wittyProfilePickerState = "profile_picker_error";
        window.wittyProfilePickerLastError = {
          status: response.status,
          message,
        };
        window.wittySetProfilePickerMessage?.(message);
        setStatus("profile_picker_error", message);
        if ([401, 410].includes(response.status)) {
          consumeProfilePickerUiToken(bootstrap);
        } else {
          window.wittySetProfilePickerDisabled?.(false);
        }
        profilePickerRequestInFlight = false;
        return false;
      }

      consumeProfilePickerUiToken(bootstrap);
      const entry = Object.freeze(validateProfilePickerImportEntry(await response.json()));
      const importUrl = entry.import_url;
      profilePickerRequestInFlight = false;
      window.wittyProfilePickerImportEntry = entry;
      window.wittyProfilePickerState = "profile_picker_import_ready";
      window.wittySetProfilePickerMessage?.("Opening import");
      setStatus("profile_picker_import_ready", "profile picker import ready");
      setTimeout(() => {
        window.history.replaceState(null, "", importUrl);
        window.location.reload();
      }, 0);
      return true;
    } catch (error) {
      profilePickerRequestInFlight = false;
      throw error;
    }
  }

  async function confirmProfileImport(bootstrap, profileIds, conflict) {
    if (
      profileImportRequestInFlight ||
      !hasLiveUiToken(bootstrap) ||
      !profileImportConfirmRequestAllowed(bootstrap, profileIds, conflict)
    ) {
      return null;
    }
    profileImportRequestInFlight = true;
    window.wittyProfileImportLastError = null;
    window.wittyProfileImportState = "profile_import_confirming";
    window.wittySetProfileImportDisabled?.(true);
    window.wittySetProfileImportMessage?.("Importing");
    setStatus("profile_import_confirming", "profile import confirming");

    try {
      const response = await fetch(bootstrap.confirm_url, {
        method: "POST",
        cache: "no-store",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          ui_token: bootstrap.ui_token,
          profile_ids: profileIds,
          conflict,
        }),
      });
      if (!response.ok) {
        const text = await response.text();
        const message = text || `profile import confirmation failed: ${response.status}`;
        window.wittyProfileImportState = "profile_import_error";
        window.wittyProfileImportLastError = {
          status: response.status,
          message,
        };
        window.wittySetProfileImportMessage?.(message);
        setStatus("profile_import_error", message);
        if ([401, 410].includes(response.status)) {
          consumeProfileImportUiToken(bootstrap);
        } else {
          window.wittySetProfileImportDisabled?.(false);
        }
        profileImportRequestInFlight = false;
        return null;
      }

      consumeProfileImportUiToken(bootstrap);
      const report = Object.freeze(validateProfileImportConfirmReport(await response.json()));
      authorizedProfileImportReports.add(report);
      profileImportRequestInFlight = false;
      window.wittyProfileImportState = "profile_import_done";
      window.wittyProfileImportReport = report;
      window.wittySetProfileImportReport?.(report);
      if (
        typeof report.next_picker_url === "string" &&
        validProfilePickerPageUrl(report.next_picker_url)
      ) {
        const nextPickerUrl = report.next_picker_url;
        window.wittyProfileImportNextPickerUrl = nextPickerUrl;
        window.wittyOpenNextProfilePicker = () => openProfilePickerUrl(nextPickerUrl);
        window.wittySetProfileImportNextPicker?.(nextPickerUrl);
        window.wittySetProfileImportMessage?.("Imported");
      } else {
        window.wittyProfileImportNextPickerUrl = "";
        window.wittyOpenNextProfilePicker = null;
        window.wittySetProfileImportMessage?.("Imported");
      }
      setStatus("profile_import_done", "profile import done");
      return report;
    } catch (error) {
      profileImportRequestInFlight = false;
      throw error;
    }
  }

  const launcherSessionConfig = await loadSessionConfigFromHash();
  const profilePickerBootstrap = launcherSessionConfig
    ? null
    : await loadProfilePickerBootstrapFromHash();
  const profileImportBootstrap =
    launcherSessionConfig || profilePickerBootstrap
      ? null
      : await loadProfileImportBootstrapFromHash();
  if (launcherSessionConfig) {
    await connectSessionConfig(launcherSessionConfig);
  } else if (profilePickerBootstrap) {
    setStatus("profile_picker_ready", "profile picker ready");
    renderProfilePicker(
      profilePickerBootstrap,
      (profileId) => {
        if (
          profilePickerRequestInFlight ||
          !hasLiveUiToken(profilePickerBootstrap) ||
          !profilePickerSelectionAllowed(profilePickerBootstrap, profileId)
        ) {
          return Promise.resolve(false);
        }
        window.wittyLastProfilePickerSelectionPromise = selectProfileFromPicker(
          profilePickerBootstrap,
          profileId,
        ).catch((error) => {
          window.wittyProfilePickerState = "profile_picker_error";
          window.wittyProfilePickerLastError = {
            status: 0,
            message: String(error?.message ?? error),
          };
          window.wittySetProfilePickerMessage?.(String(error?.message ?? error));
          if (hasLiveUiToken(profilePickerBootstrap)) {
            window.wittySetProfilePickerDisabled?.(false);
          }
          console.error(error);
          setStatus("profile_picker_error", String(error?.stack ?? error));
          return false;
        });
        return window.wittyLastProfilePickerSelectionPromise;
      },
      (actionId) => {
        if (
          profilePickerRequestInFlight ||
          !hasLiveUiToken(profilePickerBootstrap) ||
          !profilePickerImportActionAllowed(profilePickerBootstrap, actionId)
        ) {
          return Promise.resolve(false);
        }
        window.wittyLastProfilePickerImportPromise = startImportFromPicker(
          profilePickerBootstrap,
          actionId,
        ).catch((error) => {
          window.wittyProfilePickerState = "profile_picker_error";
          window.wittyProfilePickerLastError = {
            status: 0,
            message: String(error?.message ?? error),
          };
          window.wittySetProfilePickerMessage?.(String(error?.message ?? error));
          if (hasLiveUiToken(profilePickerBootstrap)) {
            window.wittySetProfilePickerDisabled?.(false);
          }
          console.error(error);
          setStatus("profile_picker_error", String(error?.stack ?? error));
          return false;
        });
        return window.wittyLastProfilePickerImportPromise;
      },
    );
    window.wittySelectProfile = (profileId) => {
      const normalizedProfileId = String(profileId ?? "");
      if (
        profilePickerRequestInFlight ||
        !hasLiveUiToken(profilePickerBootstrap) ||
        !profilePickerSelectionAllowed(profilePickerBootstrap, normalizedProfileId)
      ) {
        return Promise.resolve(false);
      }
      window.wittyLastProfilePickerSelectionPromise = selectProfileFromPicker(
        profilePickerBootstrap,
        normalizedProfileId,
      ).catch((error) => {
        window.wittyProfilePickerState = "profile_picker_error";
        window.wittyProfilePickerLastError = {
          status: 0,
          message: String(error?.message ?? error),
        };
        window.wittySetProfilePickerMessage?.(String(error?.message ?? error));
        if (hasLiveUiToken(profilePickerBootstrap)) {
          window.wittySetProfilePickerDisabled?.(false);
        }
        console.error(error);
        setStatus("profile_picker_error", String(error?.stack ?? error));
        return false;
      });
      return window.wittyLastProfilePickerSelectionPromise;
    };
    window.wittyStartProfileImport = (actionId) => {
      const normalizedActionId = String(actionId ?? "");
      if (
        profilePickerRequestInFlight ||
        !hasLiveUiToken(profilePickerBootstrap) ||
        !profilePickerImportActionAllowed(profilePickerBootstrap, normalizedActionId)
      ) {
        return Promise.resolve(false);
      }
      window.wittyLastProfilePickerImportPromise = startImportFromPicker(
        profilePickerBootstrap,
        normalizedActionId,
      ).catch((error) => {
        window.wittyProfilePickerState = "profile_picker_error";
        window.wittyProfilePickerLastError = {
          status: 0,
          message: String(error?.message ?? error),
        };
        window.wittySetProfilePickerMessage?.(String(error?.message ?? error));
        if (hasLiveUiToken(profilePickerBootstrap)) {
          window.wittySetProfilePickerDisabled?.(false);
        }
        console.error(error);
        setStatus("profile_picker_error", String(error?.stack ?? error));
        return false;
      });
      return window.wittyLastProfilePickerImportPromise;
    };
  } else if (profileImportBootstrap) {
    setStatus("profile_import_ready", "profile import ready");
    renderProfileImportReview(profileImportBootstrap, (profileIds, conflict) => {
      if (
        profileImportRequestInFlight ||
        !hasLiveUiToken(profileImportBootstrap) ||
        !profileImportConfirmRequestAllowed(profileImportBootstrap, profileIds, conflict)
      ) {
        return Promise.resolve(null);
      }
      window.wittyLastProfileImportConfirmPromise = confirmProfileImport(
        profileImportBootstrap,
        profileIds,
        conflict,
      ).catch((error) => {
        window.wittyProfileImportState = "profile_import_error";
        window.wittyProfileImportLastError = {
          status: 0,
          message: String(error?.message ?? error),
        };
        window.wittySetProfileImportMessage?.(String(error?.message ?? error));
        if (hasLiveUiToken(profileImportBootstrap)) {
          window.wittySetProfileImportDisabled?.(false);
        }
        console.error(error);
        setStatus("profile_import_error", String(error?.stack ?? error));
        return null;
      });
      return window.wittyLastProfileImportConfirmPromise;
    });
    window.wittyConfirmProfileImport = (profileIds, conflict = "reject") => {
      const normalizedProfileIds = Array.isArray(profileIds) ? profileIds.map(String) : [];
      const normalizedConflict = String(conflict ?? "reject");
      if (
        profileImportRequestInFlight ||
        !hasLiveUiToken(profileImportBootstrap) ||
        !profileImportConfirmRequestAllowed(
          profileImportBootstrap,
          normalizedProfileIds,
          normalizedConflict,
        )
      ) {
        return Promise.resolve(null);
      }
      window.wittyLastProfileImportConfirmPromise = confirmProfileImport(
        profileImportBootstrap,
        normalizedProfileIds,
        normalizedConflict,
      ).catch((error) => {
        window.wittyProfileImportState = "profile_import_error";
        window.wittyProfileImportLastError = {
          status: 0,
          message: String(error?.message ?? error),
        };
        window.wittySetProfileImportMessage?.(String(error?.message ?? error));
        if (hasLiveUiToken(profileImportBootstrap)) {
          window.wittySetProfileImportDisabled?.(false);
        }
        console.error(error);
        setStatus("profile_import_error", String(error?.stack ?? error));
        return null;
      });
      return window.wittyLastProfileImportConfirmPromise;
    };
  }

  if (window.ResizeObserver) {
    const observer = new ResizeObserver(() => {
      window.wittySyncCanvasSize();
    });
    observer.observe(canvas);
    window.wittyResizeObserver = observer;
  }
  if (window.visualViewport) {
    const syncVisualViewportIme = () => syncImeInputPosition(session, canvas, imeInput);
    window.visualViewport.addEventListener("resize", syncVisualViewportIme);
    window.visualViewport.addEventListener("scroll", syncVisualViewportIme);
  }

  let suppressedCompositionInput = "";

  function markImeComposing(composing) {
    window.wittyImeComposing = composing;
  }

  function updateImePreedit(text, source) {
    const preedit = String(text ?? "");
    const changed = session.set_ime_preedit(preedit, preedit.length, preedit.length);
    const rect = syncImeInputPosition(session, canvas, imeInput);
    captureImeState(session, {
      source,
      changed,
      composing: window.wittyImeComposing,
      cursorRect: rect,
    });
    if (session.search_is_open()) {
      captureSearchState(session);
    }
    if (session.command_palette_is_open()) {
      captureCommandPaletteState(session);
    }
    setStatus("ok", `rendered; ime_preedit=${preedit.length}`);
    return changed;
  }

  function clearImePreedit(source) {
    const changed = session.clear_ime_preedit();
    const rect = syncImeInputPosition(session, canvas, imeInput);
    captureImeState(session, {
      source,
      changed,
      composing: window.wittyImeComposing,
      cursorRect: rect,
    });
    if (session.search_is_open()) {
      captureSearchState(session);
    }
    if (session.command_palette_is_open()) {
      captureCommandPaletteState(session);
    }
    return changed;
  }

  function commitImeText(text, source) {
    const commitText = String(text ?? "");
    const committed = session.commit_ime_text(commitText);
    if (committed) {
      flushGatewayInput(session);
    }
    imeInput.value = "";
    const rect = syncImeInputPosition(session, canvas, imeInput);
    captureImeState(session, {
      source,
      committed,
      commitText,
      composing: window.wittyImeComposing,
      cursorRect: rect,
    });
    if (session.search_is_open()) {
      captureSearchState(session);
    }
    if (session.command_palette_is_open()) {
      captureCommandPaletteState(session);
    }
    setStatus("ok", `rendered; ime_commit=${committed ? commitText.length : 0}`);
    return committed;
  }

  imeInput.addEventListener("compositionstart", () => {
    markImeComposing(true);
    suppressedCompositionInput = "";
    updateImePreedit("", "compositionstart");
  });

  imeInput.addEventListener("compositionupdate", (event) => {
    updateImePreedit(event.data ?? imeInput.value ?? "", "compositionupdate");
  });

  imeInput.addEventListener("compositionend", (event) => {
    markImeComposing(false);
    clearImePreedit("compositionend-clear");
    const text = event.data ?? "";
    if (text) {
      suppressedCompositionInput = text;
      commitImeText(text, "compositionend");
      setTimeout(() => {
        if (suppressedCompositionInput === text) {
          suppressedCompositionInput = "";
        }
      }, 0);
    }
  });

  imeInput.addEventListener("beforeinput", (event) => {
    const text = event.data ?? "";
    if (suppressedCompositionInput && text === suppressedCompositionInput) {
      event.preventDefault();
      suppressedCompositionInput = "";
      imeInput.value = "";
      captureImeState(session, { source: "beforeinput-suppressed" });
      return;
    }

    if (event.inputType === "insertCompositionText" && window.wittyImeComposing) {
      event.preventDefault();
      updateImePreedit(text, "beforeinput-preedit");
      return;
    }

    if (
      text &&
      (event.inputType === "insertFromComposition" ||
        (event.inputType === "insertCompositionText" && !window.wittyImeComposing))
    ) {
      event.preventDefault();
      commitImeText(text, "beforeinput");
    }
  });

  imeInput.addEventListener("input", () => {
    const text = imeInput.value;
    if (!text) {
      return;
    }
    if (suppressedCompositionInput && text === suppressedCompositionInput) {
      suppressedCompositionInput = "";
      imeInput.value = "";
      captureImeState(session, { source: "input-suppressed" });
      return;
    }
    if (window.wittyImeComposing) {
      updateImePreedit(text, "input-preedit");
    } else {
      commitImeText(text, "input");
    }
  });

  function keyboardModifierMask(event) {
    return (
      (event.shiftKey ? 1 : 0) |
      (event.altKey ? 2 : 0) |
      (event.metaKey ? 4 : 0)
    );
  }

  function keyboardTextForEvent(event) {
    return event.key.length === 1 && !event.ctrlKey && !event.metaKey ? event.key : "";
  }

  function keyboardProtocolDiagnosticReport(fields) {
    const report = JSON.parse(
      witty_browser_keyboard_protocol_diagnostic_report_json(
        String(fields.key ?? ""),
        String(fields.text ?? ""),
        Boolean(fields.control),
        String(fields.code ?? ""),
        Number(fields.location ?? 0),
        Number(fields.modifierMask ?? 0),
        Number(fields.eventType ?? 1),
      ),
    );
    window.wittyLastKeyboardProtocolDiagnostic = report;
    return report;
  }

  window.wittyKeyboardProtocolDiagnostic = (eventOrFields = {}) => {
    if (eventOrFields instanceof KeyboardEvent) {
      const eventType = eventOrFields.type === "keyup" ? 3 : eventOrFields.repeat ? 2 : 1;
      return keyboardProtocolDiagnosticReport({
        key: eventOrFields.key,
        text: eventType === 3 ? "" : keyboardTextForEvent(eventOrFields),
        control: eventOrFields.ctrlKey,
        code: eventOrFields.code || "",
        location: eventOrFields.location || 0,
        modifierMask: keyboardModifierMask(eventOrFields),
        eventType,
      });
    }
    return keyboardProtocolDiagnosticReport(eventOrFields);
  };

  function handleTerminalKeydown(event) {
    if (window.wittyImeComposing || event.isComposing || event.key === "Process") {
      event.preventDefault();
      captureImeState(session, {
        source: "keydown-composing",
        key: event.key,
        composing: true,
      });
      return;
    }

    if (isSearchShortcut(event)) {
      event.preventDefault();
      updateSearchStatus(session, session.open_search());
      syncImeInputPosition(session, canvas, imeInput);
      return;
    }

    if (isCommandPaletteShortcut(event)) {
      event.preventDefault();
      openCommandPalette(session);
      syncImeInputPosition(session, canvas, imeInput);
      return;
    }

    if (session.command_palette_is_open()) {
      handleCommandPaletteKeydown(event, session);
      syncImeInputPosition(session, canvas, imeInput);
      return;
    }

    if (session.command_block_action_menu_is_open()) {
      handleCommandBlockActionMenuKeydown(event, session);
      syncImeInputPosition(session, canvas, imeInput);
      return;
    }

    if (session.search_is_open()) {
      handleSearchKeydown(event, session);
      syncImeInputPosition(session, canvas, imeInput);
      return;
    }

    if (isCopySelectionShortcut(event)) {
      event.preventDefault();
      window.wittyLastClipboardCopyPromise = copySelectionToClipboard(session).catch(
        (error) => {
          window.wittyLastClipboardCopy = {
            copied: false,
            reason: String(error?.message ?? error),
            textLength: 0,
          };
          console.error(error);
          setStatus("failed", `clipboard copy failed: ${String(error?.message ?? error)}`);
          return false;
        },
      );
      return;
    }

    if (isPasteClipboardShortcut(event)) {
      event.preventDefault();
      window.wittyLastClipboardPastePromise = pasteClipboardToTerminal(session).catch(
        (error) => {
          window.wittyLastClipboardPaste = {
            pasted: false,
            reason: String(error?.message ?? error),
            textLength: 0,
          };
          console.error(error);
          setStatus("failed", `clipboard paste failed: ${String(error?.message ?? error)}`);
          return false;
        },
      );
      return;
    }

    const text = keyboardTextForEvent(event);
    const modifierMask = keyboardModifierMask(event);
    const eventType = event.repeat ? 2 : 1;
    keyboardProtocolDiagnosticReport({
      key: event.key,
      text,
      control: event.ctrlKey,
      code: event.code || "",
      location: event.location || 0,
      modifierMask,
      eventType,
    });
    const handled = session.handle_key(
      event.key,
      text,
      event.ctrlKey,
      event.code || "",
      event.location || 0,
      modifierMask,
      eventType,
    );
    if (handled) {
      flushGatewayInput(session);
      event.preventDefault();
      if (event.currentTarget === imeInput) {
        imeInput.value = "";
      }
      syncImeInputPosition(session, canvas, imeInput);
      setStatus(
        "ok",
        `rendered; glyph_chars=${glyphChars}; written_bytes=${session.written_bytes()}`,
      );
    }
  }

  function handleTerminalKeyup(event) {
    if (window.wittyImeComposing || event.isComposing || event.key === "Process") {
      return;
    }

    if (
      session.command_palette_is_open() ||
      session.command_block_action_menu_is_open() ||
      session.search_is_open() ||
      isSearchShortcut(event) ||
      isCommandPaletteShortcut(event) ||
      isCopySelectionShortcut(event) ||
      isPasteClipboardShortcut(event)
    ) {
      return;
    }

    const modifierMask = keyboardModifierMask(event);
    keyboardProtocolDiagnosticReport({
      key: event.key,
      text: "",
      control: event.ctrlKey,
      code: event.code || "",
      location: event.location || 0,
      modifierMask,
      eventType: 3,
    });
    const handled = session.handle_key(
      event.key,
      "",
      event.ctrlKey,
      event.code || "",
      event.location || 0,
      modifierMask,
      3,
    );
    if (handled) {
      flushGatewayInput(session);
      event.preventDefault();
      syncImeInputPosition(session, canvas, imeInput);
      setStatus(
        "ok",
        `rendered; glyph_chars=${glyphChars}; written_bytes=${session.written_bytes()}`,
      );
    }
  }

  canvas.addEventListener("keydown", handleTerminalKeydown);
  canvas.addEventListener("keyup", handleTerminalKeyup);
  imeInput.addEventListener("keydown", handleTerminalKeydown);
  imeInput.addEventListener("keyup", handleTerminalKeyup);

  let localMouseSelectionActive = false;

  function mouseEventOffsets(event) {
    const rect = canvas.getBoundingClientRect();
    const offsetX = Number.isFinite(event.clientX)
      ? event.clientX - rect.left
      : Number.isFinite(event.offsetX)
        ? event.offsetX
        : 0;
    const offsetY = Number.isFinite(event.clientY)
      ? event.clientY - rect.top
      : Number.isFinite(event.offsetY)
        ? event.offsetY
        : 0;
    return { offsetX, offsetY };
  }

  function beginLocalSelection(event) {
    if (
      !session.mouse_reporting_active() ||
      mouseSelectionOverridePolicy !== "shift-select" ||
      !event.shiftKey ||
      event.button !== 0
    ) {
      return false;
    }

    const { offsetX, offsetY } = mouseEventOffsets(event);
    session.begin_local_selection(offsetX, offsetY, event.detail || 1);
    localMouseSelectionActive = true;
    event.preventDefault();
    setStatus(
      "ok",
      `rendered; local_selection=${session.selection_range_text()}`,
    );
    return true;
  }

  function updateLocalSelection(event) {
    if (!localMouseSelectionActive) {
      return false;
    }

    const { offsetX, offsetY } = mouseEventOffsets(event);
    session.update_local_selection(offsetX, offsetY);
    event.preventDefault();
    setStatus(
      "ok",
      `rendered; local_selection=${session.selection_range_text()}`,
    );
    return true;
  }

  function endLocalSelection(event) {
    if (!localMouseSelectionActive) {
      return false;
    }

    updateLocalSelection(event);
    session.end_local_selection();
    localMouseSelectionActive = false;
    event.preventDefault();
    setStatus(
      "ok",
      `rendered; selected_text=${session.selected_text()}`,
    );
    return true;
  }

  function activateHyperlink(event) {
    if (event.button !== 0 || (!event.ctrlKey && !event.metaKey)) {
      return false;
    }

    const { offsetX, offsetY } = mouseEventOffsets(event);
    const target = JSON.parse(session.hyperlink_activation_target_json(offsetX, offsetY));
    if (!target.hit) {
      return false;
    }

    event.preventDefault();
    if (!target.allowed) {
      window.wittyLastHyperlinkOpen = {
        hit: true,
        opened: false,
        blocked: false,
        reason: target.reason || "URL is not allowed",
        uri: "",
      };
      setStatus("failed", `hyperlink blocked: ${window.wittyLastHyperlinkOpen.reason}`);
      return true;
    }

    let popup = null;
    try {
      popup = window.open(target.uri, "_blank", "noopener,noreferrer");
    } catch (error) {
      window.wittyLastHyperlinkOpen = {
        hit: true,
        opened: false,
        blocked: false,
        reason: String(error?.message ?? error),
        uri: target.uri,
      };
      setStatus("failed", `hyperlink open failed: ${window.wittyLastHyperlinkOpen.reason}`);
      return true;
    }

    window.wittyLastHyperlinkOpen = {
      hit: true,
      opened: popup !== null,
      blocked: popup === null,
      reason: popup === null ? "popup blocked" : "",
      uri: target.uri,
    };
    setStatus(
      popup === null ? "failed" : "ok",
      popup === null ? "hyperlink popup blocked" : "hyperlink opened",
    );
    return true;
  }

  function selectCommandBlockFromGutter(event) {
    if (event.button !== 0 && event.button !== 2) {
      return false;
    }

    const { offsetX, offsetY } = mouseEventOffsets(event);
    const selected = selectCommandBlockGutterHit(session, offsetX, offsetY);
    if (selected === null) {
      return false;
    }

    canvas.style.cursor = "pointer";
    event.preventDefault();
    if (event.button === 2) {
      updateCommandBlockActionMenuStatus(
        session,
        session.open_command_block_action_menu(),
      );
    } else {
      setStatus("ok", `rendered; command_block=selected; id=${selected.id}`);
    }
    return true;
  }

  function handleLocalWheel(event) {
    const reportingActive = session.mouse_reporting_active();
    const overrideScrollback =
      reportingActive &&
      mouseSelectionOverridePolicy === "shift-select" &&
      event.shiftKey;
    if (reportingActive && !overrideScrollback) {
      return false;
    }

    event.preventDefault();
    const deltaMode = Number.isFinite(event.deltaMode) ? event.deltaMode : 0;
    window.wittyLastLocalWheelError = null;
    let handled = false;
    try {
      handled = session.handle_mouse(
        "localwheel",
        -1,
        0,
        0,
        0,
        event.deltaY,
        event.shiftKey,
        event.altKey,
        event.ctrlKey,
        deltaMode,
      );
    } catch (error) {
      window.wittyLastLocalWheelError = String(error?.stack ?? error);
      setStatus("failed", "local wheel scrollback failed");
      return true;
    }
    if (handled) {
      setStatus("ok", "rendered; local_wheel=scrollback");
    } else {
      setStatus("ok", "rendered; local_wheel=boundary");
    }
    return true;
  }

  function handleMouseEvent(kind, event, deltaY = 0) {
    const { offsetX, offsetY } = mouseEventOffsets(event);
    const hyperlinkHoverChanged = session.update_hyperlink_hover(offsetX, offsetY);
    const commandBlockHoverChanged =
      session.update_command_block_gutter_hover(offsetX, offsetY);
    const commandBlockGutterHit = JSON.parse(
      session.command_block_gutter_hit_json(offsetX, offsetY),
    );
    canvas.style.cursor = commandBlockGutterHit.hit ? "pointer" : "";
    const hoverChanged = hyperlinkHoverChanged || commandBlockHoverChanged;
    const handled = session.handle_mouse(
      kind,
      Number.isFinite(event.button) ? event.button : -1,
      Number.isFinite(event.buttons) ? event.buttons : 0,
      offsetX,
      offsetY,
      deltaY,
      event.shiftKey,
      event.altKey,
      event.ctrlKey,
      Number.isFinite(event.deltaMode) ? event.deltaMode : 0,
    );
    if (!handled) {
      if (hoverChanged) {
        setStatus("ok", "rendered; pointer=hover");
      }
      return false;
    }

    flushGatewayInput(session);
    event.preventDefault();
    setStatus(
      "ok",
      `rendered; mouse=handled; written_bytes=${session.written_bytes()}`,
    );
    return true;
  }

  canvas.addEventListener("pointerdown", (event) => {
    focusTerminalInput(session, canvas, imeInput);
    if (
      activateHyperlink(event) ||
      selectCommandBlockFromGutter(event) ||
      beginLocalSelection(event) ||
      handleMouseEvent("pointerdown", event)
    ) {
      if (typeof canvas.setPointerCapture === "function") {
        try {
          canvas.setPointerCapture(event.pointerId);
        } catch {
          // Synthetic smoke events do not always create an active pointer.
        }
      }
    }
  });

  canvas.addEventListener("pointerup", (event) => {
    const handled = endLocalSelection(event) || handleMouseEvent("pointerup", event);
    if (
      handled &&
      typeof canvas.releasePointerCapture === "function" &&
      canvas.hasPointerCapture(event.pointerId)
    ) {
      try {
        canvas.releasePointerCapture(event.pointerId);
      } catch {
        // The capture may already be gone if the browser cancelled the pointer.
      }
    }
  });

  canvas.addEventListener("pointermove", (event) => {
    if (!updateLocalSelection(event)) {
      handleMouseEvent("pointermove", event);
    }
  });

  canvas.addEventListener(
    "wheel",
    (event) => {
      if (!handleLocalWheel(event)) {
        handleMouseEvent("wheel", event, event.deltaY);
      }
    },
    { passive: false },
  );

  canvas.addEventListener("contextmenu", (event) => {
    const { offsetX, offsetY } = mouseEventOffsets(event);
    const commandBlockGutterHit = JSON.parse(
      session.command_block_gutter_hit_json(offsetX, offsetY),
    );
    if (
      session.mouse_reporting_active() ||
      session.command_block_action_menu_is_open() ||
      commandBlockGutterHit.hit
    ) {
      event.preventDefault();
    }
  });

  function handleFocusEvent(focused) {
    const handled = session.handle_focus(focused);
    if (!handled) {
      return false;
    }

    flushGatewayInput(session);
    setStatus(
      "ok",
      `rendered; focus=${focused ? "in" : "out"}; written_bytes=${session.written_bytes()}`,
    );
    return true;
  }

  let terminalFocused = false;

  function terminalHasDomFocus() {
    return document.activeElement === canvas || document.activeElement === imeInput;
  }

  function updateTerminalDomFocus(focused, force = false) {
    if (!force && terminalFocused === focused) {
      return false;
    }
    terminalFocused = focused;
    return handleFocusEvent(focused);
  }

  function deferTerminalFocusCheck() {
    setTimeout(() => {
      updateTerminalDomFocus(terminalHasDomFocus());
    }, 0);
  }

  canvas.addEventListener("focus", (event) => {
    updateTerminalDomFocus(true, !event.isTrusted);
    if (event.isTrusted) {
      focusTerminalInput(session, canvas, imeInput);
    } else {
      syncImeInputPosition(session, canvas, imeInput);
    }
  });

  canvas.addEventListener("blur", (event) => {
    if (event.isTrusted) {
      deferTerminalFocusCheck();
    } else {
      updateTerminalDomFocus(false, true);
    }
  });

  imeInput.addEventListener("focus", () => {
    updateTerminalDomFocus(true);
    syncImeInputPosition(session, canvas, imeInput);
  });

  imeInput.addEventListener("blur", () => {
    deferTerminalFocusCheck();
  });

  if (!profilePickerBootstrap && !profileImportBootstrap) {
    focusTerminalInput(session, canvas, imeInput);
    setStatus(
      "ok",
      `rendered; glyph_chars=${glyphChars}; written_bytes=${writtenBytes}; grid=${session.grid_cols()}x${session.grid_rows()}`,
    );
  }
}

main().catch((error) => {
  console.error(error);
  setStatus("failed", String(error?.stack ?? error));
});
