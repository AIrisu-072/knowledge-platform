// Minimal W3C WebDriver client for driving the real desktop app through
// tauri-driver → WebKitWebDriver. TEST-ONLY; no third-party dependencies.

const ELEMENT = 'element-6066-11e4-a52e-4f735466cecf';

export class WebDriverError extends Error {
  constructor(status, body) {
    super(`WebDriver ${status}: ${body?.value?.error ?? 'unknown'} ${body?.value?.message ?? ''}`.trim());
    this.status = status;
    this.code = body?.value?.error;
  }
}

// The client gives up only after WebDriver's own longest timeout (the 180 s
// script timeout set in Session.create), so a slow transfer script fails with
// WebDriver's script-timeout error instead of a client abort mid-script.
const SCRIPT_TIMEOUT = 180_000;
const CLIENT_TIMEOUT = SCRIPT_TIMEOUT + 20_000;

async function call(base, method, path, body) {
  const response = await fetch(`${base}${path}`, {
    method,
    headers: body === undefined ? undefined : { 'content-type': 'application/json; charset=utf-8' },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(CLIENT_TIMEOUT),
  });
  const text = await response.text();
  const json = text ? JSON.parse(text) : {};
  if (!response.ok) throw new WebDriverError(response.status, json);
  return json.value;
}

export class Session {
  constructor(base, id) {
    this.base = base;
    this.id = id;
  }

  static async create(base, application, args = []) {
    const value = await call(base, 'POST', '/session', {
      capabilities: { alwaysMatch: { browserName: 'wry', 'tauri:options': { application, args } } },
    });
    const session = new Session(base, value.sessionId);
    // Real uploads/downloads through the shell can exceed the 30 s default.
    await call(base, 'POST', `/session/${value.sessionId}/timeouts`, { script: SCRIPT_TIMEOUT, pageLoad: 60_000 });
    return session;
  }

