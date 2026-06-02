// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

const state = {
  config: null,
  authenticated: false,
  danmuConnected: false,
  vtuberRunning: false,
  chatCount: 0,
  chatLoaded: false,
  chatMessageIds: new Set(),
  chatSort: "desc",
  nextDanmuSeq: 0,
  eventCount: 0,
  qrPollTimer: null,
  streamCredentialsVisible: false,
};

const $ = (selector) => document.querySelector(selector);
const els = {
  sidebar: $("#sidebar"),
  sidebarClose: $("#sidebar-close"),
  menuToggle: $("#menu-toggle"),
  serviceStatus: $("#service-status"),
  authStatus: $("#auth-status"),
  danmuStatus: $("#danmu-status"),
  vtuberStatus: $("#vtuber-status"),
  pageTitle: $("#page-title"),
  configPath: $("#config-path"),
  profile: $("#profile"),
  logout: $("#logout"),
  refresh: $("#refresh"),
  qrBox: $("#qr-box"),
  qrGenerate: $("#qr-generate"),
  qrStop: $("#qr-stop"),
  qrStatus: $("#qr-status"),
  cookieInput: $("#cookie-input"),
  cookieLogin: $("#cookie-login"),
  roomTitle: $("#room-title"),
  categoryId: $("#category-id"),
  areaId: $("#area-id"),
  bootstrap: $("#bootstrap"),
  saveTitle: $("#save-title"),
  saveArea: $("#save-area"),
  startLive: $("#start-live"),
  stopLive: $("#stop-live"),
  streamList: $("#stream-list"),
  toggleStreamCredentials: $("#toggle-stream-credentials"),
  liveConsoleOutput: $("#live-console-output"),
  clearLiveConsole: $("#clear-live-console"),
  faceAuthPanel: $("#face-auth-panel"),
  faceAuthQr: $("#face-auth-qr"),
  faceAuthStatus: $("#face-auth-status"),
  faceAuthLink: $("#face-auth-link"),
  roomId: $("#room-id"),
  uid: $("#uid"),
  host: $("#host"),
  port: $("#port"),
  connectDanmu: $("#connect-danmu"),
  disconnectDanmu: $("#disconnect-danmu"),
  refreshDanmuToken: $("#refresh-danmu-token"),
  notifyEnabled: $("#notify-enabled"),
  notifyDanmu: $("#notify-danmu"),
  notifySuperChat: $("#notify-super-chat"),
  notifyCooldown: $("#notify-cooldown"),
  notifyExpireTimeout: $("#notify-expire-timeout"),
  saveNotifications: $("#save-notifications"),
  commentMessage: $("#comment-message"),
  sendComment: $("#send-comment"),
  sortDanmu: $("#sort-danmu"),
  loadDanmuHistory: $("#load-danmu-history"),
  loadRank: $("#load-rank"),
  clearLogs: $("#clear-logs"),
  chatCount: $("#chat-count"),
  eventCount: $("#event-count"),
  chatList: $("#chat-list"),
  systemList: $("#system-list"),
  adminUid: $("#admin-uid"),
  addAdmin: $("#add-admin"),
  deleteAdmin: $("#delete-admin"),
  loadAdmins: $("#load-admins"),
  silentUid: $("#silent-uid"),
  silentHour: $("#silent-hour"),
  searchUser: $("#search-user"),
  searchUserButton: $("#search-user-button"),
  addSilent: $("#add-silent"),
  deleteSilent: $("#delete-silent"),
  loadSilent: $("#load-silent"),
  roomSilentType: $("#room-silent-type"),
  roomSilentLevel: $("#room-silent-level"),
  roomSilentMinute: $("#room-silent-minute"),
  setRoomSilent: $("#set-room-silent"),
  loadRoomSilent: $("#load-room-silent"),
  blockedKeyword: $("#blocked-keyword"),
  addBlocked: $("#add-blocked"),
  deleteBlocked: $("#delete-blocked"),
  loadBlocked: $("#load-blocked"),
  managerOutput: $("#manager-output"),
  vtuberRuntimeStatus: $("#vtuber-runtime-status"),
  vtuberOutput: $("#vtuber-output"),
  vtuberRuntimeDir: $("#vtuber-runtime-dir"),
  vtuberPython: $("#vtuber-python"),
  vtuberCharacter: $("#vtuber-character"),
  vtuberInputMode: $("#vtuber-input-mode"),
  vtuberInputAddress: $("#vtuber-input-address"),
  vtuberOutputMode: $("#vtuber-output-mode"),
  vtuberModelSelect: $("#vtuber-model-select"),
  vtuberFrameRate: $("#vtuber-frame-rate"),
  vtuberUseTensorrt: $("#vtuber-use-tensorrt"),
  vtuberInterpolation: $("#vtuber-interpolation"),
  vtuberSuperResolution: $("#vtuber-super-resolution"),
  vtuberCacheSimplify: $("#vtuber-cache-simplify"),
  vtuberRamCache: $("#vtuber-ram-cache"),
  vtuberVramCache: $("#vtuber-vram-cache"),
  vtuberExtraArgs: $("#vtuber-extra-args"),
  saveVtuber: $("#save-vtuber"),
  startVtuber: $("#start-vtuber"),
  stopVtuber: $("#stop-vtuber"),
  refreshVtuber: $("#refresh-vtuber"),
  vtuberArchitectureNote: $("#vtuber-architecture-note"),
  themeLight: $("#theme-light"),
  themeDark: $("#theme-dark"),
  toasts: $("#toasts"),
};

// Overlay element for mobile sidebar
const overlay = document.createElement("div");
overlay.className = "sidebar-overlay";
document.body.append(overlay);

bindUi();
updateDanmuSortButton();
connectEvents();
void refreshAll();

