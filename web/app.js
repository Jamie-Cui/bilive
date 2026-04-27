const state = {
  config: null,
  authenticated: false,
  danmuConnected: false,
  qrPollTimer: null,
};

const $ = (selector) => document.querySelector(selector);
const els = {
  serviceStatus: $("#service-status"),
  authStatus: $("#auth-status"),
  danmuStatus: $("#danmu-status"),
  eventStatus: $("#event-status"),
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
  logList: $("#log-list"),
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
  theme: $("#theme"),
  saveConfig: $("#save-config"),
  toasts: $("#toasts"),
};

bindUi();
connectEvents();
void refreshAll();

function bindUi() {
  document.querySelectorAll("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => switchTab(button.dataset.tab));
  });

  els.refresh.addEventListener("click", () => void refreshAll());
  els.logout.addEventListener("click", () => void logout());
  els.qrGenerate.addEventListener("click", () => void generateQr());
  els.qrStop.addEventListener("click", stopQrPolling);
  els.cookieLogin.addEventListener("click", () => void cookieLogin());
  els.bootstrap.addEventListener("click", () => void bootstrap());
  els.saveTitle.addEventListener("click", () => void saveTitle());
  els.saveArea.addEventListener("click", () => void saveArea());
  els.startLive.addEventListener("click", () => void startLive());
  els.stopLive.addEventListener("click", () => void stopLive());
  els.categoryId.addEventListener("change", () => {
    renderAreaOptions();
    void patchConfig({ category_id: els.categoryId.value, area_id: els.areaId.value });
  });
  els.areaId.addEventListener("change", () => void patchConfig({ area_id: els.areaId.value }));
  els.connectDanmu.addEventListener("click", () => void connectDanmu());
  els.disconnectDanmu.addEventListener("click", () => void disconnectDanmu());
  els.refreshDanmuToken.addEventListener("click", () => void refreshDanmuToken());
  els.sendComment.addEventListener("click", () => void sendComment());
  els.commentMessage.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void sendComment();
    }
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
  els.saveConfig.addEventListener("click", () => void patchConfig({ theme: els.theme.value }));
}

function switchTab(tab) {
  document.querySelectorAll("[data-tab]").forEach((button) => {
    button.classList.toggle("active", button.dataset.tab === tab);
  });
  document.querySelectorAll(".tab-panel").forEach((panel) => {
    panel.classList.toggle("active", panel.id === `tab-${tab}`);
  });
  els.pageTitle.textContent = ({ account: "账号", stream: "直播", comments: "弹幕", manager: "管理", settings: "设置" })[tab] || "bilive";
}

