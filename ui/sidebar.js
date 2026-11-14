const post = (obj) => window.ipc.postMessage(JSON.stringify(obj));

const feed = document.getElementById("chat-feed");
const emptyState = document.getElementById("empty-state");
const input = document.getElementById("chat-input");
const btnSend = document.getElementById("btn-send");
const btnStop = document.getElementById("btn-stop");
const ctxCheckbox = document.getElementById("ctx-checkbox");
const ctxPageTitle = document.getElementById("ctx-page-title");
const providerSelect = document.getElementById("provider-select");
const activeModelPill = document.getElementById("active-model-pill");
const selectionPreview = document.getElementById("selection-preview");
const selectionPreviewText = document.getElementById("selection-preview-text");

let conversation = []; // [{ role, content }]
let streaming = false;
let activeRequestId = null;
let currentAssistantEl = null;
let currentAssistantText = "";
let attachedSelection = "";

function escapeHtml(s) {
  return (s || "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function renderMarkdown(md) {
  const codeBlocks = [];
  let text = (md || "").replace(/```([a-zA-Z0-9_-]*)\n([\s\S]*?)```/g, (_, lang, code) => {
    const idx = codeBlocks.length;
    codeBlocks.push({ lang, code });
    return `\u0000CODEBLOCK_${idx}\u0000`;
  });

  text = escapeHtml(text);

  // Inline code
  text = text.replace(/`([^`\n]+)`/g, "<code>$1</code>");
  // Links [label](url)
  text = text.replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+|arthos:\/\/[^\s)]+)\)/g, (_, label, url) => {
    return `<a href="${url}" data-arthos-link="${url}">${label}</a>`;
  });
  // Bold & italic
  text = text.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  text = text.replace(/\*([^*]+)\*/g, "<em>$1</em>");

  // Line-by-line blocks
  const lines = text.split("\n");
  const out = [];
  let inList = false;

  for (const line of lines) {
    if (line.startsWith("\u0000CODEBLOCK_")) {
      if (inList) { out.push("</ul>"); inList = false; }
      out.push(line);
      continue;
    }
    const hMatch = line.match(/^(#{1,4})\s+(.+)$/);
    if (hMatch) {
      if (inList) { out.push("</ul>"); inList = false; }
      const lvl = Math.min(4, Math.max(2, hMatch[1].length));
      out.push(`<h${lvl}>${hMatch[2]}</h${lvl}>`);
      continue;
    }
    if (line.startsWith("&gt; ")) {
      if (inList) { out.push("</ul>"); inList = false; }
      out.push(`<blockquote>${line.slice(5)}</blockquote>`);
      continue;
    }
    const liMatch = line.match(/^[-*•]\s+(.+)$/);
    if (liMatch) {
      if (!inList) { out.push("<ul>"); inList = true; }
      out.push(`<li>${liMatch[1]}</li>`);
      continue;
    }
    if (inList) { out.push("</ul>"); inList = false; }
    if (line.trim() === "") {
      out.push("");
    } else {
      out.push(`<p>${line}</p>`);
    }
  }
  if (inList) out.push("</ul>");

  let html = out.join("\n");
  html = html.replace(/\u0000CODEBLOCK_(\d+)\u0000/g, (_, idx) => {
    const blk = codeBlocks[Number(idx)];
    const escaped = escapeHtml(blk.code.replace(/\n$/, ""));
    return `<pre><button class="code-copy" type="button">Copy</button><code>${escaped}</code></pre>`;
  });
  return html;
}

function appendMessage(role, content, isError = false) {
  emptyState.classList.add("hidden");
  const wrap = document.createElement("div");
  wrap.className = `msg ${role}` + (isError ? " error" : "");

  const roleEl = document.createElement("div");
  roleEl.className = "msg-role";
  roleEl.textContent = role === "user" ? "You" : "Arthos AI";

  const bodyEl = document.createElement("div");
  bodyEl.className = "msg-body";
  if (role === "user") {
    bodyEl.textContent = content;
  } else {
    bodyEl.innerHTML = renderMarkdown(content);
  }

  wrap.appendChild(roleEl);
  wrap.appendChild(bodyEl);
  feed.appendChild(wrap);
  feed.scrollTop = feed.scrollHeight;
  return bodyEl;
}

function setStreaming(active) {
  streaming = active;
  btnSend.classList.toggle("hidden", active);
  btnStop.classList.toggle("hidden", !active);
}

function sendPrompt(rawText) {
  const text = (rawText ?? input.value).trim();
  if (!text || streaming) return;

  let finalPrompt = text;
  if (attachedSelection) {
    finalPrompt = `Selected text:\n> ${attachedSelection}\n\n${text}`;
    attachedSelection = "";
    selectionPreview.classList.add("hidden");
  }

  if (!rawText) {
    input.value = "";
    input.style.height = "auto";
  }

  conversation.push({ role: "user", content: finalPrompt });
  appendMessage("user", text);

  activeRequestId = "req-" + Date.now();
  currentAssistantText = "";
  currentAssistantEl = appendMessage("assistant", "Thinking...");
  setStreaming(true);

  post({
    cmd: "ai_ask",
    request_id: activeRequestId,
    messages: conversation,
    include_page: ctxCheckbox.checked,
  });
}

window.__arthosSidebarState = (data) => {
  document.documentElement.setAttribute("data-theme", data.theme || "dark");
  document.documentElement.setAttribute("data-accent", data.accent || "indigo");
  providerSelect.value = data.ai_provider || "local";
  activeModelPill.textContent = data.active_model || "Local Instant";
  const active = data.tabs.find((t) => t.id === data.active_tab_id);
  if (active) {
    ctxPageTitle.textContent = active.title || active.display_url || "Current page";
  }
};

window.__arthosSidebarEvent = (ev) => {
  if (ev.type === "prompt") {
    sendPrompt(ev.text);
  } else if (ev.type === "selection") {
    attachedSelection = ev.text || "";
    if (attachedSelection) {
      selectionPreviewText.textContent = `Selection: "${attachedSelection.slice(0, 80)}${attachedSelection.length > 80 ? "..." : ""}"`;
      selectionPreview.classList.remove("hidden");
    }
    input.focus();
  } else if (ev.type === "chunk" && ev.request_id === activeRequestId) {
    currentAssistantText += ev.text;
    if (currentAssistantEl) {
      currentAssistantEl.innerHTML = renderMarkdown(currentAssistantText);
      feed.scrollTop = feed.scrollHeight;
    }
  } else if (ev.type === "done" && ev.request_id === activeRequestId) {
    setStreaming(false);
    if (currentAssistantText) {
      conversation.push({ role: "assistant", content: currentAssistantText });
    }
    if (ev.telemetry && currentAssistantEl && currentAssistantEl.parentElement) {
      const t = ev.telemetry;
      const meta = document.createElement("div");
      meta.className = "pulse-telemetry";
      const parts = [
        `⚡ ${t.engine_tag || "Pulse"}`,
        `${t.ttft_ms ?? 0}ms TTFT`,
        `${Number(t.tok_per_sec || 0).toFixed(0)} tok/s`,
      ];
      if (t.compression_pct > 0) parts.push(`-${t.compression_pct}% ctx`);
      if (t.cache_hit) parts.push("cached");
      meta.textContent = parts.join(" · ");
      currentAssistantEl.parentElement.appendChild(meta);
      feed.scrollTop = feed.scrollHeight;
    }
    activeRequestId = null;
  } else if (ev.type === "error" && ev.request_id === activeRequestId) {
    setStreaming(false);
    if (currentAssistantEl) {
      currentAssistantEl.parentElement.classList.add("error");
      currentAssistantEl.innerHTML = renderMarkdown(`**Error:** ${ev.message}`);
    }
    activeRequestId = null;
  }
};

window.__arthosFocusChat = () => input.focus();

// Event handlers
btnSend.addEventListener("click", () => sendPrompt());
btnStop.addEventListener("click", () => {
  post({ cmd: "ai_stop" });
  setStreaming(false);
});

input.addEventListener("input", () => {
  input.style.height = "auto";
  input.style.height = Math.min(130, input.scrollHeight) + "px";
});
input.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendPrompt();
  }
});

document.querySelectorAll(".chip").forEach((c) => {
  c.addEventListener("click", () => sendPrompt(c.dataset.prompt));
});

providerSelect.addEventListener("change", () => {
  post({ cmd: "ai_set_provider", provider: providerSelect.value, model: null });
});

document.getElementById("btn-sb-settings").addEventListener("click", () => {
  post({ cmd: "open_external_tab", url: "arthos://settings" });
});
document.getElementById("btn-sb-clear").addEventListener("click", () => {
  if (streaming) post({ cmd: "ai_stop" });
  setStreaming(false);
  conversation = [];
  feed.querySelectorAll(".msg").forEach((m) => m.remove());
  emptyState.classList.remove("hidden");
});
document.getElementById("btn-sb-close").addEventListener("click", () => {
  post({ cmd: "toggle_sidebar", open: false });
});
document.getElementById("btn-grab-sel").addEventListener("click", () => {
  post({ cmd: "grab_selection" });
});
document.getElementById("selection-clear").addEventListener("click", () => {
  attachedSelection = "";
  selectionPreview.classList.add("hidden");
});

// Delegated clicks for links and code copy buttons
feed.addEventListener("click", (e) => {
  const link = e.target.closest("a[data-arthos-link]");
  if (link) {
    e.preventDefault();
    post({ cmd: "open_external_tab", url: link.dataset.arthosLink });
    return;
  }
  const copyBtn = e.target.closest(".code-copy");
  if (copyBtn) {
    const code = copyBtn.nextElementSibling?.textContent || "";
    navigator.clipboard?.writeText(code);
    copyBtn.textContent = "Copied!";
    setTimeout(() => (copyBtn.textContent = "Copy"), 1300);
  }
});

// Resize handle drag
const resizeHandle = document.getElementById("resize-handle");
let resizing = false;
let startScreenX = 0;
let startWidth = 390;

resizeHandle.addEventListener("mousedown", (e) => {
  resizing = true;
  startScreenX = e.screenX;
  startWidth = window.innerWidth;
  e.preventDefault();
});
window.addEventListener("mousemove", (e) => {
  if (!resizing) return;
  const delta = startScreenX - e.screenX;
  const nextWidth = Math.max(300, Math.min(620, Math.round(startWidth + delta)));
  post({ cmd: "set_sidebar_width", width: nextWidth });
});
window.addEventListener("mouseup", () => {
  resizing = false;
});

window.addEventListener("keydown", (e) => {
  const ctrl = e.ctrlKey || e.metaKey;
  const k = (e.key || "").toLowerCase();
  if (ctrl && k === "i") { e.preventDefault(); post({ cmd: "toggle_sidebar", open: false }); }
  else if (ctrl && k === "k") { e.preventDefault(); post({ cmd: "toggle_overlay", open: null, mode: "command", query: null }); }
  else if (ctrl && k === "l") { e.preventDefault(); post({ cmd: "focus_omnibox" }); }
});

post({ cmd: "get_state" });
