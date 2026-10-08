// Desktop shell only (injected before the bundled app's scripts, main frame).
// WebKitGTK 2.52 crashes the whole app (SIGSEGV in
// webkit_uri_scheme_request_get_http_body) when a request to the app's own
// custom scheme carries a Blob/FormData body, which every multipart upload
// does. Same-origin request bodies are therefore materialized into an
// ArrayBuffer inside the page first; the bytes, method, headers (including the
// multipart boundary) and abort signal are unchanged. Other origins and the
// Tauri IPC channel pass through untouched.
(() => {
  const nativeFetch = window.fetch.bind(window);
  const sameOrigin = (url) => {
    try {
      return new URL(url, window.location.href).origin === window.location.origin;
    } catch {
      return false;
    }
  };
  window.fetch = async function fetch(input, init) {
    // Building the Request may take over the body of a Request input, so the
    // pass-through sends this Request rather than the original arguments.
    const request = new Request(input, init);
    if (request.method === 'GET' || request.method === 'HEAD' || !sameOrigin(request.url)) {
      return nativeFetch(request);
    }
    const body = await request.arrayBuffer();
    return nativeFetch(request.url, {
      method: request.method,
      headers: request.headers,
      body,
      signal: request.signal,
      credentials: request.credentials,
      cache: request.cache,
      redirect: request.redirect,
      referrerPolicy: request.referrerPolicy,
    });
  };
})();