async function refreshAll() {
  try {
    const [health, auth, danmu] = await Promise.all([
      api("/api/health"),
      api("/api/auth/status"),
      api("/api/danmu/status"),
    ]);
    setStatus(els.serviceStatus, `${health.status} v${health.version}`, health.status === "ok");
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
  if (!cookie) {
    toast("请先粘贴 Cookie", true);
    return;
  }
  const result = await api("/api/auth/cookie", {
    method: "POST",
    body: { cookie },
  });
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
  els.qrBox.textContent = "生成中";
  const qr = await api("/api/auth/qrcode/generate", { method: "POST" });
  els.qrBox.innerHTML = qr.svg;
  els.qrStatus.textContent = "请使用哔哩哔哩 App 扫码";
  els.qrStop.disabled = false;
  state.qrPollTimer = window.setInterval(() => void pollQr(qr.qrcode_key), 1200);
}

async function pollQr(qrcodeKey) {
  const data = await api("/api/auth/qrcode/poll", {
    method: "POST",
    body: { qrcode_key: qrcodeKey },
  });
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
  if (state.qrPollTimer) {
    window.clearInterval(state.qrPollTimer);
    state.qrPollTimer = null;
  }
  els.qrStop.disabled = true;
}

async function saveTitle() {
  await api("/api/live/title", {
    method: "POST",
    body: { title: els.roomTitle.value },
  });
  await refreshAll();
  toast("标题已更新");
}

async function saveArea() {
  await api("/api/live/area", {
    method: "POST",
    body: { area_id: els.areaId.value },
  });
  await refreshAll();
  toast("分区已更新");
}

async function startLive() {
  await patchConfig({
    room_title: els.roomTitle.value,
    category_id: els.categoryId.value,
    area_id: els.areaId.value,
  });
  const result = await api("/api/live/start", { method: "POST" });
  if (result.code === 0) {
    await refreshAll();
    toast("开播成功");
    return;
  }
  showManagerResult(result);
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
  await api("/api/live/comment", {
    method: "POST",
    body: { message },
  });
  els.commentMessage.value = "";
  toast("弹幕已发送");
}

async function loadRank() {
  const result = await api("/api/live/contribution-rank");
  pushLog("在线榜单", JSON.stringify(result, null, 2), "info");
}

async function managerCall(method, path, body) {
  const result = await api(path, { method, body });
  showManagerResult(result);
  toast("操作完成");
}

async function patchConfig(patch) {
  const config = await api("/api/config", {
    method: "PATCH",
    body: patch,
  });
  state.config = config;
  renderConfig();
  return config;
}

function renderConfig() {
  const config = state.config || {};
  setStatus(els.authStatus, state.authenticated ? "已登录" : "未登录", state.authenticated);
  setStatus(els.danmuStatus, state.danmuConnected ? "已连接" : "未连接", state.danmuConnected);
  els.logout.disabled = !state.authenticated;

  els.profile.textContent = state.authenticated
    ? `${config.username || "未知用户"} · UID ${config.uid || 0} · 房间 ${config.room_id || 0}`
    : "未登录";
  els.roomTitle.value = config.room_title || "";
  els.roomId.value = config.room_id || "";
  els.uid.value = config.uid || "";
  els.theme.value = config.theme || "light";
  renderCategoryOptions();
  renderStreamList();
}

function renderCategoryOptions() {
  const areas = Array.isArray(state.config?.area_list) ? state.config.area_list : [];
  const selected = state.config?.category_id || areas[0]?.id || "";
  els.categoryId.innerHTML = areas
    .map((area) => `<option value="${escapeHtml(area.id)}">${escapeHtml(area.name)}</option>`)
    .join("");
  els.categoryId.value = selected;
  renderAreaOptions();
}

function renderAreaOptions() {
  const areas = Array.isArray(state.config?.area_list) ? state.config.area_list : [];
  const parent = areas.find((area) => area.id === els.categoryId.value) || areas[0];
  const children = Array.isArray(parent?.list) ? parent.list : [];
  const selected = state.config?.area_id || children[0]?.id || "";
  els.areaId.innerHTML = children
    .map((area) => `<option value="${escapeHtml(area.id)}">${escapeHtml(area.name)}</option>`)
    .join("");
  els.areaId.value = selected;
}

function renderStreamList() {
  const streams = Array.isArray(state.config?.streams) ? state.config.streams : [];
  if (streams.length === 0) {
    els.streamList.textContent = "暂无推流凭证";
    return;
  }
  els.streamList.innerHTML = streams
    .map((stream) => `
      <article class="stream-item">
        <strong>${escapeHtml(stream.type)}</strong>
        <div>服务器地址 <code>${escapeHtml(stream.address)}</code></div>
        <div>流密钥 <code>${escapeHtml(stream.key)}</code></div>
      </article>
    `)
    .join("");
}

function connectEvents() {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const socket = new WebSocket(`${protocol}//${window.location.host}/api/events`);
  setStatus(els.eventStatus, "连接中", false);

  socket.addEventListener("open", () => setStatus(els.eventStatus, "已连接", true));
  socket.addEventListener("close", () => {
    setStatus(els.eventStatus, "未连接", false);
    window.setTimeout(connectEvents, 1500);
  });
  socket.addEventListener("error", () => setStatus(els.eventStatus, "异常", false));
  socket.addEventListener("message", (message) => {
    const event = JSON.parse(message.data);
    if (event.type === "connection") {
      state.danmuConnected = event.payload === "connected";
      setStatus(els.danmuStatus, state.danmuConnected ? "已连接" : "未连接", state.danmuConnected);
    }
    appendEvent(event);
  });
}

function appendEvent(event) {
  if (event.type === "connection") {
    pushLog("弹幕连接", event.payload, event.payload === "connected" ? "ok" : "info");
    return;
  }
  if (event.type === "error") {
    pushLog("服务错误", event.payload.message, "error");
    return;
  }
  const payload = event.payload.payload;
  const parsed = tryParseJson(payload);
  const title = parsed?.cmd ? `弹幕事件 ${parsed.cmd}` : "弹幕事件";
  pushLog(title, payload, "info");
}

function pushLog(title, body, tone) {
  const empty = els.logList.querySelector(".empty-state");
  if (empty) empty.remove();

  const item = document.createElement("article");
  item.className = `log-item ${tone}`;
  const meta = document.createElement("div");
  const heading = document.createElement("strong");
  const time = document.createElement("time");
  const pre = document.createElement("pre");
  heading.textContent = title;
  time.textContent = new Date().toLocaleTimeString();
  pre.textContent = body;
  meta.append(heading, time);
  item.append(meta, pre);
  els.logList.prepend(item);

  while (els.logList.children.length > 160) {
    els.logList.lastElementChild.remove();
  }
}

function clearLogs() {
  els.logList.textContent = "";
  const empty = document.createElement("div");
  empty.className = "empty-state";
  empty.textContent = "暂无事件";
  els.logList.append(empty);
}

async function api(path, options = {}) {
  const init = {
    method: options.method || "GET",
    headers: {},
  };
  if (options.body !== undefined) {
    init.headers["content-type"] = "application/json";
    init.body = JSON.stringify(options.body);
  }

  const response = await fetch(path, init);
  const text = await response.text();
  const data = text ? JSON.parse(text) : null;
  if (!response.ok) {
    throw new Error(data?.error || response.statusText);
  }
  return data;
}

function showManagerResult(value) {
  els.managerOutput.textContent = JSON.stringify(value, null, 2);
}

function setStatus(element, text, ok) {
  element.textContent = text;
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
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
}

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}