function bindUi() {
  document.querySelectorAll("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      switchTab(button.dataset.tab);
      closeSidebar();
    });
  });

  els.menuToggle.addEventListener("click", openSidebar);
  els.sidebarClose.addEventListener("click", closeSidebar);
  overlay.addEventListener("click", closeSidebar);

  document.querySelectorAll("[data-collapse]").forEach((header) => {
    header.addEventListener("click", () => {
      const card = header.closest(".card, .danmu-connect-card");
      if (card) card.classList.toggle("collapsed");
    });
  });

  els.refresh.addEventListener("click", () => void withLoading(els.refresh, refreshAll));
  els.logout.addEventListener("click", () => void logout());
  els.qrGenerate.addEventListener("click", () => void generateQr());
  els.qrStop.addEventListener("click", stopQrPolling);
  els.cookieLogin.addEventListener("click", () => void withLoading(els.cookieLogin, cookieLogin));
  els.bootstrap.addEventListener("click", () => void withLoading(els.bootstrap, bootstrap));
  els.saveTitle.addEventListener("click", () => void withLoading(els.saveTitle, saveTitle));
  els.saveArea.addEventListener("click", () => void withLoading(els.saveArea, saveArea));
  els.startLive.addEventListener("click", () => void withLoading(els.startLive, startLive));
  els.stopLive.addEventListener("click", () => void withLoading(els.stopLive, stopLive));
  els.toggleStreamCredentials.addEventListener("click", toggleStreamCredentials);
  els.streamList.addEventListener("click", (event) => void handleStreamListClick(event));
  els.clearLiveConsole.addEventListener("click", clearLiveConsole);
  els.categoryId.addEventListener("change", () => {
    const areaId = renderAreaOptions();
    void patchConfig({ category_id: els.categoryId.value, area_id: areaId });
  });
  els.areaId.addEventListener("change", () => void patchConfig({ area_id: els.areaId.value }));
  els.connectDanmu.addEventListener("click", () => void withLoading(els.connectDanmu, connectDanmu));
  els.disconnectDanmu.addEventListener("click", () => void withLoading(els.disconnectDanmu, disconnectDanmu));
  els.refreshDanmuToken.addEventListener("click", () => void withLoading(els.refreshDanmuToken, refreshDanmuToken));
  els.saveNotifications.addEventListener("click", () => void withLoading(els.saveNotifications, saveNotifications));
  els.sendComment.addEventListener("click", () => void sendComment());
  els.commentMessage.addEventListener("keydown", (event) => {
    if (event.key === "Enter") { event.preventDefault(); void sendComment(); }
  });
  els.sortDanmu.addEventListener("click", toggleDanmuSort);
  els.loadDanmuHistory.addEventListener("click", () => void withLoading(els.loadDanmuHistory, loadDanmuHistory));
  els.chatList.addEventListener("click", (event) => handleChatListClick(event));

  els.loadRank.addEventListener("click", () => void loadRank());
  els.clearLogs.addEventListener("click", clearLogs);
  els.addAdmin.addEventListener("click", () => void managerCall("POST", "/api/manager/admins", { uid: els.adminUid.value }));
  els.deleteAdmin.addEventListener("click", () => void managerCall("DELETE", `/api/manager/admins/${encodeURIComponent(els.adminUid.value)}`));
  els.loadAdmins.addEventListener("click", () => void managerCall("GET", "/api/manager/admins?page=1"));
  els.addSilent.addEventListener("click", () => void managerCall("POST", "/api/manager/silent-users", { uid: els.silentUid.value, hour: els.silentHour.value || "1" }));
  els.deleteSilent.addEventListener("click", () => void managerCall("DELETE", `/api/manager/silent-users/${encodeURIComponent(els.silentUid.value)}`));
  els.searchUserButton.addEventListener("click", () => void managerCall("GET", `/api/manager/search-users?search=${encodeURIComponent(els.searchUser.value)}`));
  els.loadSilent.addEventListener("click", () => void managerCall("GET", "/api/manager/silent-users?page=1"));
  els.setRoomSilent.addEventListener("click", () => void managerCall("POST", "/api/manager/room-silent", {
    type: els.roomSilentType.value,
    level: Number(els.roomSilentLevel.value || 1),
    minute: Number(els.roomSilentMinute.value || 0),
  }));
  els.loadRoomSilent.addEventListener("click", () => void managerCall("GET", "/api/manager/room-silent"));
  els.addBlocked.addEventListener("click", () => void managerCall("POST", "/api/manager/blocked-words", { keyword: els.blockedKeyword.value }));
  els.deleteBlocked.addEventListener("click", () => void managerCall("POST", "/api/manager/blocked-words/delete", { keyword: els.blockedKeyword.value }));
  els.loadBlocked.addEventListener("click", () => void managerCall("GET", "/api/manager/blocked-words"));
  els.saveVtuber.addEventListener("click", () => void withLoading(els.saveVtuber, saveVtuber));
  els.startVtuber.addEventListener("click", () => void withLoading(els.startVtuber, startVtuber));
  els.stopVtuber.addEventListener("click", () => void withLoading(els.stopVtuber, stopVtuber));
  els.refreshVtuber.addEventListener("click", () => void withLoading(els.refreshVtuber, refreshVtuber));
  [els.themeLight, els.themeDark].forEach((button) => {
    button.addEventListener("click", () => void setTheme(button.dataset.themeValue));
  });
}

function openSidebar() {
  els.sidebar.classList.add("open");
  overlay.classList.add("visible");
}

function closeSidebar() {
  els.sidebar.classList.remove("open");
  overlay.classList.remove("visible");
}

async function withLoading(button, fn) {
  button.classList.add("loading");
  try { await fn(); }
  catch (error) { toast(error.message, true); }
  finally { button.classList.remove("loading"); }
}

function switchTab(tab) {
  document.querySelectorAll("[data-tab]").forEach((button) => {
    button.classList.toggle("active", button.dataset.tab === tab);
  });
  document.querySelectorAll(".tab-panel").forEach((panel) => {
    panel.classList.toggle("active", panel.id === `tab-${tab}`);
  });
  els.pageTitle.textContent = ({ account: "账号", stream: "直播", comments: "弹幕", vtuber: "VTuber", manager: "管理" })[tab] || "bilive";
  if (tab === "comments" && !state.chatLoaded) {
    void loadDanmuHistory();
  }
  if (tab === "vtuber") {
    void refreshVtuber();
  }
}

async function refreshAll() {
  try {
    const [health, auth, danmu, vtuber] = await Promise.all([
      api("/api/health"),
      api("/api/auth/status"),
      api("/api/danmu/status"),
      api("/api/vtuber/status").catch((error) => ({ error: error.message })),
    ]);
    setStatus(els.serviceStatus, `v${health.version}`, health.status === "ok");
    state.authenticated = auth.authenticated;
    state.config = auth.config;
    state.danmuConnected = danmu.connected;
    state.vtuberRunning = Boolean(vtuber.running);
    const configPath = auth.config_path || "默认配置路径";
    const statePath = auth.state_path || "";
    els.configPath.textContent = configPath;
    els.configPath.title = statePath ? `配置：${configPath}\n状态：${statePath}` : configPath;
    renderConfig();
    renderVtuberStatus(vtuber);
  } catch (error) {
    toast(error.message, true);
  }
}

