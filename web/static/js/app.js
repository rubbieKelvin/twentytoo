/* Twentytoo enhancement layer (01-ui-kit §9).
 *
 * Everything here is optional: the product is fully functional without
 * this file (links, forms, and <details>/<dialog> elements do the work).
 * This script adds exactly the behaviors CSS alone cannot, using
 * DELEGATED listeners only — htmx swaps replace DOM nodes, so per-element
 * bindings would die on the first swap. Keep this file under ~2KB of
 * logic; anything bigger is a signal the feature needs a different design.
 */
(function () {
  "use strict";

  var root = document.documentElement;
  var body = document.body;

  /* Presence flag for CSS (e.g. dialogs degrade to inline cards) and
     motion gate for view transitions (01 §8.5). */
  root.classList.add("js");
  if (!window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    root.classList.add("tt-motion");
  }

  /* htmx config defaults: swap 422 responses (form re-renders) and keep
     page titles in sync on boosted swaps. */
  document.addEventListener("htmx:beforeSwap", function (e) {
    if (e.detail.xhr.status === 422) {
      e.detail.shouldSwap = true;
      e.detail.isError = false;
    }
  });
  document.addEventListener("htmx:afterSwap", function (e) {
    var text = e.detail.xhr.responseText;
    if (!text) return;
    var match = /<title[^>]*>([^<]*)<\/title>/i.exec(text);
    if (match) document.title = match[1];
  });

  /* Toasts (01 §7.10): auto-dismiss after 4s, pause on hover. */
  function arm(toast) {
    if (!toast || toast.getAttribute("data-tt-armed")) return;
    toast.setAttribute("data-tt-armed", "1");
    var timer = setTimeout(function () { toast.remove(); }, 4000);
    toast.addEventListener("mouseenter", function () { clearTimeout(timer); });
    toast.addEventListener("mouseleave", function () {
      timer = setTimeout(function () { toast.remove(); }, 1200);
    });
  }

  function renderToast(data) {
    var host = document.getElementById("toasts");
    if (!host) return;
    var toast = document.createElement("div");
    toast.className = "toast toast--" + (data.kind || "info");
    toast.setAttribute("data-tt-toast", "");
    toast.setAttribute("role", "status");
    toast.textContent = data.message || "";
    host.appendChild(toast);
    arm(toast);
  }

  /* Mutation feedback: the server sends HX-Trigger {"tt:toast": {...}}
     alongside HX-Redirect. The redirect destroys the current document, so
     an OOB swap would die with it — stash the payload and render the toast
     on the redirected page. */
  document.addEventListener("tt:toast", function (e) {
    try {
      sessionStorage.setItem("tt.flash", JSON.stringify(e.detail || {}));
    } catch (_) { /* storage unavailable: drop the toast */ }
  });
  try {
    var flash = sessionStorage.getItem("tt.flash");
    if (flash) {
      sessionStorage.removeItem("tt.flash");
      renderToast(JSON.parse(flash));
    }
  } catch (_) { /* no flash or broken storage */ }

  /* Sidebar: desktop rail collapse (persisted) and mobile drawer. */
  if (localStorage.getItem("tt.sidebar") === "collapsed") {
    body.classList.add("sidebar-collapsed");
  }
  document.addEventListener("click", function (e) {
    if (!e.target.closest("[data-tt-sidebar-toggle]")) return;
    if (window.matchMedia("(max-width: 900px)").matches) {
      body.classList.toggle("sidebar-open");
    } else {
      body.classList.toggle("sidebar-collapsed");
      localStorage.setItem(
        "tt.sidebar",
        body.classList.contains("sidebar-collapsed") ? "collapsed" : "expanded"
      );
    }
  });

  /* Menus: close open <details data-tt-menu> on outside click / Escape. */
  document.addEventListener("click", function (e) {
    document.querySelectorAll("details[data-tt-menu][open]").forEach(function (menu) {
      if (!menu.contains(e.target)) menu.removeAttribute("open");
    });
  });
  document.addEventListener("keydown", function (e) {
    if (e.key !== "Escape") return;
    document.querySelectorAll("details[data-tt-menu][open]").forEach(function (menu) {
      menu.removeAttribute("open");
    });
  });

  /* Dialogs: open targets with showModal(), close on backdrop click.
     Escape and focus management are native <dialog> behavior. */
  document.addEventListener("click", function (e) {
    var opener = e.target.closest("[data-tt-dialog-open]");
    if (opener) {
      var dialog = document.querySelector(opener.getAttribute("data-tt-dialog-open"));
      if (dialog) dialog.showModal();
      return;
    }
    if (e.target.tagName === "DIALOG") {
      var rect = e.target.getBoundingClientRect();
      var inside =
        e.clientX >= rect.left && e.clientX <= rect.right &&
        e.clientY >= rect.top && e.clientY <= rect.bottom;
      if (!inside) e.target.close();
    }
  });
})();
