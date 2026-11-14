(() => {
  if (window.top !== window.self) return;
  if (window.__arthosBridgeInstalled) return;
  window.__arthosBridgeInstalled = true;
  window.__arthosSelectionPillEnabled = true;

  const post = (obj) => {
    try {
      if (window.ipc && typeof window.ipc.postMessage === "function") {
        window.ipc.postMessage(JSON.stringify(obj));
      }
    } catch (_) {}
  };

  // Global browser shortcuts from content viewport
  window.addEventListener(
    "keydown",
    (e) => {
      const ctrl = e.ctrlKey || e.metaKey;
      const key = (e.key || "").toLowerCase();

      if (ctrl && !e.shiftKey && key === "t") {
        e.preventDefault();
        post({ cmd: "new_tab", url: null, incognito: false });
      } else if (ctrl && e.shiftKey && key === "n") {
        e.preventDefault();
        post({ cmd: "new_tab", url: null, incognito: true });
      } else if (ctrl && !e.shiftKey && key === "w") {
        e.preventDefault();
        post({ cmd: "close_tab", id: 0 });
      } else if (ctrl && !e.shiftKey && key === "l") {
        e.preventDefault();
        post({ cmd: "focus_omnibox" });
      } else if (e.altKey && key === "d") {
        e.preventDefault();
        post({ cmd: "focus_omnibox" });
      } else if (ctrl && !e.shiftKey && key === "k") {
        e.preventDefault();
        post({ cmd: "toggle_overlay", open: null, mode: "command", query: null });
      } else if (ctrl && !e.shiftKey && key === "i") {
        e.preventDefault();
        post({ cmd: "toggle_sidebar", open: null });
      } else if (ctrl && !e.shiftKey && key === "f") {
        e.preventDefault();
        post({ cmd: "toggle_find_bar", open: true });
      } else if (ctrl && (key === "r" || e.key === "F5") || (!ctrl && e.key === "F5")) {
        e.preventDefault();
        post({ cmd: "nav_reload" });
      } else if (e.key === "F12" || (ctrl && e.shiftKey && key === "i")) {
        e.preventDefault();
        post({ cmd: "open_devtools" });
      } else if (ctrl && key === "tab") {
        e.preventDefault();
        post({ cmd: e.shiftKey ? "prev_tab" : "next_tab" });
      } else if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        post({ cmd: "nav_back" });
      } else if (e.altKey && e.key === "ArrowRight") {
        e.preventDefault();
        post({ cmd: "nav_forward" });
      } else if (e.altKey && key === "r") {
        e.preventDefault();
        post({ cmd: "toggle_reader_mode" });
      } else if (ctrl && (key === "=" || key === "+")) {
        e.preventDefault();
        post({ cmd: "zoom_in" });
      } else if (ctrl && key === "-") {
        e.preventDefault();
        post({ cmd: "zoom_out" });
      } else if (ctrl && key === "0") {
        e.preventDefault();
        post({ cmd: "zoom_reset" });
      } else if (e.key === "Escape") {
        hidePill();
      }
    },
    true
  );

  // Clean DOM context extraction for Arthos AI
  window.__arthosExtractContext = () => {
    const selection = (window.getSelection ? window.getSelection().toString() : "").trim();
    const title = document.title || "";
    const url = location.href || "";

    const headings = Array.from(document.querySelectorAll("h1, h2, h3"))
      .map((el) => (el.innerText || "").replace(/\s+/g, " ").trim())
      .filter((t) => t.length > 1 && t.length < 180)
      .slice(0, 25);

    const seenLinks = new Set();
    const links = [];
    for (const a of document.querySelectorAll("a[href]")) {
      const href = a.href || "";
      const text = (a.innerText || "").replace(/\s+/g, " ").trim();
      if (
        text.length >= 2 &&
        text.length <= 100 &&
        (href.startsWith("http://") || href.startsWith("https://")) &&
        !seenLinks.has(href)
      ) {
        seenLinks.add(href);
        links.push({ text, href });
        if (links.length >= 20) break;
      }
    }

    const root =
      document.querySelector("article") ||
      document.querySelector("main") ||
      document.body;

    let cleanText = "";
    if (root) {
      const clone = root.cloneNode(true);
      clone
        .querySelectorAll("script, style, noscript, svg, iframe, nav, footer")
        .forEach((n) => n.remove());
      cleanText = (clone.innerText || clone.textContent || "")
        .replace(/\n{3,}/g, "\n\n")
        .replace(/[ \t]{2,}/g, " ")
        .trim();
    }

    const words = cleanText ? cleanText.split(/\s+/).length : 0;
    return {
      title,
      url,
      selection,
      text: cleanText.slice(0, 16000),
      headings,
      links,
      word_count: words,
    };
  };

  // Distraction-free Reader Mode extractor
  window.__arthosExtractReader = () => {
    const title =
      (document.querySelector("h1")?.innerText || document.title || "Article").trim();
    const byline = (
      document.querySelector('meta[name="author"]')?.content ||
      document.querySelector('[rel="author"], .author, .byline')?.innerText ||
      ""
    ).trim();
    const site_name = (
      document.querySelector('meta[property="og:site_name"]')?.content ||
      location.hostname ||
      ""
    ).trim();

    let candidate =
      document.querySelector("article") ||
      document.querySelector('[role="main"]') ||
      document.querySelector("main");

    if (!candidate) {
      let best = document.body;
      let bestLen = 0;
      for (const el of document.querySelectorAll("div, section")) {
        const len = (el.innerText || "").trim().length;
        if (len > bestLen && el.querySelectorAll("p").length >= 2) {
          best = el;
          bestLen = len;
        }
      }
      candidate = best;
    }

    const clone = (candidate || document.body).cloneNode(true);
    clone
      .querySelectorAll(
        "script, style, noscript, iframe, form, button, nav, aside, footer, header, [role='banner'], [role='navigation'], .ad, .ads, .advertisement"
      )
      .forEach((el) => el.remove());

    // Strip inline event handlers and unsafe attributes
    for (const el of clone.querySelectorAll("*")) {
      for (const attr of Array.from(el.attributes)) {
        if (attr.name.startsWith("on") || attr.name === "style") {
          el.removeAttribute(attr.name);
        }
      }
    }

    const text = (clone.innerText || "").trim();
    const word_count = text ? text.split(/\s+/).length : 0;
    return {
      title,
      url: location.href,
      byline,
      site_name,
      html: clone.innerHTML,
      word_count,
    };
  };

  // Floating AI Selection Pill (Shadow DOM isolated)
  let pillHost = null;
  let pillTimer = null;

  function hidePill() {
    if (pillHost) {
      pillHost.style.display = "none";
    }
  }

  function ensurePill() {
    if (pillHost) return pillHost;
    pillHost = document.createElement("div");
    pillHost.id = "__arthos_selection_pill";
    pillHost.style.cssText =
      "position:fixed;z-index:2147483647;display:none;pointer-events:auto;";
    const shadow = pillHost.attachShadow({ mode: "closed" });
    shadow.innerHTML = `
      <style>
        .pill {
          display: flex;
          align-items: center;
          gap: 2px;
          padding: 4px;
          border-radius: 999px;
          background: rgba(16, 19, 28, 0.94);
          border: 1px solid rgba(139, 148, 255, 0.35);
          box-shadow: 0 10px 28px rgba(0,0,0,0.45);
          backdrop-filter: blur(12px);
          font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
          font-size: 12px;
          color: #eef1f8;
          user-select: none;
        }
        button {
          border: none;
          background: transparent;
          color: #dbe2f4;
          padding: 5px 10px;
          border-radius: 999px;
          cursor: pointer;
          font-size: 12px;
          font-weight: 500;
          display: flex;
          align-items: center;
          gap: 5px;
          transition: background 0.12s, color 0.12s;
        }
        button:hover {
          background: rgba(108, 123, 255, 0.24);
          color: #ffffff;
        }
        .spark { color: #8b98ff; font-weight: 700; }
      </style>
      <div class="pill">
        <button data-act="explain"><span class="spark">✦</span> Explain</button>
        <button data-act="summarize">Summarize</button>
        <button data-act="ask">Ask AI</button>
      </div>
    `;
    shadow.querySelectorAll("button").forEach((btn) => {
      btn.addEventListener("mousedown", (e) => {
        e.preventDefault();
        e.stopPropagation();
        const sel = (window.getSelection()?.toString() || "").trim();
        if (sel) {
          post({ cmd: "ask_selection", text: sel, action: btn.dataset.act });
        }
        hidePill();
      });
    });
    document.documentElement.appendChild(pillHost);
    return pillHost;
  }

  document.addEventListener("mouseup", () => {
    if (!window.__arthosSelectionPillEnabled) return;
    if (location.protocol === "arthos:" || location.hostname === "arthos.localhost") return;
    clearTimeout(pillTimer);
    pillTimer = setTimeout(() => {
      const selObj = window.getSelection();
      const text = (selObj?.toString() || "").trim();
      if (!selObj || selObj.rangeCount === 0 || text.length < 12) {
        hidePill();
        return;
      }
      const rect = selObj.getRangeAt(0).getBoundingClientRect();
      if (!rect || (rect.width === 0 && rect.height === 0)) {
        hidePill();
        return;
      }
      const host = ensurePill();
      const top = Math.max(8, rect.top - 42);
      const left = Math.min(window.innerWidth - 250, Math.max(8, rect.left));
      host.style.top = `${top}px`;
      host.style.left = `${left}px`;
      host.style.display = "block";
    }, 180);
  });

  document.addEventListener("scroll", () => {
    hidePill();
    hideAutofill();
  }, true);
  document.addEventListener("mousedown", (e) => {
    if (pillHost && e.target !== pillHost) {
      hidePill();
    }
    if (autofillHost && e.target !== autofillHost && e.target !== activeLoginField) {
      hideAutofill();
    }
  });

  // Shadow-DOM Password Autofill for synced Chrome / DPAPI Vault logins
  let autofillHost = null;
  let activeLoginField = null;

  function hideAutofill() {
    if (autofillHost) autofillHost.style.display = "none";
  }

  function setNativeValue(el, val) {
    if (!el || val == null) return;
    const desc = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value");
    if (desc && desc.set) {
      desc.set.call(el, val);
    } else {
      el.value = val;
    }
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  }

  function findCredentialTargets(anchor) {
    const form = anchor?.closest("form") || document;
    const passInput =
      (anchor?.type === "password" ? anchor : null) ||
      form.querySelector('input[type="password"]:not([disabled])') ||
      document.querySelector('input[type="password"]:not([disabled])');

    const userSelectors = [
      'input[autocomplete="username"]',
      'input[autocomplete="email"]',
      'input[type="email"]',
      'input[name*="user" i]',
      'input[name*="login" i]',
      'input[name*="email" i]',
      'input[id*="user" i]',
      'input[id*="email" i]',
      'input[type="text"]',
    ];
    let userInput = null;
    if (anchor && anchor.type !== "password" && (anchor.type === "text" || anchor.type === "email" || !anchor.type)) {
      userInput = anchor;
    } else {
      for (const sel of userSelectors) {
        const candidate = form.querySelector(sel);
        if (candidate && candidate !== passInput && candidate.offsetParent !== null) {
          userInput = candidate;
          break;
        }
      }
    }
    return { userInput, passInput };
  }

  function isLoginField(el) {
    if (!el || el.tagName !== "INPUT") return false;
    const t = (el.type || "text").toLowerCase();
    if (t === "password") return true;
    if (t === "email" || t === "text") {
      const form = el.closest("form");
      if (form && form.querySelector('input[type="password"]')) return true;
      if ((el.autocomplete || "").includes("username") || (el.autocomplete || "").includes("email")) return true;
    }
    return false;
  }

  document.addEventListener(
    "focusin",
    (e) => {
      if (location.protocol === "arthos:" || location.hostname === "arthos.localhost") return;
      if (isLoginField(e.target)) {
        activeLoginField = e.target;
        post({ cmd: "request_autofill" });
      }
    },
    true
  );

  window.__arthosApplyAutofill = (creds) => {
    if (!Array.isArray(creds) || creds.length === 0) {
      hideAutofill();
      return;
    }
    const anchor = activeLoginField || document.querySelector('input[type="password"]');
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    if (!rect || (rect.width === 0 && rect.height === 0)) return;

    if (!autofillHost) {
      autofillHost = document.createElement("div");
      autofillHost.id = "__arthos_autofill";
      autofillHost.style.cssText =
        "position:fixed;z-index:2147483647;display:none;pointer-events:auto;";
      autofillHost.attachShadow({ mode: "closed" });
      document.documentElement.appendChild(autofillHost);
    }

    const shadow = autofillHost.shadowRoot;
    shadow.innerHTML = "";
    const style = document.createElement("style");
    style.textContent = `
      .af-card {
        min-width: 220px;
        max-width: 310px;
        padding: 5px;
        border-radius: 10px;
        background: rgba(16, 19, 28, 0.96);
        border: 1px solid rgba(124, 138, 255, 0.38);
        box-shadow: 0 12px 30px rgba(0,0,0,0.5);
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        color: #eef1f8;
      }
      .af-hdr {
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.05em;
        text-transform: uppercase;
        color: #9aa4c2;
        padding: 4px 8px 3px;
      }
      button.af-item {
        width: 100%;
        text-align: left;
        border: none;
        background: transparent;
        color: #eef1f8;
        padding: 7px 8px;
        border-radius: 7px;
        cursor: pointer;
        display: flex;
        flex-direction: column;
        gap: 1px;
      }
      button.af-item:hover {
        background: rgba(108, 123, 255, 0.22);
      }
      .af-user { font-size: 12.5px; font-weight: 600; }
      .af-orig { font-size: 11px; color: #9aa4c2; }
    `;
    const card = document.createElement("div");
    card.className = "af-card";
    const hdr = document.createElement("div");
    hdr.className = "af-hdr";
    hdr.textContent = "Arthos Vault Autofill";
    card.appendChild(hdr);

    for (const c of creds.slice(0, 5)) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "af-item";
      const uEl = document.createElement("span");
      uEl.className = "af-user";
      uEl.textContent = c.username || "(no username)";
      const oEl = document.createElement("span");
      oEl.className = "af-orig";
      oEl.textContent = `${c.origin || location.hostname} • ••••••••`;
      btn.appendChild(uEl);
      btn.appendChild(oEl);
      btn.addEventListener("mousedown", (ev) => {
        ev.preventDefault();
        ev.stopPropagation();
        const { userInput, passInput } = findCredentialTargets(anchor);
        if (userInput && c.username) setNativeValue(userInput, c.username);
        if (passInput && c.password) setNativeValue(passInput, c.password);
        hideAutofill();
      });
      card.appendChild(btn);
    }

    shadow.appendChild(style);
    shadow.appendChild(card);

    autofillHost.style.top = `${Math.min(window.innerHeight - 120, rect.bottom + 4)}px`;
    autofillHost.style.left = `${Math.min(window.innerWidth - 260, Math.max(8, rect.left))}px`;
    autofillHost.style.display = "block";
  };
})();