async function bootstrap() {
  const result = await api("/api/auth/bootstrap", { method: "POST" });
  state.config = result.config;
  state.authenticated = true;
  renderConfig();
  const warnings = Array.isArray(result.warnings) ? result.warnings : [];
  toast(warnings.length ? "账号已刷新，直播间信息未完整初始化" : "初始化完成", warnings.length > 0);
  return result;
}

async function cookieLogin() {
  const cookie = els.cookieInput.value.trim();
  if (!cookie) { toast("请先粘贴 Cookie", true); return; }
  const result = await api("/api/auth/cookie", { method: "POST", body: { cookie } });
  state.authenticated = true;
  state.config = result.config;
  els.cookieInput.value = "";
  renderConfig();
  const warnings = Array.isArray(result.warnings) ? result.warnings : [];
  toast(warnings.length ? "登录成功，直播间信息未完整初始化" : "登录成功", warnings.length > 0);
}

async function logout() {
  await api("/api/auth/logout", { method: "POST" });
  stopQrPolling();
  state.authenticated = false;
  state.config = null;
  renderConfig();
  toast("已退出");
}

async function generateQr() {
  stopQrPolling();
  els.qrBox.innerHTML = '<span>生成中</span>';
  const qr = await api("/api/auth/qrcode/generate", { method: "POST" });
  els.qrBox.innerHTML = qr.svg;
  els.qrStatus.textContent = "请使用哔哩哔哩 App 扫码";
  els.qrStop.disabled = false;
  state.qrPollTimer = window.setInterval(() => void pollQr(qr.qrcode_key), 1200);
}

async function pollQr(qrcodeKey) {
  const data = await api("/api/auth/qrcode/poll", { method: "POST", body: { qrcode_key: qrcodeKey } });
  switch (data.code) {
    case 0:
      stopQrPolling();
      els.qrStatus.textContent = "已确认，正在初始化";
      try {
        const result = await bootstrap();
        const warnings = Array.isArray(result.warnings) ? result.warnings : [];
        els.qrStatus.textContent = warnings.length ? "账号已刷新，直播间信息未完整初始化" : "初始化完成";
      } catch (error) {
        await refreshAll();
        els.qrStatus.textContent = "初始化失败，已刷新账号状态";
        toast(error.message, true);
      }
      break;
    case 86038:
      stopQrPolling();
      els.qrStatus.textContent = "二维码已失效";
      break;
    case 86090:
      els.qrStatus.textContent = "已扫码，请在手机上确认";
      break;
    default:
      els.qrStatus.textContent = "等待扫码";
  }
}

function stopQrPolling() {
  if (state.qrPollTimer) { window.clearInterval(state.qrPollTimer); state.qrPollTimer = null; }
  els.qrStop.disabled = true;
}

async function saveTitle() {
  await api("/api/live/title", { method: "POST", body: { title: els.roomTitle.value } });
  await refreshAll();
  toast("标题已更新");
}

async function saveArea() {
  await api("/api/live/area", { method: "POST", body: { area_id: els.areaId.value } });
  await refreshAll();
  toast("分区已更新");
}

async function startLive() {
  if (Number(state.config?.room_id || 0) <= 0) {
    toast("当前账号没有直播间，无法开播。请使用已开通直播间的账号登录。", true);
    return;
  }
  await patchConfig({ room_title: els.roomTitle.value, category_id: els.categoryId.value, area_id: els.areaId.value });
  const result = await api("/api/live/start", { method: "POST" });
  showLiveConsoleResult(result);
  if (result.code === 0) { await refreshAll(); toast("开播成功"); return; }
  toast(result.message || "开播需要额外验证", true);
}

async function stopLive() {
  await api("/api/live/stop", { method: "POST" });
  await refreshAll();
  toast("已停止直播");
}

async function refreshDanmuToken() {
  const roomId = Number(els.roomId.value || state.config?.room_id || 0);
  await api(`/api/live/danmu-info?room_id=${roomId}`);
  await refreshAll();
  toast("弹幕 Token 已刷新");
}

async function connectDanmu() {
  await api("/api/danmu/connect", {
    method: "POST",
    body: {
      room_id: Number(els.roomId.value || state.config?.room_id || 0),
      uid: Number(els.uid.value || state.config?.uid || 0),
      host: els.host.value,
      port: Number(els.port.value || 2243),
    },
  });
  await refreshAll();
}

async function disconnectDanmu() {
  await api("/api/danmu/disconnect", { method: "POST" });
  await refreshAll();
}

async function sendComment() {
  const message = els.commentMessage.value.trim();
  if (!message) return;
  await api("/api/live/comment", { method: "POST", body: { message } });
  els.commentMessage.value = "";
  toast("弹幕已发送");
}

async function loadDanmuHistory() {
  const roomId = Number(els.roomId.value || state.config?.room_id || 0);
  const result = await api(`/api/danmu/messages?room_id=${encodeURIComponent(roomId)}`);
  renderDanmuMessages(result.items || []);
  state.chatLoaded = true;
  if (result.recent_error) {
    toast(`已加载本地弹幕，最近历史补全失败: ${result.recent_error}`, true);
  } else {
    toast(`已加载 ${result.total || 0} 条本场弹幕`);
  }
}

async function loadRank() {
  const result = await api("/api/live/contribution-rank");
  pushSystemLog("在线榜单", JSON.stringify(result, null, 2), "info");
}

async function managerCall(method, path, body) {
  const result = await api(path, { method, body });
  showManagerResult(result);
  toast("操作完成");
}

async function patchConfig(patch) {
  const config = await api("/api/config", { method: "PATCH", body: patch });
  state.config = config;
  renderConfig();
  return config;
}

async function saveNotifications() {
  await patchConfig({
    danmu_notifications: {
      enabled: els.notifyEnabled.checked,
      danmu: els.notifyDanmu.checked,
      super_chat: els.notifySuperChat.checked,
      cooldown_secs: clampInteger(els.notifyCooldown.value, 0, 3600, 2),
      expire_timeout_ms: clampInteger(els.notifyExpireTimeout.value, 0, 3600000, 0),
    },
  });
  toast("提醒设置已保存");
}

async function saveVtuber() {
  const result = await persistVtuberConfig();
  renderVtuberStatus(result);
  toast("VTuber 设置已保存");
}

async function persistVtuberConfig() {
  const config = vtuberFormConfig();
  const result = await api("/api/vtuber/config", { method: "POST", body: { config } });
  state.config = { ...(state.config || {}), vtuber: config };
  return result;
}

async function startVtuber() {
  await persistVtuberConfig();
  const result = await api("/api/vtuber/start", { method: "POST" });
  renderVtuberStatus(result);
  toast("VTuber 已启动");
}

