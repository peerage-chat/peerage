// Фронтенд P2P Messenger. Общается с бэкендом через глобальный Tauri API
// (window.__TAURI__, включён withGlobalTauri).

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// --- состояние ---
let myPub = "";
let contacts = []; // [{key,name,fingerprint,online}]
const messages = {}; // key -> [{id, text, mine, status}]
let currentChat = null;

const $ = (id) => document.getElementById(id);
const esc = (s) =>
  s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

function showScreen(id) {
  document.querySelectorAll(".screen").forEach((s) => s.classList.remove("active"));
  $(id).classList.add("active");
}

// --- идентичность / мой код ---
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
    if (n.online) {
      el.textContent = `в сети · пиров ${n.peers}`;
      el.classList.add("online");
    } else {
      el.textContent = `подключение… (${n.peers})`;
      el.classList.remove("online");
    }
  } catch (_) {}
}

// --- контакты ---
async function loadContacts() {
  contacts = await invoke("list_contacts");
  renderContacts();
}

function contactByKey(key) {
  return contacts.find((c) => c.key === key);
}

function renderContacts() {
  const list = $("contact-list");
  list.innerHTML = "";
  $("no-contacts").style.display = contacts.length ? "none" : "block";
  for (const c of contacts) {
    const li = document.createElement("li");
    li.className = "contact";
    li.innerHTML =
      `<span class="dot ${c.online ? "on" : "off"}">●</span>` +
      `<span class="name">${esc(c.name)}</span>` +
      `<span class="fp">${esc(c.fingerprint.split(" ").slice(0, 3).join(" "))}…</span>`;
    li.onclick = () => openChat(c.key);
    list.appendChild(li);
  }
}

// --- чат ---
function openChat(key) {
  currentChat = key;
  const c = contactByKey(key);
  $("chat-name").textContent = c ? c.name : key.slice(0, 8);
  updateChatStatus();
  renderMessages();
  showScreen("screen-chat");
  $("chat-input").focus();
}

function updateChatStatus() {
  const c = contactByKey(currentChat);
  const dot = $("chat-status");
  const online = c && c.online;
  dot.textContent = online ? "● онлайн" : "⚫ офлайн";
  dot.className = "status-dot " + (online ? "on" : "off");
}

function renderMessages() {
  const box = $("chat-messages");
  const list = messages[currentChat] || [];
  box.innerHTML = list
    .map((m) => {
      let meta = "";
      if (m.mine) {
        if (m.status === "delivered") meta = `<span class="ok">✓✓</span>`;
        else if (m.status === "failed") meta = `<span class="fail">не доставлено</span>`;
        else meta = "✓";
      }
      return (
        `<div class="msg ${m.mine ? "out" : "in"}">${esc(m.text)}` +
        (meta ? `<span class="meta">${meta}</span>` : "") +
        `</div>`
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
  try {
    const id = await invoke("send_text", { keyHex: currentChat, text });
    pushMessage(currentChat, { id, text, mine: true, status: "sent" });
  } catch (e) {
    pushMessage(currentChat, { id: 0, text, mine: true, status: "failed" });
  }
}

// --- добавление контакта ---
async function doAdd() {
  const key = $("add-key").value.trim();
  const name = $("add-name").value.trim();
  $("add-error").textContent = "";
  try {
    await invoke("add_contact", { keyHex: key, name });
    $("add-key").value = "";
    $("add-name").value = "";
    await loadContacts();
    showScreen("screen-main");
  } catch (e) {
    $("add-error").textContent = String(e);
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
  if (!list) return;
  const m = list.find((x) => x.id === id);
  if (m) {
    m.status = status;
    if (key === currentChat) renderMessages();
  }
}

async function wireEvents() {
  await listen("presence", (e) => setOnline(e.payload.peer, e.payload.online));
  await listen("message", (e) => {
    pushMessage(e.payload.peer, {
      id: e.payload.id,
      text: e.payload.text,
      mine: false,
      status: "",
    });
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
  $("chat-input").addEventListener("keydown", (e) => {
    if (e.key === "Enter") sendCurrent();
  });
  document.querySelectorAll("[data-back]").forEach((b) => {
    b.onclick = () => showScreen("screen-main");
  });
}

async function main() {
  bindUi();
  await wireEvents();
  await loadIdentity();
  await loadContacts();
  await refreshNet();
  setInterval(refreshNet, 3000);
  // периодически обновляем онлайн-статусы контактов (на случай пропущенных событий)
  setInterval(loadContacts, 5000);
}

main();
