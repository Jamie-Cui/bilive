// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

const state = {
  config: null,
  authenticated: false,
  danmuConnected: false,
  chatCount: 0,
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
  commentMessage: $("#comment-message"),
  sendComment: $("#send-comment"),
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
  themeLight: $("#theme-light"),
  themeDark: $("#theme-dark"),
  toasts: $("#toasts"),
};

// Overlay element for mobile sidebar
const overlay = document.createElement("div");
overlay.className = "sidebar-overlay";
document.body.append(overlay);

bindUi();
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
  els.sendComment.addEventListener("click", () => void sendComment());
  els.commentMessage.addEventListener("keydown", (event) => {
    if (event.key === "Enter") { event.preventDefault(); void sendComment(); }
  });

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
  els.pageTitle.textContent = ({ account: "账号", stream: "直播", comments: "弹幕", manager: "管理" })[tab] || "bilive";
}

async function refreshAll() {
  try {
    const [health, auth, danmu] = await Promise.all([
      api("/api/health"),
      api("/api/auth/status"),
      api("/api/danmu/status"),
    ]);
    setStatus(els.serviceStatus, `v${health.version}`, health.status === "ok");
    state.authenticated = auth.authenticated;
    state.config = auth.config;
    state.danmuConnected = danmu.connected;
    els.configPath.textContent = auth.config_path || "默认配置路径";
    renderConfig();
  } catch (error) {
    toast(error.message, true);
  }
}

async function bootstrap() {
  const result = await api("/api/auth/bootstrap", { method: "POST" });
  state.config = result.config;
  state.authenticated = true;
  renderConfig();
  toast("初始化完成");
}

async function cookieLogin() {
  const cookie = els.cookieInput.value.trim();
  if (!cookie) { toast("请先粘贴 Cookie", true); return; }
  const result = await api("/api/auth/cookie", { method: "POST", body: { cookie } });
  state.authenticated = true;
  state.config = result.config;
  els.cookieInput.value = "";
  renderConfig();
  toast("登录成功");
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
      await bootstrap();
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
  applyTheme(config.theme || "dark");
  renderCategoryOptions();
  renderStreamList();
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
  if (parsed?.cmd === "DANMU_MSG") {
    pushDanmuMessage(parseDanmuMessage(parsed), payload, receivedAt);
    return;
  }
  if (parsed?.cmd === "SUPER_CHAT_MESSAGE") {
    pushDanmuMessage(parseSuperChatMessage(parsed), payload, receivedAt);
    return;
  }
  pushSystemEvent(parsed, payload, receivedAt);
}

function pushDanmuMessage(message, raw, receivedAt) {
  const empty = els.chatList.querySelector(".empty-state");
  if (empty) empty.remove();

  const item = document.createElement("article");
  item.className = `danmu-message ${message.tone || ""}`;
  const avatar = document.createElement("div");
  const main = document.createElement("div");
  const meta = document.createElement("div");
  const name = document.createElement("strong");
  const time = document.createElement("time");
  const text = document.createElement("p");

  avatar.className = "danmu-avatar";
  avatar.textContent = firstGlyph(message.name);
  main.className = "danmu-message-main";
  meta.className = "danmu-message-meta";
  name.className = "danmu-name";
  name.textContent = message.name;
  time.textContent = receivedAt.toLocaleTimeString();
  text.className = "danmu-text";
  text.textContent = message.content || "(空消息)";

  meta.append(name);
  if (message.medal) meta.append(pill(message.medal, "danmu-medal"));
  if (message.price) meta.append(pill(message.price, "danmu-price"));
  if (message.color) meta.append(colorSwatch(message.color));
  meta.append(time);
  main.append(meta, text, rawDetails(raw));
  item.append(avatar, main);
  els.chatList.prepend(item);

  state.chatCount += 1;
  updateDanmuCounters();
  while (els.chatList.children.length > 160) els.chatList.lastElementChild.remove();
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
  state.eventCount = 0;
  updateDanmuCounters();
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

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}