async function stopVtuber() {
  const result = await api("/api/vtuber/stop", { method: "POST" });
  renderVtuberStatus(result);
  toast("VTuber 已停止");
}

async function refreshVtuber() {
  const result = await api("/api/vtuber/status");
  renderVtuberStatus(result);
}

function vtuberFormConfig() {
  return {
    enabled: true,
    runtime_dir: els.vtuberRuntimeDir.value.trim(),
    python: els.vtuberPython.value.trim() || "python",
    character: els.vtuberCharacter.value.trim() || "lambda_00",
    input_mode: els.vtuberInputMode.value,
    input_address: els.vtuberInputAddress.value.trim(),
    output_mode: els.vtuberOutputMode.value,
    model_select: els.vtuberModelSelect.value.trim() || "v3_seperable_half",
    use_tensorrt: els.vtuberUseTensorrt.checked,
    frame_rate_limit: clampInteger(els.vtuberFrameRate.value, 1, 240, 30),
    interpolation: els.vtuberInterpolation.value.trim() || "Off",
    super_resolution: els.vtuberSuperResolution.value.trim() || "Off",
    ram_cache_size: els.vtuberRamCache.value.trim() || "2gb",
    vram_cache_size: els.vtuberVramCache.value.trim() || "2gb",
    cache_simplify: clampInteger(els.vtuberCacheSimplify.value, 0, 16, 3),
    extra_args: splitArgs(els.vtuberExtraArgs.value),
  };
}

function renderConfig() {
  const config = state.config || {};
  setStatus(els.authStatus, state.authenticated ? (config.username || "已登录") : "未登录", state.authenticated);
  setStatus(els.danmuStatus, state.danmuConnected ? "已连接" : "未连接", state.danmuConnected);
  els.logout.disabled = !state.authenticated;

  if (state.authenticated) {
    els.profile.innerHTML = `<span>${escapeHtml(config.username || "未知用户")} · UID ${escapeHtml(config.uid || 0)} · 房间 ${escapeHtml(config.room_id || 0)}</span>`;
  } else {
    els.profile.innerHTML = '<span class="profile-empty">未登录</span>';
  }
  els.roomTitle.value = config.room_title || "";
  els.roomId.value = config.room_id || "";
  els.uid.value = config.uid || "";
  els.startLive.disabled = !state.authenticated || Number(config.room_id || 0) <= 0;
  applyTheme(config.theme || "dark");
  renderNotificationSettings(config.danmu_notifications || {});
  renderCategoryOptions();
  renderStreamList();
  renderVtuberConfig(config.vtuber || {});
}

function renderNotificationSettings(settings) {
  els.notifyEnabled.checked = Boolean(settings.enabled);
  els.notifyDanmu.checked = settings.danmu !== false;
  els.notifySuperChat.checked = settings.super_chat !== false;
  els.notifyCooldown.value = String(clampInteger(settings.cooldown_secs, 0, 3600, 2));
  els.notifyExpireTimeout.value = String(clampInteger(settings.expire_timeout_ms, 0, 3600000, 0));
}

function renderVtuberConfig(config) {
  els.vtuberRuntimeDir.value = config.runtime_dir || "";
  els.vtuberPython.value = config.python || "python";
  els.vtuberCharacter.value = config.character || "lambda_00";
  els.vtuberInputMode.value = config.input_mode || "mouse";
  els.vtuberInputAddress.value = config.input_address || "";
  els.vtuberOutputMode.value = config.output_mode || "debug";
  els.vtuberModelSelect.value = config.model_select || "v3_seperable_half";
  els.vtuberFrameRate.value = String(clampInteger(config.frame_rate_limit, 1, 240, 30));
  els.vtuberUseTensorrt.checked = Boolean(config.use_tensorrt);
  els.vtuberInterpolation.value = config.interpolation || "Off";
  els.vtuberSuperResolution.value = config.super_resolution || "Off";
  els.vtuberCacheSimplify.value = String(clampInteger(config.cache_simplify, 0, 16, 3));
  els.vtuberRamCache.value = config.ram_cache_size || "2gb";
  els.vtuberVramCache.value = config.vram_cache_size || "2gb";
  els.vtuberExtraArgs.value = Array.isArray(config.extra_args) ? config.extra_args.join(" ") : "";
}

function renderVtuberStatus(status) {
  if (status?.error) {
    setStatus(els.vtuberStatus, "不可用", false);
    els.vtuberRuntimeStatus.textContent = "不可用";
    els.vtuberRuntimeStatus.className = "status-badge";
    els.vtuberOutput.textContent = status.error;
    return;
  }

  const running = Boolean(status?.running);
  const configured = Boolean(status?.configured);
  state.vtuberRunning = running;
  setStatus(els.vtuberStatus, running ? "运行中" : "未启动", running);
  els.vtuberRuntimeStatus.textContent = running ? `运行中${status.pid ? ` · PID ${status.pid}` : ""}` : configured ? "已配置" : "未配置";
  els.vtuberRuntimeStatus.className = `status-badge ${running ? "ok" : configured ? "ready" : ""}`;
  els.stopVtuber.disabled = !running;
  els.startVtuber.disabled = running;
  els.vtuberOutput.textContent = JSON.stringify({
    configured,
    running,
    pid: status?.pid || null,
    command: status?.command || [],
  }, null, 2);
  renderVtuberRecommendation(status?.recommendation);
}

function renderVtuberRecommendation(recommendation) {
  if (!recommendation) {
    els.vtuberArchitectureNote.textContent = "暂无判断";
    return;
  }
  const items = Array.isArray(recommendation.rationale) ? recommendation.rationale : [];
  els.vtuberArchitectureNote.innerHTML = `
    <div><strong>Rust 重写：</strong>${escapeHtml(recommendation.rust_rewrite || "")}</div>
    <div><strong>并入 bilive：</strong>${escapeHtml(recommendation.merge_into_bilive || "")}</div>
    ${items.map((item) => `<p>${escapeHtml(item)}</p>`).join("")}
  `;
}

async function setTheme(theme) {
  applyTheme(theme);
  await patchConfig({ theme });
}

function applyTheme(theme) {
  const selected = theme === "dark" ? "dark" : "light";
  document.documentElement.dataset.theme = selected;
  els.themeLight.classList.toggle("active", selected === "light");
  els.themeDark.classList.toggle("active", selected === "dark");
}

