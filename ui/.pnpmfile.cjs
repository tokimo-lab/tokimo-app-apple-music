// Tokimo monorepo dev-mode override.
// When this app is checked out *inside* the main tokimo monorepo
// (so packages/ui, packages/tokimo-package-sdk, packages/tokimo-app-builder
// exist as sibling submodules), rewrite the @tokimo/* git dependencies
// to local file: paths so changes to those packages are picked up
// without bumping a sha. Outside the monorepo this hook is a no-op
// and the github:#sha references in package.json are used as-is.
const fs = require("node:fs");
const path = require("node:path");

function findMonorepoRoot(start) {
  let dir = start;
  while (dir !== path.dirname(dir)) {
    if (
      fs.existsSync(path.join(dir, "packages/tokimo-app-builder/package.json"))
    ) {
      return dir;
    }
    dir = path.dirname(dir);
  }
  return null;
}

const root = findMonorepoRoot(__dirname);
const overrides = root
  ? {
      "@tokimo/ui": `file:${root}/packages/ui`,
      "@tokimo/sdk": `file:${root}/packages/tokimo-package-sdk`,
      "@tokimo/app-builder": `file:${root}/packages/tokimo-app-builder`,
      "@tokimo/viewers": `file:${root}/packages/tokimo-viewers`,
    }
  : null;

if (overrides) {
  console.log(
    `[tokimo .pnpmfile.cjs] monorepo detected at ${root}; overriding @tokimo/* to file: paths`,
  );
}

function rewriteSection(section) {
  if (!section) return;
  for (const [name, spec] of Object.entries(overrides)) {
    if (Object.hasOwn(section, name)) section[name] = spec;
  }
}

module.exports = {
  hooks: {
    readPackage(pkg) {
      if (!overrides) return pkg;
      rewriteSection(pkg.dependencies);
      rewriteSection(pkg.devDependencies);
      rewriteSection(pkg.peerDependencies);
      return pkg;
    },
  },
};
