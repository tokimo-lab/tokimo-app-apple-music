// Tokimo monorepo dev-mode override.
// When this app is checked out *inside* the main tokimo monorepo
// (so packages/ui, packages/tokimo-package-sdk, packages/tokimo-app-builder
// exist as sibling submodules), rewrite the @tokimo/* git dependencies
// to local file: paths so changes to those packages are picked up
// without bumping a sha. Outside the monorepo this hook rewrites any
// remaining workspace:* specs in @tokimo/* transitive deps to fixed github:
// refs so standalone pnpm install works without a workspace.
const fs = require("node:fs");
const path = require("node:path");

function findMonorepoRoot(start) {
  let dir = start;
  while (dir !== path.dirname(dir)) {
    if (fs.existsSync(path.join(dir, "packages/tokimo-app-builder/package.json"))) {
      return dir;
    }
    dir = path.dirname(dir);
  }
  return null;
}

const root = findMonorepoRoot(__dirname);

// file: overrides used when inside the main monorepo
const fileOverrides = root
  ? {
      "@tokimo/ui": `file:${root}/packages/ui`,
      "@tokimo/sdk": `file:${root}/packages/tokimo-package-sdk`,
      "@tokimo/app-builder": `file:${root}/packages/tokimo-app-builder`,
      "@tokimo/viewers": `file:${root}/packages/tokimo-viewers`,
    }
  : null;

// Fixed github: refs used as fallback for workspace:* in standalone mode
const githubRefs = {
  "@tokimo/ui": "github:tokimo-lab/tokimo-ui#67925b8147d21f7d5ac3db50a3601400b144b89d",
  "@tokimo/sdk": "github:tokimo-lab/tokimo-package-sdk#2632b1b675b012735d54f85fee00b71b7f27e0c4",
  "@tokimo/app-builder": "github:tokimo-lab/tokimo-app-builder#c50ec9f03da12154b98ada27fadd2e87215ab351",
  "@tokimo/viewers": "github:tokimo-lab/tokimo-viewers#97f4742d3e21ca012403cc5849d7d643c52d9abe",
};

if (fileOverrides) {
  console.log(`[tokimo .pnpmfile.cjs] monorepo detected at ${root}; overriding @tokimo/* to file: paths`);
}

function rewriteSection(section) {
  if (!section) return;
  for (const [name, spec] of Object.entries(section)) {
    if (fileOverrides && Object.hasOwn(fileOverrides, name)) {
      section[name] = fileOverrides[name];
    } else if (!fileOverrides && spec === "workspace:*" && Object.hasOwn(githubRefs, name)) {
      // standalone mode: fix transitive workspace:* refs that would fail outside a workspace
      section[name] = githubRefs[name];
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