function renderCategoryOptions() {
  const areas = Array.isArray(state.config?.area_list) ? state.config.area_list : [];
  const configCategoryId = selectValue(state.config?.category_id);
  const fallbackCategoryId = selectValue(areas[0]?.id);
  const selected = areas.some((area) => selectValue(area.id) === configCategoryId) ? configCategoryId : fallbackCategoryId;
  els.categoryId.innerHTML = areas
    .map((area) => `<option value="${escapeHtml(selectValue(area.id))}">${escapeHtml(area.name)}</option>`)
    .join("");
  els.categoryId.value = selected;
  renderAreaOptions();
}

function renderAreaOptions() {
  const areas = Array.isArray(state.config?.area_list) ? state.config.area_list : [];
  const parent = areas.find((area) => selectValue(area.id) === els.categoryId.value) || areas[0];
  const children = Array.isArray(parent?.list) ? parent.list : [];
  const configAreaId = selectValue(state.config?.area_id);
  const fallbackAreaId = selectValue(children[0]?.id);
  const selected = children.some((area) => selectValue(area.id) === configAreaId) ? configAreaId : fallbackAreaId;
  els.areaId.innerHTML = children
    .map((area) => `<option value="${escapeHtml(selectValue(area.id))}">${escapeHtml(area.name)}</option>`)
    .join("");
  els.areaId.value = selected;
  return selected;
}

function renderStreamList() {
  const streams = uniqueStreams(Array.isArray(state.config?.streams) ? state.config.streams : []);
  if (streams.length === 0) {
    els.streamList.textContent = "暂无推流凭证";
    els.toggleStreamCredentials.disabled = true;
    els.toggleStreamCredentials.textContent = "显示";
    return;
  }
  els.toggleStreamCredentials.disabled = false;
  els.toggleStreamCredentials.textContent = state.streamCredentialsVisible ? "隐藏" : "显示";
  els.streamList.innerHTML = streams
    .map((stream, index) => {
      const address = String(stream.address || "");
      const streamCode = String(stream.key || "");
      const fullUrl = streamUrl(address, streamCode);
      const fullUrlText = credentialText(fullUrl);
      const addressText = credentialText(address);
      const streamCodeText = credentialText(streamCode);
      return `
      <article class="stream-item">
        <div class="stream-header">
          <strong>${escapeHtml(stream.type)}</strong>
          <div class="stream-actions">
            <button class="btn btn-ghost btn-sm" data-copy-stream="${index}" data-copy-field="fullUrl" type="button">复制完整地址</button>
            <button class="btn btn-ghost btn-sm" data-copy-stream="${index}" data-copy-field="address" type="button">复制推流地址</button>
            <button class="btn btn-ghost btn-sm" data-copy-stream="${index}" data-copy-field="streamCode" type="button">复制推流码</button>
            <button class="btn btn-ghost btn-sm" data-test-stream="${index}" type="button">测试推流</button>
          </div>
        </div>
        <div class="credential-row"><span>完整推流地址</span><code>${escapeHtml(fullUrlText)}</code></div>
        <div class="credential-row"><span>推流地址</span><code>${escapeHtml(addressText)}</code></div>
        <div class="credential-row"><span>推流码</span><code>${escapeHtml(streamCodeText)}</code></div>
      </article>`;
    })
    .join("");
}

function toggleStreamCredentials() {
  state.streamCredentialsVisible = !state.streamCredentialsVisible;
  renderStreamList();
}

async function handleStreamListClick(event) {
  const testButton = event.target.closest("[data-test-stream]");
  if (testButton) { await testStream(Number(testButton.dataset.testStream), testButton); return; }
  const button = event.target.closest("[data-copy-stream]");
  if (!button) return;
  const streams = uniqueStreams(Array.isArray(state.config?.streams) ? state.config.streams : []);
  const stream = streams[Number(button.dataset.copyStream)];
  if (!stream) return;
  const address = String(stream.address || "");
  const streamCode = String(stream.key || "");
  const values = { fullUrl: streamUrl(address, streamCode), address, streamCode };
  await copyText(values[button.dataset.copyField] || "");
}

async function testStream(index, button) {
  const label = button.textContent;
  button.disabled = true;
  button.textContent = "测试中";
  showLiveConsoleResult({ message: "测试推流中", duration_seconds: 5 });
  try {
    const result = await api("/api/live/test-stream", { method: "POST", body: { index } });
    showLiveConsoleResult(result);
    toast(result.warning ? "测试推流完成，收尾警告已忽略" : "测试推流完成");
  } catch (error) {
    showLiveConsoleResult({ ok: false, error: error.message });
    toast(error.message, true);
  } finally {
    button.disabled = false;
    button.textContent = label;
  }
}

function credentialText(value) {
  return state.streamCredentialsVisible ? (value || "") : (value ? "****************" : "");
}

