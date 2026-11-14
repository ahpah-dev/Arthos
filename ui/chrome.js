const post = (obj) => {
  window.ipc.postMessage(JSON.stringify(obj));
};

const tabStrip = document.getElementById("tab-strip");
const omniInput = document.getElementById("omnibox-input");
const omniBadge = document.getElementById("omni-badge");
const btnBookmark = document.getElementById("btn-bookmark");
const starIcon = document.getElementById("star-icon");
const btnReader = document.getElementById("btn-reader");
const btnAi = document.getElementById("btn-ai");
const btnDownloads = document.getElementById("btn-downloads");
const findBar = document.getElementById("find-bar");
const findInput = document.getElementById("find-input");
const toastEl = document.getElementById("toast");

let state = null;
let omniDirty = false;
let toastTimer = null;

function faviconUrl(url) {
  try {
    if (!url.startsWith("http://") && !url.startsWith("https://")) return null;
    const u = new URL(url);
    if (u.hostname === "arthos.localhost" || u.hostname.endsWith(".localhost")) return null;
    return `https://www.google.com/s2/favicons?sz=32&domain=${encodeURIComponent(u.hostname)}`;
  } catch {
    return null;
  }
}

function renderTabs(data) {
  tabStrip.innerHTML = "";
  for (const tab of data.tabs) {
    const el = document.createElement("button");
    el.className = "tab" + (tab.id === data.active_tab_id ? " active" : "") + (tab.incognito ? " incognito" : "");
    el.title = tab.title || tab.display_url;

    const iconWrap = document.createElement("span");
    iconWrap.className = "tab-icon";
    if (tab.loading) {
      const sp = document.createElement("span");
      sp.className = "spinner";
      iconWrap.appendChild(sp);
    } else if (tab.incognito) {
      iconWrap.innerHTML = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#a78bfa" stroke-width="2"><circle cx="8" cy="15" r="3"/><circle cx="16" cy="15" r="3"/><path d="M4 10h16"/></svg>`;
    } else {
      const fav = faviconUrl(tab.url);
      if (fav) {
        const img = document.createElement("img");
        img.src = fav;
        img.onerror = () => {
          iconWrap.innerHTML = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M3 12h18"/></svg>`;
        };
        iconWrap.appendChild(img);
      } else {
        iconWrap.innerHTML = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" stroke-width="2"><path d="M12 3l8 16H4L12 3z"/></svg>`;
      }
    }

    const titleSpan = document.createElement("span");
    titleSpan.className = "tab-title";
    titleSpan.textContent = tab.title || "New Tab";

    const closeBtn = document.createElement("span");
    closeBtn.className = "tab-close";
    closeBtn.textContent = "×";
    closeBtn.title = "Close Tab (Ctrl+W)";
    closeBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      post({ cmd: "close_tab", id: tab.id });
    });

    el.appendChild(iconWrap);
    el.appendChild(titleSpan);
    el.appendChild(closeBtn);
    el.addEventListener("click", () => post({ cmd: "switch_tab", id: tab.id }));
    el.addEventListener("auxclick", (e) => {
      if (e.button === 1) post({ cmd: "close_tab", id: tab.id });
    });

    tabStrip.appendChild(el);
    if (tab.id === data.active_tab_id) {
      el.scrollIntoView({ inline: "nearest", behavior: "smooth" });
    }
  }
}

