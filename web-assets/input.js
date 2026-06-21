(function () {
  'use strict';

  // term._herdrInputBound guards against re-binding xterm.js listeners on
  // reconnect. The handlers read the live socket via getSocket() so they keep
  // working after the WebSocket is replaced.
  window.herdrSetupInput = function (term, getSocket) {
    if (term._herdrInputBound) {
      return;
    }
    term._herdrInputBound = true;

    var encoder = new TextEncoder();

    function send(payload) {
      var ws = getSocket();
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(payload);
      }
    }

    term.onData(function (data) {
      send(encoder.encode(data));
    });

    term.onBinary(function (data) {
      var bytes = new Uint8Array(data.length);
      for (var i = 0; i < data.length; i++) {
        bytes[i] = data.charCodeAt(i);
      }
      send(bytes);
    });

    term.attachCustomKeyEventHandler(function (event) {
      if (event.type !== 'keydown') return true;
      if ((event.ctrlKey || event.metaKey) && event.key === 'v') {
        return false;
      }
      return true;
    });

    // xterm.js routes keyboard input through a hidden textarea, so the paste
    // event fires there rather than on the outer element. Fall back to the
    // element if the textarea isn't exposed.
    var pasteTarget = term.textarea || term.element;
    pasteTarget.addEventListener('paste', function (event) {
      event.preventDefault();
      var text = (event.clipboardData || window.clipboardData).getData('text');
      if (text) {
        send(JSON.stringify({ type: 'paste', text: text }));
      }
    });
  };
})();

(function () {
  'use strict';

  window.herdrSetupTouchScroll = function (term, getSocket) {
    if (term._herdrTouchBound) return;

    var el = term.element;
    if (!el) return;
    term._herdrTouchBound = true;

    var encoder = new TextEncoder();
    var DEAD_ZONE = 5;
    var MAX_STEPS = 5;
    var tracking = null;
    var screenEl = el.querySelector('.xterm-screen');

    var dims = term._core && term._core._renderService
      ? term._core._renderService.dimensions : null;
    var cw = dims ? dims.css.cell.width
      : (screenEl ? screenEl.getBoundingClientRect().width / term.cols : 9);
    var ch = dims ? dims.css.cell.height
      : (screenEl ? screenEl.getBoundingClientRect().height / term.rows : 18);

    function send(payload) {
      var ws = getSocket();
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(payload);
      }
    }

    function cellAt(clientX, clientY) {
      if (!screenEl) return { col: 1, row: 1 };
      var rect = screenEl.getBoundingClientRect();
      var col = Math.max(1, Math.min(Math.floor((clientX - rect.left) / cw) + 1, term.cols));
      var row = Math.max(1, Math.min(Math.floor((clientY - rect.top) / ch) + 1, term.rows));
      return { col: col, row: row };
    }

    function emitScroll(direction, clientX, clientY) {
      var cell = cellAt(clientX, clientY);
      var seq = '\x1b[<' + direction + ';' + cell.col + ';' + cell.row + 'M';
      send(encoder.encode(seq));
    }

    el.addEventListener('touchstart', function (e) {
      if (e.touches.length !== 1) return;
      var t = e.touches[0];
      tracking = { id: t.identifier, startY: t.clientY, lastY: t.clientY, scrolled: false };
    }, { passive: true });

    el.addEventListener('touchmove', function (e) {
      if (!tracking) return;
      if (e.touches.length > 1) { tracking = null; return; }
      var t = null;
      for (var i = 0; i < e.changedTouches.length; i++) {
        if (e.changedTouches[i].identifier === tracking.id) {
          t = e.changedTouches[i];
          break;
        }
      }
      if (!t) return;

      var deltaY = t.clientY - tracking.lastY;
      var totalMove = Math.abs(t.clientY - tracking.startY);

      if (!tracking.scrolled && totalMove < DEAD_ZONE) return;
      tracking.scrolled = true;
      e.preventDefault();

      var direction = deltaY < 0 ? 64 : 65;
      var steps = Math.min(Math.floor(Math.abs(deltaY) / ch), MAX_STEPS);
      if (steps > 0) {
        for (var s = 0; s < steps; s++) {
          emitScroll(direction, t.clientX, t.clientY);
        }
        tracking.lastY += (deltaY > 0 ? 1 : -1) * steps * ch;
      }
    }, { passive: false });

    el.addEventListener('touchend', function (e) {
      if (!tracking) return;
      for (var i = 0; i < e.changedTouches.length; i++) {
        if (e.changedTouches[i].identifier === tracking.id) {
          tracking = null;
          break;
        }
      }
    }, { passive: true });

    el.addEventListener('touchcancel', function () {
      tracking = null;
    }, { passive: true });
  };
})();