function uniqueStreams(streams) {
  const seen = new Set();
  return streams.filter((stream) => {
    const key = `${stream.address || ""}\n${stream.key || ""}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function streamUrl(address, key) {
  if (!address || !key) return address || key;
  if (key.startsWith("?") || key.startsWith("&")) return `${address}${key}`;
  return `${address}${address.endsWith("/") ? "" : "/"}${key}`;
}

function connectEvents() {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const socket = new WebSocket(`${protocol}//${window.location.host}/api/events`);

  socket.addEventListener("open", () => {});
  socket.addEventListener("close", () => {
    window.setTimeout(connectEvents, 1500);
  });
  socket.addEventListener("error", () => {});
  socket.addEventListener("message", (message) => {
    const event = tryParseJson(message.data);
    if (!event) return;
    if (event.type === "connection") {
      state.danmuConnected = event.payload === "connected";
      setStatus(els.danmuStatus, state.danmuConnected ? "已连接" : "未连接", state.danmuConnected);
    }
    appendEvent(event);
  });
}

function appendEvent(event) {
  const receivedAt = new Date();
  if (event.type === "connection") {
    pushSystemLog("弹幕连接", connectionLabel(event.payload), event.payload === "connected" ? "ok" : "info", null, receivedAt);
    return;
  }
  if (event.type === "error") {
    pushSystemLog("服务错误", event.payload?.message || "未知错误", "error", null, receivedAt);
    return;
  }
  const payload = event.payload?.payload;
  if (!payload) {
    pushSystemLog("未知事件", JSON.stringify(event), "info", null, receivedAt);
    return;
  }
  const parsed = tryParseJson(payload);
  if (String(parsed?.cmd || "").startsWith("DANMU_MSG")) {
    pushDanmuMessage(parseDanmuMessage(parsed), payload, receivedAt);
    return;
  }
  if (parsed?.cmd === "SUPER_CHAT_MESSAGE") {
    pushDanmuMessage(parseSuperChatMessage(parsed), payload, receivedAt);
    return;
  }
  pushSystemEvent(parsed, payload, receivedAt);
}

function renderDanmuMessages(items) {
  state.chatMessageIds = new Set();
  state.chatCount = 0;
  els.chatList.replaceChildren();

  const entries = sortedDanmuEntries(items);
  state.nextDanmuSeq = entries.reduce((next, entry, index) => Math.max(next, entryReceivedSeq(entry, index) + 1), 0);

  for (const [index, entry] of entries.entries()) {
    const parsed = tryParseJson(entry.payload);
    if (!parsed) continue;
    const receivedAt = entry.received_at ? new Date(Number(entry.received_at)) : new Date();
    const options = { id: entry.id, timeline: entry.timeline || "", receivedSeq: entryReceivedSeq(entry, index), scroll: false };
    if (String(parsed.cmd || "").startsWith("DANMU_MSG")) {
      pushDanmuMessage(parseDanmuMessage(parsed), entry.payload, receivedAt, options);
    } else if (parsed.cmd === "SUPER_CHAT_MESSAGE") {
      pushDanmuMessage(parseSuperChatMessage(parsed), entry.payload, receivedAt, options);
    }
  }

  if (state.chatCount === 0) {
    renderEmpty(els.chatList, "暂无弹幕", "chat");
  } else if (state.chatSort === "asc") {
    scrollChatToBottom();
  } else {
    scrollChatToTop();
  }
  updateDanmuCounters();
  updateDanmuSortButton();
}

function pushDanmuMessage(message, raw, receivedAt, options = {}) {
  const parsed = tryParseJson(raw);
  const id = options.id || danmuMessageId(parsed, raw);
  if (id && state.chatMessageIds.has(id)) return false;
  const cardTime = danmuCardTime(parsed, receivedAt.getTime(), options.timeline || "");
  const receivedSeq = options.receivedSeq ?? nextDanmuReceivedSeq();
  const shouldStickToEdge = options.scroll !== false && (state.chatSort === "asc" ? isNearBottom(els.chatList) : isNearTop(els.chatList));
  const previousHeight = els.chatList.scrollHeight;
  const empty = els.chatList.querySelector(".empty-state");
  if (empty) empty.remove();

  const item = document.createElement("article");
  item.className = `danmu-message ${message.tone || ""}`;
  item.dataset.sortAt = String(cardTime.sortAt);
  item.dataset.receivedSeq = String(receivedSeq);
  item.dataset.messageId = id || "";
  const avatar = document.createElement("div");
  const main = document.createElement("div");
  const meta = document.createElement("div");
  const name = document.createElement("strong");
  const time = document.createElement("time");
  const text = document.createElement("p");
  const reply = document.createElement("button");

  avatar.className = "danmu-avatar";
  avatar.textContent = firstGlyph(message.name);
  main.className = "danmu-message-main";
  meta.className = "danmu-message-meta";
  name.className = "danmu-name";
  name.textContent = message.name;
  time.textContent = cardTime.text;
  text.className = "danmu-text";
  text.textContent = message.content || "(空消息)";
  reply.className = "danmu-reply";
  reply.type = "button";
  reply.dataset.replyTo = message.name;
  reply.textContent = "回复";

  meta.append(name);
  if (message.medal) meta.append(pill(message.medal, "danmu-medal"));
  if (message.price) meta.append(pill(message.price, "danmu-price"));
  if (message.color) meta.append(colorSwatch(message.color));
  meta.append(reply, time);
  main.append(meta, text, rawDetails(raw));
  item.append(avatar, main);
  insertDanmuItem(item);

  if (id) state.chatMessageIds.add(id);
  state.chatCount += 1;
  updateDanmuCounters();
  if (options.scroll !== false) {
    if (state.chatSort === "asc") {
      if (shouldStickToEdge) scrollChatToBottom();
    } else if (shouldStickToEdge) {
      scrollChatToTop();
    } else {
      els.chatList.scrollTop += els.chatList.scrollHeight - previousHeight;
    }
  }
  return true;
}

function pushSystemEvent(parsed, raw, receivedAt) {
  const event = describeSystemEvent(parsed, raw);
  pushSystemLog(event.title, event.body, event.tone, event.includeRaw === false ? null : raw, receivedAt);
}

function pushSystemLog(title, body, tone = "info", raw = null, receivedAt = new Date()) {
  const empty = els.systemList.querySelector(".empty-state");
  if (empty) empty.remove();

  const item = document.createElement("article");
  item.className = `log-item ${tone}`;
  const meta = document.createElement("div");
  const heading = document.createElement("strong");
  const time = document.createElement("time");
  const pre = document.createElement("pre");
  heading.textContent = title;
  time.textContent = receivedAt.toLocaleTimeString();
  pre.textContent = body;
  meta.append(heading, time);
  item.append(meta, pre);
  if (raw) item.append(rawDetails(raw));
  els.systemList.prepend(item);

  state.eventCount += 1;
  updateDanmuCounters();
  while (els.systemList.children.length > 120) els.systemList.lastElementChild.remove();
}

function clearLogs() {
  renderEmpty(els.chatList, "暂无弹幕", "chat");
  renderEmpty(els.systemList, "暂无事件", "system");
  state.chatCount = 0;
  state.chatLoaded = false;
  state.chatMessageIds = new Set();
  state.nextDanmuSeq = 0;
  state.eventCount = 0;
  updateDanmuCounters();
}

function toggleDanmuSort() {
  state.chatSort = state.chatSort === "asc" ? "desc" : "asc";
  const hasMessages = sortRenderedDanmuMessages();
  updateDanmuSortButton();
  if (!hasMessages) return;
  if (state.chatSort === "asc") {
    scrollChatToBottom();
  } else {
    scrollChatToTop();
  }
}

function updateDanmuSortButton() {
  els.sortDanmu.textContent = state.chatSort === "asc" ? "早到晚" : "晚到早";
  els.sortDanmu.title = state.chatSort === "asc" ? "当前按时间从早到晚排序" : "当前按时间从晚到早排序";
  els.sortDanmu.dataset.sort = state.chatSort;
}

function handleChatListClick(event) {
  const button = event.target.closest("[data-reply-to]");
  if (!button) return;
  const name = button.dataset.replyTo || "";
  const prefix = name ? `@${name} ` : "";
  els.commentMessage.value = prefix;
  els.commentMessage.focus();
  els.commentMessage.setSelectionRange(prefix.length, prefix.length);
}

function parseDanmuMessage(parsed) {
  const info = Array.isArray(parsed.info) ? parsed.info : [];
  const meta = Array.isArray(info[0]) ? info[0] : [];
  const user = Array.isArray(info[2]) ? info[2] : [];
  const medal = Array.isArray(info[3]) ? info[3] : [];
  const extra = extractDanmuExtra(meta);
  const medalName = medal[1] ? `${medal[1]} ${medal[0] || ""}`.trim() : "";

  return {
    name: String(user[1] || extra?.uname || "匿名用户"),
    content: String(info[1] || extra?.content || ""),
    medal: medalName,
    color: normalizeDanmuColor(meta[3] ?? extra?.color),
    tone: extra?.send_from_me ? "mine" : "",
  };
}

function parseSuperChatMessage(parsed) {
  const data = parsed.data || {};
  return {
    name: String(data.user_info?.uname || "匿名用户"),
    content: String(data.message || ""),
    medal: "醒目留言",
    price: data.price ? `¥${data.price}` : "",
    color: normalizeDanmuColor(data.background_color),
    tone: "highlight",
  };
}

function danmuMessageId(parsed, raw) {
  const cmd = String(parsed?.cmd || "");
  if (cmd.startsWith("DANMU_MSG")) {
    const info = Array.isArray(parsed.info) ? parsed.info : [];
    const meta = Array.isArray(info[0]) ? info[0] : [];
    const user = Array.isArray(info[2]) ? info[2] : [];
    return `danmu:${valueText(user[0])}:${valueText(info[1])}:${valueText(meta[4] ?? meta[13])}`;
  }
  if (cmd === "SUPER_CHAT_MESSAGE") {
    const data = parsed.data || {};
    const uid = data.uid ?? data.user_info?.uid ?? "";
    return `super_chat:${valueText(data.id ?? data.message_id)}:${valueText(uid)}:${valueText(data.message)}`;
  }
  return raw ? `raw:${raw}` : "";
}

function sortedDanmuEntries(items) {
  const entries = Array.isArray(items) ? items.map((entry, index) => ({ entry, index })) : [];
  entries.sort((left, right) => compareDanmuSortKeys(entrySortKey(left.entry, left.index), entrySortKey(right.entry, right.index)));
  return entries.map(({ entry }) => entry);
}

function entrySortKey(entry, fallbackSeq = 0) {
  const parsed = tryParseJson(entry?.payload);
  const cardTime = danmuCardTime(parsed, Number(entry?.sent_at) || Number(entry?.received_at) || 0, entry?.timeline || "");
  return {
    sortAt: cardTime.sortAt,
    receivedSeq: entryReceivedSeq(entry, fallbackSeq),
    id: String(entry?.id || ""),
  };
}

function insertDanmuItem(item) {
  const messages = Array.from(els.chatList.querySelectorAll(".danmu-message"));
  for (const message of messages) {
    const comparison = compareDanmuElements(message, item);
    if (state.chatSort === "asc" ? comparison > 0 : comparison < 0) {
      els.chatList.insertBefore(item, message);
      return;
    }
  }
  els.chatList.append(item);
}

function sortRenderedDanmuMessages() {
  const messages = Array.from(els.chatList.querySelectorAll(".danmu-message"));
  if (messages.length === 0) return false;
  messages.sort(compareDanmuElements);
  if (state.chatSort === "desc") messages.reverse();
  els.chatList.replaceChildren(...messages);
  return true;
}

function compareDanmuElements(left, right) {
  return compareDanmuSortKeys(elementSortKey(left), elementSortKey(right));
}

function compareDanmuSortKeys(left, right) {
  return compareNumber(left.sortAt, right.sortAt)
    || compareNumber(left.receivedSeq, right.receivedSeq)
    || left.id.localeCompare(right.id);
}

function compareNumber(left, right) {
  const leftValue = Number.isFinite(left) ? left : 0;
  const rightValue = Number.isFinite(right) ? right : 0;
  return leftValue - rightValue;
}

function elementSortKey(element) {
  return {
    sortAt: Number(element.dataset.sortAt || 0),
    receivedSeq: Number(element.dataset.receivedSeq || 0),
    id: element.dataset.messageId || "",
  };
}

function entryReceivedSeq(entry, fallback = 0) {
  const value = Number(entry?.received_seq);
  return Number.isFinite(value) && value >= 0 ? value : fallback;
}

function nextDanmuReceivedSeq() {
  const value = state.nextDanmuSeq;
  state.nextDanmuSeq += 1;
  return value;
}

function danmuCardTime(parsed, fallbackMillis, timeline = "") {
  const sentAt = danmuSortAt(parsed, fallbackMillis);
  const text = timeline || new Date(sentAt).toLocaleTimeString();
  return { text, sortAt: cardTimeSortAt(text, sentAt) };
}

function cardTimeSortAt(text, fallbackMillis) {
  const trimmed = String(text || "").trim();
  if (!trimmed) return fallbackMillis;

  const normalized = trimmed.replace(" ", "T");
  const parsed = Date.parse(normalized);
  if (Number.isFinite(parsed)) return parsed;

  const match = /^(\d{1,2}):(\d{2})(?::(\d{2}))?$/.exec(trimmed);
  if (match) {
    const base = new Date(fallbackMillis || Date.now());
    base.setHours(Number(match[1]), Number(match[2]), Number(match[3] || 0), 0);
    return base.getTime();
  }

  return fallbackMillis;
}

function danmuSortAt(parsed, fallbackMillis) {
  const cmd = String(parsed?.cmd || "");
  if (cmd.startsWith("DANMU_MSG")) {
    const info = Array.isArray(parsed.info) ? parsed.info : [];
    const meta = Array.isArray(info[0]) ? info[0] : [];
    return epochMillis(info[9]?.ts ?? meta[4] ?? meta[13]) || fallbackMillis;
  }
  if (cmd === "SUPER_CHAT_MESSAGE") {
    const data = parsed.data || {};
    return epochMillis(data.ts ?? data.start_time ?? data.time) || fallbackMillis;
  }
  return fallbackMillis;
}

function epochMillis(value) {
  const number = Number(value);
  if (!Number.isFinite(number) || number <= 0) return 0;
  return number >= 100000000000 ? number : number * 1000;
}

function clampInteger(value, min, max, fallback) {
  const number = Number(value);
  if (!Number.isFinite(number)) return fallback;
  return Math.min(max, Math.max(min, Math.trunc(number)));
}

function valueText(value) {
  if (value === null || value === undefined) return "";
  if (typeof value === "object") return "";
  return String(value);
}

function extractDanmuExtra(meta) {
  const holder = meta.find((item) => item && typeof item === "object" && typeof item.extra === "string");
  return holder ? tryParseJson(holder.extra) : null;
}

function describeSystemEvent(parsed, raw) {
  if (!parsed) return { title: "原始事件", body: raw.slice(0, 240), tone: "info" };

  const data = parsed.data || {};
  switch (parsed.cmd) {
    case "ONLINE_RANK_COUNT":
      return {
        title: "在线人数",
        body: data.online_count_text || data.count_text || `${data.online_count || data.count || 0}`,
        tone: "ok",
        includeRaw: false,
      };
    case "WATCHED_CHANGE":
      return { title: "看过人数", body: data.text_large || data.text_small || "已更新", tone: "info", includeRaw: false };
    case "INTERACT_WORD":
    case "INTERACT_WORD_V2":
      return { title: "互动", body: data.uname ? `${data.uname} 进入直播间` : "进入直播间", tone: "info", includeRaw: false };
    case "SEND_GIFT":
      return {
        title: "礼物",
        body: `${data.uname || "用户"} 送出 ${data.giftName || "礼物"} x${data.num || 1}`,
        tone: "gift",
        includeRaw: false,
      };
    case "ONLINE_RANK_V3":
      return { title: "在线榜单", body: "榜单已更新", tone: "info", includeRaw: false };
    case "STOP_LIVE_ROOM_LIST":
      return { title: "推荐列表", body: "列表已更新", tone: "info", includeRaw: false };
    default:
      return { title: parsed.cmd ? `事件 ${parsed.cmd}` : "系统事件", body: raw.slice(0, 240), tone: "info" };
  }
}

function pill(text, className) {
  const item = document.createElement("span");
  item.className = className;
  item.textContent = text;
  return item;
}

function colorSwatch(color) {
  const item = document.createElement("span");
  item.className = "danmu-color";
  item.style.backgroundColor = color;
  item.title = color;
  return item;
}

function rawDetails(raw) {
  const details = document.createElement("details");
  const summary = document.createElement("summary");
  const pre = document.createElement("pre");
  details.className = "raw-details";
  summary.textContent = "原始数据";
  pre.textContent = raw;
  details.append(summary, pre);
  return details;
}

function renderEmpty(container, text, kind) {
  const empty = document.createElement("div");
  const mark = document.createElement("span");
  const label = document.createElement("span");
  empty.className = `empty-state ${kind === "chat" ? "chat-empty" : "system-empty"}`;
  mark.className = "empty-mark";
  label.textContent = text;
  empty.append(mark, label);
  container.replaceChildren(empty);
}

function updateDanmuCounters() {
  els.chatCount.textContent = `${state.chatCount} 条`;
  els.eventCount.textContent = `${state.eventCount} 条`;
}

function isNearBottom(element) {
  return element.scrollHeight - element.scrollTop - element.clientHeight < 48;
}

function isNearTop(element) {
  return element.scrollTop < 48;
}

function scrollChatToBottom() {
  els.chatList.scrollTop = els.chatList.scrollHeight;
}

function scrollChatToTop() {
  els.chatList.scrollTop = 0;
}

function firstGlyph(value) {
  return Array.from(String(value || "?").trim())[0] || "?";
}

function normalizeDanmuColor(value) {
  if (value === null || value === undefined || value === "" || String(value) === "16777215") return "";
  if (typeof value === "string" && value.startsWith("#")) return value;
  const number = Number(value);
  if (!Number.isFinite(number) || number <= 0) return "";
  return `#${number.toString(16).padStart(6, "0").slice(-6)}`;
}

function connectionLabel(value) {
  if (value === "connected") return "已连接";
  if (value === "connecting") return "连接中";
  if (value === "disconnected") return "已断开";
  return String(value || "未知状态");
}

function showLiveConsoleResult(value) {
  els.liveConsoleOutput.textContent = JSON.stringify(value, null, 2);
  renderFaceAuth(value);
}

function clearLiveConsole() {
  hideFaceAuth();
  els.liveConsoleOutput.textContent = "暂无数据";
}

function renderFaceAuth(value) {
  const faceAuth = value?.face_auth;
  if (!faceAuth?.svg) { hideFaceAuth(); return; }
  els.faceAuthPanel.hidden = false;
  els.faceAuthQr.innerHTML = faceAuth.svg;
  els.faceAuthStatus.textContent = value.message || "需要完成开播身份验证";
  if (faceAuth.url) {
    els.faceAuthLink.href = faceAuth.url;
    els.faceAuthLink.hidden = false;
  } else {
    els.faceAuthLink.removeAttribute("href");
    els.faceAuthLink.hidden = true;
  }
}

function hideFaceAuth() {
  els.faceAuthPanel.hidden = true;
  els.faceAuthQr.textContent = "未生成";
  els.faceAuthStatus.textContent = "等待开播返回";
  els.faceAuthLink.removeAttribute("href");
  els.faceAuthLink.hidden = true;
}

async function copyText(value) {
  if (!value) return;
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(value);
    } else {
      fallbackCopyText(value);
    }
    toast("已复制");
  } catch {
    fallbackCopyText(value);
    toast("已复制");
  }
}

