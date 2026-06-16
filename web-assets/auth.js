(function () {
  'use strict';

  var loginEl = document.getElementById('login');
  var terminalEl = document.getElementById('terminal');
  var form = document.getElementById('login-form');
  var tokenInput = document.getElementById('token-input');
  var errorEl = document.getElementById('login-error');

  function showError(msg) {
    errorEl.textContent = msg;
    errorEl.hidden = false;
  }

  function showTerminal() {
    loginEl.hidden = true;
    terminalEl.hidden = false;
    window.herdrInitTerminal();
  }

  form.addEventListener('submit', function (e) {
    e.preventDefault();
    errorEl.hidden = true;
    var token = tokenInput.value.trim();
    if (!token) {
      showError('Token is required');
      return;
    }

    fetch('/auth', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token: token }),
    })
      .then(function (resp) {
        if (resp.ok) {
          showTerminal();
        } else {
          showError('Invalid token');
        }
      })
      .catch(function () {
        showError('Connection failed');
      });
  });

  window.herdrShowLogin = function () {
    terminalEl.hidden = true;
    loginEl.hidden = false;
    tokenInput.value = '';
    tokenInput.focus();
  };
})();
