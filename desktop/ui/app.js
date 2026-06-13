// Фронтенд P2P Messenger. Общается с бэкендом через глобальный Tauri API.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let myPub = "";
let contacts = []; // [{key,name,fingerprint,online,added}]
const messages = {}; // key -> [{id,text,mine,time,status}]
const unread = {}; // key -> count
let currentChat = null;
let infoKey = null;
let searchTerm = "";

const $ = (id) => document.getElementById(id);
const esc = (s) =>
  String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const hhmm = (t) =>
  t ? new Date(t * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "";

function showScreen(id) {
  document.querySelectorAll(".screen").forEach((s) => s.classList.remove("active"));
  $(id).classList.add("active");
}

// --- идентичность ---
async function loadIdentity() {
  const me = await invoke("my_identity");
  myPub = me.pubkey;
  $("my-address").textContent = me.address;
  $("my-pubkey").textContent = me.pubkey;
  $("my-words").innerHTML = me.fingerprint.map((w) => `<span>${esc(w)}</span>`).join("");
}

// --- статус сети ---
async function refreshNet() {
  try {
    const n = await invoke("net_status");
    const el = $("net-status");
    el.textContent = n.online ? `в сети · пиров ${n.peers}` : `подключение… (${n.peers})`;
    el.classList.toggle("online", n.online);
  } catch (_) {}
}

// --- контакты ---
async function loadContacts() {
  contacts = await invoke("list_contacts");
  renderContacts();
}
const contactByKey = (key) => contacts.find((c) => c.key === key);

function renderContacts() {
  const list = $("contact-list");
  const term = searchTerm.toLowerCase();
  const shown = contacts.filter((c) => !term || c.name.toLowerCase().includes(term));
  list.innerHTML = "";
  $("no-contacts").style.display = contacts.length ? "none" : "block";
  for (const c of shown) {
    const u = unread[c.key] || 0;
    const li = document.createElement("li");
    li.className = "contact";
    li.innerHTML =
      `<span class="dot ${c.online ? "on" : "off"}">●</span>` +
      `<span class="name">${esc(c.name)}</span>` +
      (u ? `<span class="badge">${u}</span>` : "") +
      `<span class="fp">${esc(c.fingerprint.split(" ").slice(0, 3).join(" "))}…</span>`;
    li.onclick = () => openChat(c.key);
    list.appendChild(li);
  }
}

// --- чат ---
async function openChat(key) {
  currentChat = key;
  unread[key] = 0;
  renderContacts();
  const c = contactByKey(key);
  $("chat-name").textContent = c ? c.name : key.slice(0, 8);
  updateChatStatus();
  try {
    messages[key] = await invoke("get_history", { keyHex: key });
  } catch (_) {
    messages[key] = messages[key] || [];
  }
  renderMessages();
  showScreen("screen-chat");
  $("chat-input").focus();
}

function updateChatStatus() {
  const c = contactByKey(currentChat);
  const online = c && c.online;
  const dot = $("chat-status");
  dot.textContent = online ? "● онлайн" : "⚫ офлайн";
  dot.className = "status-dot " + (online ? "on" : "off");
}

function renderMessages() {
  const box = $("chat-messages");
  const list = messages[currentChat] || [];
  box.innerHTML = list
    .map((m) => {
      let mark = "";
      if (m.mine) {
        if (m.status === "delivered") mark = `<span class="ok">✓✓</span>`;
        else if (m.status === "failed") mark = `<span class="fail">не доставлено</span>`;
        else mark = "✓";
      }
      return (
        `<div class="msg ${m.mine ? "out" : "in"}">${esc(m.text)}` +
        `<span class="meta">${hhmm(m.time)} ${mark}</span></div>`
      );
    })
    .join("");
  box.scrollTop = box.scrollHeight;
}

function pushMessage(key, msg) {
  (messages[key] = messages[key] || []).push(msg);
  if (key === currentChat) renderMessages();
}

async function sendCurrent() {
  const input = $("chat-input");
  const text = input.value.trim();
  if (!text || !currentChat) return;
  input.value = "";
  const ts = Math.floor(Date.now() / 1000);
  try {
    const id = await invoke("send_text", { keyHex: currentChat, text });
    pushMessage(currentChat, { id, text, mine: true, time: ts, status: "sent" });
  } catch (_) {
    pushMessage(currentChat, { id: 0, text, mine: true, time: ts, status: "failed" });
  }
}

// --- добавление контакта ---
async function doAdd() {
  $("add-error").textContent = "";
  try {
    await invoke("add_contact", { keyHex: $("add-key").value.trim(), name: $("add-name").value.trim() });
    $("add-key").value = "";
    $("add-name").value = "";
    await loadContacts();
    showScreen("screen-main");
  } catch (e) {
    $("add-error").textContent = String(e);
  }
}

// --- инфо о контакте ---
async function openInfo() {
  if (!currentChat) return;
  infoKey = currentChat;
  $("info-error").textContent = "";
  try {
    const c = await invoke("contact_info", { keyHex: infoKey });
    $("info-name").value = c.name;
    $("info-status").textContent = c.online ? "онлайн" : "офлайн";
    $("info-words").innerHTML = c.fingerprint.split(" ").map((w) => `<span>${esc(w)}</span>`).join("");
    $("info-key").textContent = c.key;
    showScreen("screen-info");
  } catch (e) {
    $("info-error").textContent = String(e);
  }
}

async function doRename() {
  try {
    await invoke("rename_contact", { keyHex: infoKey, name: $("info-name").value.trim() });
    await loadContacts();
    if (currentChat === infoKey) $("chat-name").textContent = $("info-name").value.trim();
  } catch (e) {
    $("info-error").textContent = String(e);
  }
}

async function doDelete() {
  try {
    await invoke("delete_contact", { keyHex: infoKey });
    delete messages[infoKey];
    delete unread[infoKey];
    currentChat = null;
    await loadContacts();
    showScreen("screen-main");
  } catch (e) {
    $("info-error").textContent = String(e);
  }
}

// --- события движка ---
function setOnline(key, online) {
  const c = contactByKey(key);
  if (c) c.online = online;
  renderContacts();
  if (key === currentChat) updateChatStatus();
}

function markStatus(key, id, status) {
  const list = messages[key];
  const m = list && list.find((x) => x.id === id);
  if (m) {
    m.status = status;
    if (key === currentChat) renderMessages();
  }
}

async function wireEvents() {
  await listen("presence", (e) => setOnline(e.payload.peer, e.payload.online));
  await listen("contacts-changed", () => loadContacts());
  await listen("message", (e) => {
    const { peer, id, text } = e.payload;
    const ts = Math.floor(Date.now() / 1000);
    pushMessage(peer, { id, text, mine: false, time: ts, status: "" });
    if (peer !== currentChat) {
      unread[peer] = (unread[peer] || 0) + 1;
      renderContacts();
    }
  });
  await listen("ack", (e) => markStatus(e.payload.peer, e.payload.id, "delivered"));
  await listen("undelivered", (e) => markStatus(e.payload.peer, e.payload.id, "failed"));
}

// --- копирование ключа ---
async function copyKey() {
  const text = $("my-pubkey").textContent;
  try {
    await navigator.clipboard.writeText(text);
  } catch (_) {
    const r = document.createRange();
    r.selectNode($("my-pubkey"));
    window.getSelection().removeAllRanges();
    window.getSelection().addRange(r);
    document.execCommand("copy");
  }
  const btn = $("copy-pubkey");
  const old = btn.textContent;
  btn.textContent = "Скопировано ✓";
  setTimeout(() => (btn.textContent = old), 1500);
}

// --- инициализация ---
function bindUi() {
  $("open-mycode").onclick = () => showScreen("screen-mycode");
  $("fab-add").onclick = () => showScreen("screen-add");
  $("do-add").onclick = doAdd;
  $("copy-pubkey").onclick = copyKey;
  $("chat-send").onclick = sendCurrent;
  $("chat-input").addEventListener("keydown", (e) => e.key === "Enter" && sendCurrent());
  $("search").addEventListener("input", (e) => {
    searchTerm = e.target.value;
    renderContacts();
  });
  $("open-info").onclick = openInfo;
  $("open-info-2").onclick = openInfo;
  $("info-rename").onclick = doRename;
  $("info-delete").onclick = doDelete;
  document.querySelectorAll("[data-back]").forEach((b) => (b.onclick = () => showScreen("screen-main")));
  document
    .querySelectorAll("[data-back-chat]")
    .forEach((b) => (b.onclick = () => showScreen("screen-chat")));
}

async function main() {
  bindUi();
  await wireEvents();
  await loadIdentity();
  await loadContacts();
  await refreshNet();
  setInterval(refreshNet, 3000);
  setInterval(loadContacts, 5000);
}

main();
