(function () {
  'use strict';

  // term._herdrResizeBound guards against attaching duplicate observers on
  // reconnect. The handler reads the live socket via getSocket().
  window.herdrSetupResize = function (term, getSocket, fitAddon) {
    if (term._herdrResizeBound) {
      return;
    }
    term._herdrResizeBound = true;

    var resizeTimeout = null;

    function sendResize() {
      var ws = getSocket();
      if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({
          type: 'resize',
          cols: term.cols,
          rows: term.rows,
        }));
      }
    }

    function handleResize() {
      if (resizeTimeout) clearTimeout(resizeTimeout);
      resizeTimeout = setTimeout(function () {
        fitAddon.fit();
        sendResize();
      }, 100);
    }

    new ResizeObserver(handleResize).observe(document.getElementById('terminal'));
    window.addEventListener('resize', handleResize);
  };
})();
