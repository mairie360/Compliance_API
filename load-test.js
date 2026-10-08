// k6 load test, run by ./performance_test.sh (docker-compose-performance.yml).
//
// Built on the shared OpenAPI coverage module (mairie360/CICD `tests/k6/coverage.js`, MAIR-194):
// every operation of the spec served by the API needs exactly one handler ("METHOD /path", path
// as in openapi.json), k6 aborts at init otherwise. When you add an endpoint, add its handler to
// `readHandlers` (GET) or `writeHandlers` (any other method) and send its request through
// `request()` (raw `http.*` calls are not counted).
//
// Two scenarios share the spec, split by HTTP method (same shape as the five APIs, MAIR-195):
// - `reads`: the GET operations under the historical profile (ramp up to 20 VUs), against the
//   fixtures created once in setup() and removed in teardown();
// - `writes`: every other operation with 2 VUs. Each handler is self-contained: it creates what it
//   needs through `fixture()`, sends its request, then deletes what it created, so the handlers
//   do not depend on their order and the database ends as it started. The template publishes no
//   write operation, so the scenario only exists once the spec or `writeHandlers` has one.
import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import { createCoverage, loadSpec } from '/coverage.js';

const BASE_URL = (__ENV.BASE_URL || 'http://localhost:3004').replace(/\/+$/, '');

// Static HS256 service JWT (sub=zap-scan, role=service, exp=2100, signed with the stack's
// throwaway JWT_SECRET=b"secret"), the same one ZAP injects: only services call this API
// (MAIR-498, src/auth.rs). Public routes ignore it.
const TOKEN =
  __ENV.JWT ||
  'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ6YXAtc2NhbiIsInJvbGUiOiJzZXJ2aWNlIiwiZXhwIjo0MTAyNDQ0ODAwfQ.RqrSxraljpbUiU6ZABESaXsMoPVolPpy1QvXoZJPJCs'; // gitleaks:allow (test-only token of the throwaway stack secret)
const AUTH = { Authorization: `Bearer ${TOKEN}` };

// p(95) latency budget of each family of operations, in ms (reference machine).
const READ_BUDGET_MS = 200;
const WRITE_BUDGET_MS = 500;

const READ_METHODS = ['get', 'head', 'options'];

/** The served spec restricted to the operations whose method passes `keep`. */
function specSubset(spec, keep) {
  const paths = {};
  for (const path of Object.keys(spec.paths)) {
    const kept = {};
    for (const method of Object.keys(spec.paths[path])) {
      if (keep(method)) kept[method] = spec.paths[path][method];
    }
    if (Object.keys(kept).length > 0) paths[path] = kept;
  }
  return Object.assign({}, spec, { paths });
}

/**
 * Raw call for the fixtures of setup(), teardown() and the write handlers, outside the coverage
 * count. Tagged `op: fixture` so it stays out of the per-operation latency thresholds, but it
 * still counts in `http_req_failed`. Aborts the handler (or setup) on a non-2xx answer.
 */
function fixture(method, path, body) {
  const res = http.request(
    method,
    `${BASE_URL}${path}`,
    body === undefined ? null : JSON.stringify(body),
    { headers: Object.assign({ 'Content-Type': 'application/json' }, AUTH), tags: { op: 'fixture' } },
  );
  if (res.status < 200 || res.status >= 300) {
    fail(`fixture ${method} ${path} answered ${res.status}: ${res.body}`);
  }
  return res;
}

const spec = loadSpec();

const readHandlers = {
  'GET /health': ({ request }) => check(request(), { 'health 200': (r) => r.status === 200 }),
  'GET /ready': ({ request }) => check(request(), { 'ready 200': (r) => r.status === 200 }),
  'GET /api/v1/masking-patterns': ({ request }) => check(request(), { 'patterns 200': (r) => r.status === 200 }),
  'GET /api/v1/erasures/{userId}': ({ request }) =>
    check(request({ path: { userId: 2 } }), { 'erasure read': (r) => r.status === 200 || r.status === 404 }),
};

const writeHandlers = {
  'POST /api/v1/scans': ({ request }) => check(request(), { 'scan 200': (r) => r.status === 200 }),
  // The seeded account 2: erased once, the next calls only read its finished steps (202).
  'POST /api/v1/erasures/{userId}': ({ request }) =>
    check(request({ path: { userId: 2 } }), { 'erasure 202': (r) => r.status === 202 }),
};

const reads = createCoverage(readHandlers, {
  spec: specSubset(spec, (method) => READ_METHODS.includes(method)),
});
// `createCoverage` throws on a spec without operation: no write operation and no write handler
// means no `writes` scenario. A handler left without its operation still makes k6 abort.
const writeSpec = specSubset(spec, (method) => !READ_METHODS.includes(method));
const writes =
  Object.keys(writeSpec.paths).length > 0 || Object.keys(writeHandlers).length > 0
    ? createCoverage(writeHandlers, { spec: writeSpec })
    : null;

/** One `p(95)` threshold per operation (`op` tag) of `coverage`. */
function latencyThresholds(coverage, budgetMs) {
  const thresholds = {};
  for (const operation of coverage.operations) {
    thresholds[`http_req_duration{op:${operation.op}}`] = [`p(95)<${budgetMs}`];
  }
  return thresholds;
}

export const options = {
  scenarios: {
    reads: {
      executor: 'ramping-vus',
      exec: 'readScenario',
      stages: [
        { duration: '30s', target: 20 }, // Ramp up to 20 virtual users
        { duration: '1m', target: 20 }, // Hold
        { duration: '10s', target: 0 }, // Ramp down
      ],
    },
    ...(writes && {
      writes: {
        executor: 'constant-vus',
        exec: 'writeScenario',
        vus: 2,
        duration: '1m40s',
      },
    }),
  },
  thresholds: {
    ...reads.thresholds, // every operation exercised, no handler error (shared counters)
    ...latencyThresholds(reads, READ_BUDGET_MS),
    ...(writes && latencyThresholds(writes, WRITE_BUDGET_MS)),
    http_req_failed: ['rate<0.01'], // Less than 1% errors
  },
};

/** Read fixtures, passed to every handler as `data` (none in the template). */
export function setup() {
  return {};
}

export function teardown(data) {}

export function readScenario(data) {
  reads.run({ headers: AUTH, data });
  sleep(1);
}

export function writeScenario(data) {
  writes.run({ headers: AUTH, data });
  sleep(1);
}
