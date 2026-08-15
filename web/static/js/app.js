/* Twentytoo enhancement layer (01-ui-kit §9).
 *
 * Everything here is optional: the product is fully functional without
 * this file — links, forms, and the server-rendered `.show` toasts do the
 * work. Tabler's bundle (`tabler.min.js`) provides the component
 * behaviors (modals, dropdowns, toasts, navbar collapse) via data
 * attributes; this script adds only the load-time behaviors data
 * attributes cannot express. Keep it small: anything bigger belongs in
 * Tabler itself or behind a feature flag.
 */
(function () {
  "use strict";

  var tabler = window.tabler || {};
  var Toast = tabler.Toast || (tabler.bootstrap && tabler.bootstrap.Toast);

  /* Server-rendered flash toasts: hand them to the Tabler API so
     autohide and the close button work. Without JS (or without the
     bundle) the `.show` class keeps them visible and dismissible only
     by navigation. */
  if (Toast) {
    document.querySelectorAll('.toast[data-bs-toggle="toast"]').forEach(function (el) {
      new Toast(el).show();
    });
  }
})();
