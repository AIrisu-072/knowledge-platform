// Keep the existing jsdom runner; expose Node's native Fetch request/response
// classes so route tests can inspect the generated SDK's serialized HTTP requests.
const { TestEnvironment } = require('jest-environment-jsdom');
module.exports = class FetchDomEnvironment extends TestEnvironment {
  constructor(...args) {
    super(...args);
    Object.assign(this.global, { Request, Response, Headers, fetch });
  }
};