  #path(suffix) {
    return `/session/${this.id}${suffix}`;
  }

  async delete() {
    try {
      await call(this.base, 'DELETE', this.#path(''));
    } catch {
      // The app may already be gone (for example after a deliberate exit).
    }
  }

  url() {
    return call(this.base, 'GET', this.#path('/url'));
  }

  title() {
    return call(this.base, 'GET', this.#path('/title'));
  }

  /** Top-level browsing contexts (windows) of the app. */
  handles() {
    return call(this.base, 'GET', this.#path('/window/handles'));
  }

  back() {
    return call(this.base, 'POST', this.#path('/back'), {});
  }

  /** Runs a synchronous script in the page; returns its JSON value. */
  execute(script, args = []) {
    return call(this.base, 'POST', this.#path('/execute/sync'), { script, args });
  }

  /** Runs an async script; the last argument is the completion callback. */
  executeAsync(script, args = []) {
    return call(this.base, 'POST', this.#path('/execute/async'), { script, args });
  }

  async find(css) {
    const value = await call(this.base, 'POST', this.#path('/element'), { using: 'css selector', value: css });
    return new Element(this, value[ELEMENT]);
  }

  async findAll(css) {
    const values = await call(this.base, 'POST', this.#path('/elements'), { using: 'css selector', value: css });
    return values.map((value) => new Element(this, value[ELEMENT]));
  }

  async findByXPath(xpath) {
    const value = await call(this.base, 'POST', this.#path('/element'), { using: 'xpath', value: xpath });
    return new Element(this, value[ELEMENT]);
  }

  async active() {
    const value = await call(this.base, 'GET', this.#path('/element/active'));
    return new Element(this, value[ELEMENT]);
  }

  /** Sends real key events to the focused element (W3C actions). */
  async keys(sequence) {
    const actions = [];
    for (const key of sequence) actions.push({ type: 'keyDown', value: key }, { type: 'keyUp', value: key });
    await call(this.base, 'POST', this.#path('/actions'), { actions: [{ type: 'key', id: 'keyboard', actions }] });
    await call(this.base, 'DELETE', this.#path('/actions'));
  }

  /** Presses the keys together (for example Shift+Tab), releasing in reverse. */
  async chord(keys) {
    const actions = [...keys.map((value) => ({ type: 'keyDown', value })), ...[...keys].reverse().map((value) => ({ type: 'keyUp', value }))];
    await call(this.base, 'POST', this.#path('/actions'), { actions: [{ type: 'key', id: 'keyboard', actions }] });
    await call(this.base, 'DELETE', this.#path('/actions'));
  }

  async screenshot() {
    return Buffer.from(await call(this.base, 'GET', this.#path('/screenshot')), 'base64');
  }

  async waitFor(predicate, { timeout = 15_000, interval = 100, message = 'condition' } = {}) {
    const deadline = Date.now() + timeout;
    let last;
    while (Date.now() < deadline) {
      try {
        const value = await predicate();
        if (value) return value;
      } catch (error) {
        last = error;
      }
      await new Promise((resolve) => setTimeout(resolve, interval));
    }
    throw new Error(`Timed out waiting for ${message}${last ? `: ${last.message}` : ''}`);
  }

  /** Waits for an element matching the CSS selector whose text includes `text`. */
  waitForText(css, text, options = {}) {
    return this.waitFor(async () => {
      for (const element of await this.findAll(css)) {
        if ((await element.text()).includes(text)) return element;
      }
      return undefined;
    }, { message: `${css} containing ${JSON.stringify(text)}`, ...options });
  }

  /** Text of the whole document body. */
  bodyText() {
    return this.execute('return document.body.innerText;');
  }
}

export class Element {
  constructor(session, id) {
    this.session = session;
    this.id = id;
  }

  #path(suffix) {
    return `/session/${this.session.id}/element/${this.id}${suffix}`;
  }

  click() {
    return call(this.session.base, 'POST', this.#path('/click'), {});
  }

  /** Two real pointer clicks in quick succession on the element (a double click). */
  async doubleClick() {
    // Viewport coordinates of the element's centre: WebKitWebDriver rejects an
    // element-relative move inside a modal overlay as "out of bounds".
    const { x, y } = await this.session.execute(`arguments[0].scrollIntoView({ block: 'center' });
      const r = arguments[0].getBoundingClientRect();
      return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) };`, [this.ref]);
    const press = [{ type: 'pointerDown', button: 0 }, { type: 'pointerUp', button: 0 }];
    await call(this.session.base, 'POST', `/session/${this.session.id}/actions`, {
      actions: [{ type: 'pointer', id: 'mouse', parameters: { pointerType: 'mouse' }, actions: [{ type: 'pointerMove', origin: 'viewport', x, y }, ...press, ...press] }],
    });
    await call(this.session.base, 'DELETE', `/session/${this.session.id}/actions`);
  }

  type(text) {
    return call(this.session.base, 'POST', this.#path('/value'), { text });
  }

  clear() {
    return call(this.session.base, 'POST', this.#path('/clear'), {});
  }

  text() {
    return call(this.session.base, 'GET', this.#path('/text'));
  }

  attribute(name) {
    return call(this.session.base, 'GET', this.#path(`/attribute/${encodeURIComponent(name)}`));
  }

  property(name) {
    return call(this.session.base, 'GET', this.#path(`/property/${encodeURIComponent(name)}`));
  }

  enabled() {
    return call(this.session.base, 'GET', this.#path('/enabled'));
  }

  displayed() {
    return call(this.session.base, 'GET', this.#path('/displayed'));
  }

  async find(css) {
    const value = await call(this.session.base, 'POST', this.#path('/element'), { using: 'css selector', value: css });
    return new Element(this.session, value[ELEMENT]);
  }

  async findAll(css) {
    const values = await call(this.session.base, 'POST', this.#path('/elements'), { using: 'css selector', value: css });
    return values.map((value) => new Element(this.session, value[ELEMENT]));
  }

  /** WebDriver element reference, for passing into execute(). */
  get ref() {
    return { [ELEMENT]: this.id };
  }
}

export const Keys = Object.freeze({
  TAB: '',
  ENTER: '',
  ESCAPE: '',
  SHIFT: '',
  ARROW_DOWN: '',
  ARROW_UP: '',
});
