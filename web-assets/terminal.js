(function () {
  'use strict';

  var term = null;
  var ws = null;
  var fitAddon = null;

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

  function connect() {
    var opened = false;
    var proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    ws = new WebSocket(proto + '//' + location.host + '/ws');
    ws.binaryType = 'arraybuffer';

    ws.onopen = function () {
      opened = true;
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
      // The /ws upgrade is rejected before opening (401 for a missing/expired
      // session cookie, 403 for an origin mismatch). Re-entering the token only
      // fixes the 401 case, so log the close for the 403 case to be diagnosable
      // rather than a silent login loop.
      if (!opened) {
        console.warn('herdr: websocket closed before opening', event.code, event.reason);
        window.herdrShowLogin();
        return;
      }
      term.write('\r\n\x1b[31mDisconnected (code ' + event.code + '). Refresh to reconnect.\x1b[0m\r\n');
    };
  }

  window.herdrInitTerminal = function () {
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

    try {
      var webglAddon = new WebglAddon.WebglAddon();
      webglAddon.onContextLoss(function () { webglAddon.dispose(); });
      term.loadAddon(webglAddon);
    } catch (e) {}

    fitAddon.fit();
    connect();
  };
})();
