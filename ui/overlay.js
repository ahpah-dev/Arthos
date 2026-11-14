const post = (obj) => window.ipc.postMessage(JSON.stringify(obj));

const backdrop = document.getElementById("backdrop");
const palette = document.getElementById("palette");
const cmdInput = document.getElementById("cmd-input");
const cmdList = document.getElementById("cmd-list");

let contextData = { tabs: [], bookmarks: [], history: [] };
let filtered = [];
let selectedIdx = 0;

function closeOverlay() {
  post({ cmd: "toggle_overlay", open: false, mode: null, query: null });
}

function buildItems(query) {
  const q = (query || "").trim();
  const ql = q.toLowerCase();
  const items = [];

  if (q.length > 0) {
    const cleanQ = q.replace(/^(\?\s*|\/ai\s*|\/ask\s*)/i, "");
    items.push({
      badge: "AI",
      isAi: true,
      title: `Ask Arthos AI: "${cleanQ}"`,
      sub: "Open Copilot",
      run: () => {
        closeOverlay();
        post({ cmd: "navigate", input: `? ${cleanQ}` });
      },
    });
  }

  const actions = [
    { badge: "AI", isAi: true, title: "Summarize Current Page", sub: "Ctrl+I", run: () => { closeOverlay(); post({ cmd: "navigate", input: "? Summarize this page concisely with key takeaways." }); } },
    { badge: "AI", isAi: true, title: "Toggle Arthos AI Copilot", sub: "Ctrl+I", run: () => { closeOverlay(); post({ cmd: "toggle_sidebar", open: null }); } },
    { badge: "View", title: "Toggle Reader Mode", sub: "Alt+R", run: () => { closeOverlay(); post({ cmd: "toggle_reader_mode" }); } },
    { badge: "Tab", title: "New Tab", sub: "Ctrl+T", run: () => { closeOverlay(); post({ cmd: "new_tab", url: null, incognito: false }); } },
    { badge: "Tab", title: "New Private Tab", sub: "Ctrl+Shift+N", run: () => { closeOverlay(); post({ cmd: "new_tab", url: null, incognito: true }); } },
    { badge: "Find", title: "Find in Page", sub: "Ctrl+F", run: () => { closeOverlay(); post({ cmd: "toggle_find_bar", open: true }); } },
    { badge: "Page", title: "Bookmark This Page", sub: "Star", run: () => { closeOverlay(); post({ cmd: "toggle_bookmark" }); } },
    { badge: "App", title: "Bookmarks, History & Downloads", sub: "arthos://library", run: () => { closeOverlay(); post({ cmd: "new_tab", url: "arthos://library", incognito: false }); } },
    { badge: "App", title: "Open Settings", sub: "arthos://settings", run: () => { closeOverlay(); post({ cmd: "new_tab", url: "arthos://settings", incognito: false }); } },
    { badge: "Dev", title: "Open Developer Tools", sub: "F12", run: () => { closeOverlay(); post({ cmd: "open_devtools" }); } },
  ];

  for (const a of actions) {
    if (!ql || a.title.toLowerCase().includes(ql) || a.badge.toLowerCase().includes(ql)) {
      items.push(a);
    }
  }

  for (const t of contextData.tabs || []) {
    const title = t.title || t.url || "Tab";
    if (!ql || title.toLowerCase().includes(ql) || (t.url || "").toLowerCase().includes(ql)) {
      items.push({
        badge: "Tab",
        title: `Switch to: ${title}`,
        sub: t.url || "",
        run: () => { closeOverlay(); post({ cmd: "switch_tab", id: t.id }); },
      });
    }
  }

  for (const b of contextData.bookmarks || []) {
    if (!ql || b.title.toLowerCase().includes(ql) || b.url.toLowerCase().includes(ql)) {
      items.push({
        badge: "Star",
        title: b.title,
        sub: b.url,
        run: () => { closeOverlay(); post({ cmd: "navigate", input: b.url }); },
      });
    }
  }

  for (const h of (contextData.history || []).slice(0, 12)) {
    if (ql && (h.title.toLowerCase().includes(ql) || h.url.toLowerCase().includes(ql))) {
      items.push({
        badge: "Hist",
        title: h.title,
        sub: h.url,
        run: () => { closeOverlay(); post({ cmd: "navigate", input: h.url }); },
      });
    }
  }

  if (q.length > 0 && !q.startsWith("?")) {
    items.push({
      badge: "Go",
      title: `Open or Search "${q}"`,
      sub: "Navigate",
      run: () => { closeOverlay(); post({ cmd: "navigate", input: q }); },
    });
  }

  return items.slice(0, 20);
}

function renderList() {
  cmdList.innerHTML = "";
  if (selectedIdx >= filtered.length) selectedIdx = Math.max(0, filtered.length - 1);

  filtered.forEach((item, idx) => {
    const row = document.createElement("div");
    row.className = "cmd-item" + (idx === selectedIdx ? " selected" : "");
    row.innerHTML = `
      <div class="cmd-left">
        <span class="cmd-badge ${item.isAi ? "ai" : ""}"></span>
        <span class="cmd-title"></span>
      </div>
      <span class="cmd-sub"></span>
    `;
    row.querySelector(".cmd-badge").textContent = item.badge;
    row.querySelector(".cmd-title").textContent = item.title;
    row.querySelector(".cmd-sub").textContent = item.sub || "";
    row.addEventListener("click", () => item.run());
    cmdList.appendChild(row);
    if (idx === selectedIdx) {
      row.scrollIntoView({ block: "nearest" });
    }
  });
}

window.__arthosOverlayEvent = (ev) => {
  if (ev.type === "open") {
    document.documentElement.setAttribute("data-theme", ev.theme || "dark");
    document.documentElement.setAttribute("data-accent", ev.accent || "indigo");
    contextData = ev;
    cmdInput.value = ev.query || "";
    selectedIdx = 0;
    filtered = buildItems(cmdInput.value);
    renderList();
    setTimeout(() => cmdInput.focus(), 10);
  }
};

cmdInput.addEventListener("input", () => {
  selectedIdx = 0;
  filtered = buildItems(cmdInput.value);
  renderList();
});

cmdInput.addEventListener("keydown", (e) => {
  if (e.key === "ArrowDown") {
    e.preventDefault();
    if (filtered.length > 0) {
      selectedIdx = (selectedIdx + 1) % filtered.length;
      renderList();
    }
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    if (filtered.length > 0) {
      selectedIdx = (selectedIdx - 1 + filtered.length) % filtered.length;
      renderList();
    }
  } else if (e.key === "Enter") {
    e.preventDefault();
    if (filtered[selectedIdx]) {
      filtered[selectedIdx].run();
    }
  } else if (e.key === "Escape") {
    e.preventDefault();
    closeOverlay();
  }
});

backdrop.addEventListener("mousedown", (e) => {
  if (!palette.contains(e.target)) {
    closeOverlay();
  }
});