function fallbackCopyText(value) {
  const input = document.createElement("textarea");
  input.value = value;
  input.setAttribute("readonly", "");
  input.style.position = "fixed";
  input.style.opacity = "0";
  document.body.append(input);
  input.select();
  document.execCommand("copy");
  input.remove();
}

async function api(path, options = {}) {
  const init = { method: options.method || "GET", headers: {} };
  if (options.body !== undefined) {
    init.headers["content-type"] = "application/json";
    init.body = JSON.stringify(options.body);
  }
  const response = await fetch(path, init);
  const text = await response.text();
  const data = text ? JSON.parse(text) : null;
  if (!response.ok) throw new Error(data?.error || response.statusText);
  return data;
}

function showManagerResult(value) {
  els.managerOutput.textContent = JSON.stringify(value, null, 2);
}

function setStatus(element, text, ok) {
  const span = element.querySelector(".status-text");
  if (span) span.textContent = text;
  element.classList.toggle("ok", ok);
}

function toast(message, error = false) {
  const item = document.createElement("div");
  item.className = `toast ${error ? "error" : ""}`;
  item.textContent = message;
  els.toasts.append(item);
  window.setTimeout(() => item.remove(), 3500);
}

function tryParseJson(value) {
  try { return JSON.parse(value); } catch { return null; }
}

function selectValue(value) {
  return value == null ? "" : String(value);
}

function splitArgs(value) {
  return String(value || "")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
}

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}