function updateOmniboxBadge(val) {
  const v = (val || "").trim();
  omniBadge.className = "";
  if (v.startsWith("? ") || v.startsWith("/ai ") || v.startsWith("/ask ")) {
    omniBadge.className = "ai-mode";
    omniBadge.textContent = "✦ AI";
  } else if (v.startsWith("arthos://")) {
    omniBadge.className = "internal-mode";
    omniBadge.innerHTML = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"><path d="M12 3l8 16H4L12 3z"/></svg>`;
  } else if (v.startsWith("https://")) {
    omniBadge.innerHTML = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#34d399" stroke-width="2.2"><rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg>`;
  } else {
    omniBadge.innerHTML = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>`;
  }
}

window.__arthosUpdate = (data) => {
  state = data;
  document.documentElement.setAttribute("data-theme", data.theme || "dark");
  document.documentElement.setAttribute("data-accent", data.accent || "indigo");

  renderTabs(data);

  const active = data.tabs.find((t) => t.id === data.active_tab_id);
  if (active && (!omniDirty || document.activeElement !== omniInput)) {
    const shown = active.display_url === "arthos://newtab" ? "" : active.display_url;
    omniInput.value = shown;
    updateOmniboxBadge(active.display_url);
  }

  if (active) {
    btnBookmark.classList.toggle("active", !!active.is_bookmarked);
    starIcon.setAttribute("fill", active.is_bookmarked ? "currentColor" : "none");
    btnReader.classList.toggle("active", !!active.is_reader);
  }

  btnAi.classList.toggle("active", !!data.sidebar_open);
  findBar.classList.toggle("hidden", !data.find_open);
  btnDownloads.classList.toggle("hidden", !data.downloads || data.downloads.length === 0);
};

window.__arthosFocusOmnibox = () => {
  omniInput.focus();
  omniInput.select();
};

window.__arthosFocusFind = () => {
  findInput.focus();
  findInput.select();
};

window.__arthosToast = (msg) => {
  toastEl.textContent = msg;
  toastEl.classList.remove("hidden");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toastEl.classList.add("hidden"), 2300);
};

// Event listeners
document.getElementById("brand-btn").addEventListener("click", () => post({ cmd: "nav_home" }));
document.getElementById("new-tab-btn").addEventListener("click", () => post({ cmd: "new_tab", url: null, incognito: false }));
document.getElementById("incognito-tab-btn").addEventListener("click", () => post({ cmd: "new_tab", url: null, incognito: true }));

document.getElementById("win-min").addEventListener("click", () => post({ cmd: "window_minimize" }));
document.getElementById("win-max").addEventListener("click", () => post({ cmd: "window_maximize" }));
document.getElementById("win-close").addEventListener("click", () => post({ cmd: "window_close" }));

const dragSpacer = document.getElementById("drag-spacer");
dragSpacer.addEventListener("mousedown", (e) => {
  if (e.button === 0) post({ cmd: "window_drag" });
});
dragSpacer.addEventListener("dblclick", () => post({ cmd: "window_maximize" }));

tabStrip.addEventListener("wheel", (e) => {
  if (e.deltaY !== 0) {
    tabStrip.scrollLeft += e.deltaY;
  }
});

document.getElementById("btn-back").addEventListener("click", () => post({ cmd: "nav_back" }));
document.getElementById("btn-forward").addEventListener("click", () => post({ cmd: "nav_forward" }));
document.getElementById("btn-reload").addEventListener("click", () => post({ cmd: "nav_reload" }));
document.getElementById("btn-home").addEventListener("click", () => post({ cmd: "nav_home" }));

document.getElementById("btn-reader").addEventListener("click", () => post({ cmd: "toggle_reader_mode" }));
document.getElementById("btn-bookmark").addEventListener("click", () => post({ cmd: "toggle_bookmark" }));
document.getElementById("btn-command").addEventListener("click", () => post({ cmd: "toggle_overlay", open: null, mode: "command", query: null }));
document.getElementById("btn-library").addEventListener("click", () => post({ cmd: "new_tab", url: "arthos://library", incognito: false }));
document.getElementById("btn-downloads").addEventListener("click", () => post({ cmd: "new_tab", url: "arthos://downloads", incognito: false }));
document.getElementById("btn-settings").addEventListener("click", () => post({ cmd: "new_tab", url: "arthos://settings", incognito: false }));
document.getElementById("btn-ai").addEventListener("click", () => post({ cmd: "toggle_sidebar", open: null }));

omniInput.addEventListener("input", () => {
  omniDirty = true;
  updateOmniboxBadge(omniInput.value);
});
omniInput.addEventListener("focus", () => {
  omniInput.select();
});
omniInput.addEventListener("blur", () => {
  omniDirty = false;
});
omniInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    omniDirty = false;
    post({ cmd: "navigate", input: omniInput.value });
    omniInput.blur();
  } else if (e.key === "Escape") {
    omniDirty = false;
    if (state) window.__arthosUpdate(state);
    omniInput.blur();
  }
});

// Find bar
findInput.addEventListener("input", () => {
  if (findInput.value) post({ cmd: "find_in_page", query: findInput.value, backward: false });
});
findInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    post({ cmd: "find_in_page", query: findInput.value, backward: e.shiftKey });
  } else if (e.key === "Escape") {
    post({ cmd: "toggle_find_bar", open: false });
  }
});
document.getElementById("find-prev").addEventListener("click", () => post({ cmd: "find_in_page", query: findInput.value, backward: true }));
document.getElementById("find-next").addEventListener("click", () => post({ cmd: "find_in_page", query: findInput.value, backward: false }));
document.getElementById("find-close").addEventListener("click", () => post({ cmd: "toggle_find_bar", open: false }));

// Keyboard shortcuts when chrome has focus
window.addEventListener("keydown", (e) => {
  const ctrl = e.ctrlKey || e.metaKey;
  const k = (e.key || "").toLowerCase();
  if (ctrl && !e.shiftKey && k === "t") { e.preventDefault(); post({ cmd: "new_tab", url: null, incognito: false }); }
  else if (ctrl && e.shiftKey && k === "n") { e.preventDefault(); post({ cmd: "new_tab", url: null, incognito: true }); }
  else if (ctrl && !e.shiftKey && k === "w") { e.preventDefault(); post({ cmd: "close_tab", id: 0 }); }
  else if (ctrl && !e.shiftKey && k === "l") { e.preventDefault(); window.__arthosFocusOmnibox(); }
  else if (ctrl && !e.shiftKey && k === "k") { e.preventDefault(); post({ cmd: "toggle_overlay", open: null, mode: "command", query: null }); }
  else if (ctrl && !e.shiftKey && k === "i") { e.preventDefault(); post({ cmd: "toggle_sidebar", open: null }); }
  else if (ctrl && !e.shiftKey && k === "f") { e.preventDefault(); post({ cmd: "toggle_find_bar", open: true }); }
  else if (e.key === "F12") { e.preventDefault(); post({ cmd: "open_devtools" }); }
});

post({ cmd: "get_state" });
