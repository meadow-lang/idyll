// Run with `node --test` (no dependencies, no VS Code).
const { test } = require("node:test");
const assert = require("node:assert");
const path = require("path");
const { candidates, pick, driverCandidates, pickDriver } = require("../src/resolve");

// Built with `path.join`, like the code under test: these tests pretend to be
// other platforms but run on this one, so the separator is the host's.
const HOME = "/home/u";
const installed = path.join(HOME, ".idyll", "bin", "Idyll");
const installedFront = path.join(HOME, ".idyll", "bin", "idyll-front");
const onPath = path.join("/usr/local/bin", "Idyll");
const onPathFront = path.join("/usr/local/bin", "idyll-front");
const alsoOnPath = path.join("/usr/bin", "Idyll");
const alsoOnPathFront = path.join("/usr/bin", "idyll-front");
const checkout = path.join("/src/idyll", "front", "target", "release", "native", "silo", "Idyll");

/// A fixed environment, so the tests do not depend on the machine.
const env = (extra = {}) => ({ PATH: "/usr/local/bin:/usr/bin", ...extra });
const opts = (extra = {}) => ({ home: HOME, platform: "linux", env: env(), ...extra });

test("a setting is tried alone", () => {
  assert.deepEqual(candidates("/opt/Idyll", opts()), ["/opt/Idyll"]);
});

test("PATH comes first, then the install directory, then a checkout's build", () => {
  const c = candidates("Idyll", opts({ folders: ["/src/idyll"] }));
  assert.deepEqual(c, [onPathFront, onPath, alsoOnPathFront, alsoOnPath, installedFront, installed, checkout]);
  assert.deepEqual(candidates(undefined, opts({ folders: ["/src/idyll"] })), c);
});

test("what install.sh installs, idyll-front, is tried before an Idyll beside it", () => {
  // there an Idyll is likely the driver's idyll, on a system that ignores case
  const chosen = pick("Idyll", { ...opts(), exists: (p) => p === installed || p === installedFront, probe: (p) => p === installedFront });
  assert.equal(chosen.command, installedFront);
  assert.deepEqual(chosen.found, [installedFront, installed]);
});

test("IDYLL_HOME is searched before the default install directory", () => {
  const c = candidates("Idyll", opts({ env: env({ IDYLL_HOME: "/opt/idl" }) }));
  const home = path.join("/opt/idl", "bin", "Idyll");
  assert.ok(c.includes(home), c.join(","));
  assert.ok(c.indexOf(home) < c.indexOf(installed));
});

test("no folder is searched unless one is given", () => {
  // the extension gives the workspace's folders only when it is trusted
  assert.ok(!candidates("Idyll", opts()).some((p) => p.includes(path.join("front", "target"))));
});

test("windows looks for Idyll.exe and idyll-front.exe and splits PATH on ;", () => {
  const c = candidates("Idyll", { home: "C:\\Users\\u", platform: "win32", env: { PATH: "C:\\bin;C:\\other" } });
  assert.ok(c.every((p) => p.endsWith("Idyll.exe") || p.endsWith("idyll-front.exe")), c.join(","));
  assert.ok(c.includes(path.join("C:\\bin", "Idyll.exe")), c.join(","));
  assert.ok(c.includes(path.join("C:\\Users\\u", ".idyll", "bin", "idyll-front.exe")), c.join(","));
});

test("a build without the server is passed over for one with it", () => {
  // on Windows the Rust driver's idyll.exe is an Idyll.exe, without `lsp`
  const chosen = pick("Idyll", { ...opts({ folders: ["/src/idyll"] }), exists: (p) => p === onPath || p === checkout, probe: (p) => p === checkout });
  assert.equal(chosen.command, checkout);
  assert.equal(chosen.lsp, true);
});

test("only a build without the server is still said, so the message can say why", () => {
  const chosen = pick("Idyll", { ...opts(), exists: (p) => p === installed, probe: () => false });
  assert.equal(chosen.command, installed);
  assert.equal(chosen.lsp, false);
  assert.deepEqual(chosen.found, [installed]);
});

test("nothing on disk leaves the command unset", () => {
  const chosen = pick("Idyll", { ...opts(), exists: () => false, probe: () => true });
  assert.equal(chosen.command, undefined);
  assert.deepEqual(chosen.found, []);
});

test("a setting that does not exist is not quietly replaced", () => {
  const chosen = pick("/opt/Idyll", { ...opts(), exists: (p) => p === installed, probe: () => true });
  assert.equal(chosen.command, undefined);
});

test("a candidate is asked only once it is known to exist", () => {
  const probed = [];
  pick("Idyll", { ...opts(), exists: (p) => p === installed, probe: (p) => (probed.push(p), true) });
  assert.deepEqual(probed, [installed]);
});

// The driver, which runs a test: `idyll`, not front/'s `Idyll`.
const driverOnPath = path.join("/usr/local/bin", "idyll");
const driverInstalled = path.join(HOME, ".idyll", "bin", "idyll");

test("the driver is looked for where the server is, by its own name", () => {
  assert.deepEqual(driverCandidates("idyll", opts()), [driverOnPath, path.join("/usr/bin", "idyll"), driverInstalled]);
});

test("a driver without `test --exact` is passed over", () => {
  // (on Windows front/'s Idyll.exe answers to idyll.exe too, and has no `test`)
  const chosen = pickDriver("idyll", { ...opts(), exists: (p) => p === driverOnPath || p === driverInstalled, probe: (p) => p === driverInstalled });
  assert.equal(chosen.command, driverInstalled);
  assert.equal(chosen.tests, true);
});
