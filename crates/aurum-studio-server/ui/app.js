// Aurum Studio's page.
//
// The token arrives in the URL, because opening a browser at a URL is the only
// way to hand it over without a login step. It is moved into sessionStorage and
// stripped from the address bar immediately, so it does not persist in history
// and cannot leak through a Referer header. The page itself never contains the
// token: the server refuses to bake it in.

(function () {
  'use strict';

  const TOKEN_KEY = 'aurum_token';

  // Take the token from the URL once, then get it out of sight.
  const params = new URLSearchParams(window.location.search);
  const fromUrl = params.get('t');
  if (fromUrl) {
    sessionStorage.setItem(TOKEN_KEY, fromUrl);
    params.delete('t');
    const query = params.toString();
    window.history.replaceState(
      {},
      '',
      window.location.pathname + (query ? '?' + query : '')
    );
  }
  const token = sessionStorage.getItem(TOKEN_KEY) || '';

  const $ = (id) => document.getElementById(id);
  const connection = $('connection');
  const connectionText = connection.querySelector('.connection-text');
  const project = $('project');
  const projectValue = project.querySelector('.chip-value');
  const log = $('log');
  const health = $('health');
  const busy = $('busy');
  const busyText = busy.querySelector('.status-text');

  const MAX_LINES = 500;
  const EMPTY_LOG = 'Nothing has happened yet.';

  // -- small renderers ---------------------------------------------------

  function setState(state, label) {
    connection.dataset.state = state;
    connectionText.textContent = label;
  }

  function setProject(name) {
    projectValue.textContent = name || 'project';
    project.title = name || '';
  }

  function setBusy(text) {
    const active = Boolean(text);
    busy.dataset.active = String(active);
    busyText.textContent = text || '';
  }

  function clearEmptyLog() {
    const placeholder = log.querySelector('.log-empty');
    if (placeholder) placeholder.remove();
  }

  function say(text, kind) {
    clearEmptyLog();

    const line = document.createElement('li');
    if (kind) line.className = kind;

    const time = document.createElement('time');
    time.textContent = new Date().toLocaleTimeString([], { hour12: false });

    const body = document.createElement('span');
    body.textContent = text;

    line.append(time, body);

    // Follow the tail only when the reader is already at the bottom, so a
    // line arriving never yanks the view away from something being read.
    const atBottom = log.scrollTop + log.clientHeight >= log.scrollHeight - 28;
    log.append(line);
    while (log.childElementCount > MAX_LINES) log.removeChild(log.firstChild);
    if (atBottom) log.scrollTop = log.scrollHeight;
  }

  function renderHealth(event) {
    health.classList.remove('empty');
    health.replaceChildren();

    const verdict = document.createElement('div');
    // The class is the label the core emits, so the stylesheet has to name the
    // same string. A test in assets.rs holds the two together.
    verdict.className = 'verdict ' + event.verdict;
    verdict.textContent = event.verdict;

    const summary = document.createElement('div');
    summary.className = 'summary';
    summary.textContent = event.summary;

    health.append(verdict, summary);
  }

  // -- requests ----------------------------------------------------------

  async function post(path, body) {
    return fetch(path, {
      method: 'POST',
      headers: {
        'X-Aurum-Token': token,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(body || {}),
    });
  }

  document.querySelectorAll('button[data-command]').forEach((button) => {
    button.addEventListener('click', async () => {
      const command = button.dataset.command;
      const extra = button.dataset.args ? JSON.parse(button.dataset.args) : {};
      setBusy(command + ' requested');
      try {
        const response = await post('/api/command', { command, ...extra });
        if (!response.ok) {
          say((await response.text()).trim(), 'bad');
          setBusy('');
        }
      } catch (error) {
        say('could not reach Studio: ' + error, 'bad');
        setBusy('');
      }
    });
  });

  $('clear').addEventListener('click', () => {
    log.replaceChildren();
    const placeholder = document.createElement('li');
    placeholder.className = 'log-empty';
    placeholder.textContent = EMPTY_LOG;
    log.append(placeholder);
  });

  $('shutdown').addEventListener('click', async () => {
    setState('closed', 'stopped');
    try {
      await post('/api/stop');
    } catch (error) {
      /* The server may close the connection before it can answer. */
    }
    say('Studio has been asked to shut down. This page can be closed.', 'note');
  });

  // -- events ------------------------------------------------------------

  function handle(event) {
    switch (event.kind) {
      case 'health':
        renderHealth(event);
        say('health: ' + event.verdict + ' — ' + event.summary);
        break;
      case 'build-started':
        setBusy('building ' + event.package + ' (' + event.profile + ')');
        say('building ' + event.package + ' (' + event.profile + ')', 'note');
        break;
      case 'build-finished':
        setBusy('');
        say(
          (event.ok ? 'build ok: ' : 'build failed: ') + event.summary,
          event.ok ? 'good' : 'bad'
        );
        break;
      case 'process-started':
        say(event.process + ' started (pid ' + event.pid + ')', 'good');
        break;
      case 'process-stopped':
        say(event.process + ': ' + event.description);
        break;
      case 'change':
        say(event.verdict + ': ' + event.reason);
        break;
      case 'log':
        say(event.line);
        break;
      case 'error':
        setBusy('');
        say(event.message, 'bad');
        break;
      case 'stopped':
        setState('closed', 'stopped');
        say('Studio stopped.', 'note');
        break;
      default:
        say(JSON.stringify(event));
    }
  }

  function connect() {
    setState('connecting', 'connecting');
    // The token rides in the query here rather than a header because
    // EventSource cannot set one, and it is already out of the address bar.
    const stream = new EventSource('/api/events?t=' + encodeURIComponent(token));

    stream.onopen = () => setState('open', 'connected');
    stream.onerror = () => {
      // EventSource retries by itself; report it rather than giving up.
      setState('connecting', 'reconnecting');
    };
    stream.onmessage = (message) => {
      let event;
      try {
        event = JSON.parse(message.data);
      } catch (error) {
        return;
      }
      handle(event);
    };
  }

  // A snapshot first, so the page has something to say before the first event
  // arrives rather than showing an empty shell.
  fetch('/api/state', { headers: { 'X-Aurum-Token': token } })
    .then((response) => response.json())
    .then((state) => {
      setProject(state.project);
      say('Studio ' + state.studio + ' · ' + state.root, 'note');
    })
    .catch(() => setProject(''))
    .finally(connect);
})();
