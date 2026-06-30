// Tokimo standalone app dependency policy.
// Default mode keeps package.json Git revisions so committed pnpm-lock.yaml is
// portable outside the monorepo. Set TOKIMO_LINK_LOCAL_PACKAGES=1 only for
// local package development; never commit a lock generated in that mode.
const fs = require("node:fs");
const path = require("node:path");

const LOCAL_PACKAGE_PATHS = {
  "@tokimo/app-builder": "packages/tokimo-app-builder",
  "@tokimo/sdk": "packages/tokimo-package-sdk",
  "@tokimo/terminal": "packages/tokimo-package-terminal",
  "@tokimo/ui": "packages/ui",
  "@tokimo/viewers": "packages/tokimo-viewers",
};

function findMonorepoRoot(start) {
  let dir = start;
  while (dir !== path.dirname(dir)) {
    if (
      fs.existsSync(path.join(dir, "feeds.toml")) &&
      fs.existsSync(path.join(dir, "packages"))
    ) {
      return dir;
    }
    dir = path.dirname(dir);
  }
  return null;
}

function rel(target) {
  return path.relative(__dirname, target).split(path.sep).join("/");
}

function collectDirectTokimoSpecs() {
  const pkg = JSON.parse(
    fs.readFileSync(path.join(__dirname, "package.json"), "utf8"),
  );
  const refs = {};
  for (const sectionName of [
    "dependencies",
    "devDependencies",
    "peerDependencies",
  ]) {
    const section = pkg[sectionName];
    if (!section) continue;
    for (const [name, spec] of Object.entries(section)) {
      if (name.startsWith("@tokimo/") && typeof spec === "string") {
        refs[name] = spec;
      }
    }
  }
  return refs;
}

const directTokimoSpecs = collectDirectTokimoSpecs();
const root =
  process.env.TOKIMO_LINK_LOCAL_PACKAGES === "1"
    ? findMonorepoRoot(__dirname)
    : null;

const localOverrides = {};
if (root) {
  for (const [name, packagePath] of Object.entries(LOCAL_PACKAGE_PATHS)) {
    if (!Object.hasOwn(directTokimoSpecs, name)) continue;
    const target = path.join(root, packagePath);
    if (fs.existsSync(path.join(target, "package.json"))) {
      localOverrides[name] = `link:${rel(target)}`;
    }
  }
}

if (root && Object.keys(localOverrides).length > 0) {
  console.log(
    `[tokimo .pnpmfile.cjs] TOKIMO_LINK_LOCAL_PACKAGES=1; overriding @tokimo/* to local links`,
  );
}

function rewriteSection(section) {
  if (!section) return;
  for (const [name, spec] of Object.entries(section)) {
    if (Object.hasOwn(localOverrides, name)) {
      section[name] = localOverrides[name];
    } else if (
      spec === "workspace:*" &&
      Object.hasOwn(directTokimoSpecs, name)
    ) {
      section[name] = directTokimoSpecs[name];
    }
  }
}

module.exports = {
  hooks: {
    readPackage(pkg) {
      rewriteSection(pkg.dependencies);
      rewriteSection(pkg.devDependencies);
      rewriteSection(pkg.peerDependencies);
      return pkg;
    },
  },
};
