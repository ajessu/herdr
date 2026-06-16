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
