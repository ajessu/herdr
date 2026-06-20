(function () {
  'use strict';

  var term = null;
  var ws = null;
  var fitAddon = null;

  var reconnectAttempts = 0;
  var reconnectTimer = null;
  var maxAttempts = 6;
  var baseDelay = 1000;
  var maxDelay = 30000;

  function getSocket() {
    return ws;
  }

  function handleControlMessage(msg) {
    switch (msg.type) {
      case 'window_title':
        if (msg.title != null) document.title = msg.title;
        break;
      case 'clipboard':
        if (navigator.clipboard && msg.data) {
          navigator.clipboard.writeText(msg.data).catch(function () {});
        }
        break;
      case 'mouse_capture':
        break;
      case 'notify':
        break;
    }
  }

  function computeBackoffDelay(attempt) {
    var delay = Math.min(maxDelay, baseDelay * Math.pow(2, attempt));
    return delay * (0.5 + Math.random() * 0.5);
  }

  function attemptReconnect() {
    if (reconnectAttempts >= maxAttempts) {
      term.write('\r\n\x1b[31mConnection lost — reload to retry.\x1b[0m\r\n');
      return;
    }

    var attempt = reconnectAttempts;
    reconnectAttempts++;
    var delay = computeBackoffDelay(attempt);

    reconnectTimer = setTimeout(function () {
      reconnectTimer = null;
      fetch('/config.json', { credentials: 'same-origin' })
        .then(function (resp) {
          if (!resp.ok) throw new Error('config fetch failed');
          return resp.json();
        })
        .then(function (config) {
          if (!config || config.mode !== 'trust-proxy') {
            window.herdrMode = 'standalone';
            window.herdrShowLogin();
            return;
          }
          connect();
        })
        .catch(function () {
          attemptReconnect();
        });
    }, delay);
  }

  function connect() {
    var opened = false;
    var proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    ws = new WebSocket(proto + '//' + location.host + '/ws');
    ws.binaryType = 'arraybuffer';

    ws.onopen = function () {
      opened = true;
      reconnectAttempts = 0;
      var hello = JSON.stringify({
        type: 'hello',
        cols: term.cols,
        rows: term.rows,
      });
      ws.send(hello);
      window.herdrSetupInput(term, getSocket);
      window.herdrSetupResize(term, getSocket, fitAddon);
    };

    ws.onmessage = function (event) {
      if (event.data instanceof ArrayBuffer) {
        term.write(new Uint8Array(event.data));
      } else {
        try {
          handleControlMessage(JSON.parse(event.data));
        } catch (e) {}
      }
    };

    ws.onclose = function (event) {
      if (event.code === 1000) return;
      if (!opened) {
        if (window.herdrMode === 'trust-proxy') {
          attemptReconnect();
        } else {
          window.herdrShowLogin();
        }
        return;
      }
      term.write('\r\n\x1b[31mDisconnected (code ' + event.code + '). Refresh to reconnect.\x1b[0m\r\n');
    };
  }

  window.herdrInitTerminal = function () {
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    reconnectAttempts = 0;
    if (term) {
      connect();
      return;
    }

    var container = document.getElementById('terminal');
    term = new Terminal({
      scrollback: 0,
      allowProposedApi: true,
      cursorBlink: true,
    });

    fitAddon = new FitAddon.FitAddon();
    term.loadAddon(fitAddon);

    var webLinksAddon = new WebLinksAddon.WebLinksAddon();
    term.loadAddon(webLinksAddon);

    term.open(container);

    if (new URLSearchParams(location.search).get('renderer') === 'webgl') {
      try {
        var webglAddon = new WebglAddon.WebglAddon();
        webglAddon.onContextLoss(function () { webglAddon.dispose(); });
        term.loadAddon(webglAddon);
      } catch (e) {}
    }

    fitAddon.fit();
    connect();
  };
})();
