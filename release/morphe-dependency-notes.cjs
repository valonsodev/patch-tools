"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { execFileSync } = require("node:child_process");

const VERSION_CATALOG = "engine/gradle/libs.versions.toml";

const DEPENDABOT_MORPHE_ALIASES = [
  "morphe-patcher",
  "morphe-patches-library",
  "morphe-smali",
  "morphe-baksmali",
];

function parseVersionCatalog(source) {
  const versions = new Map();
  const libraries = new Map();
  let section = null;

  for (const line of source.split(/\r?\n/)) {
    const trimmed = line.replace(/\s+#.*$/, "").trim();

    if (!trimmed) {
      continue;
    }

    const sectionMatch = trimmed.match(/^\[([^\]]+)\]$/);
    if (sectionMatch) {
      section = sectionMatch[1];
      continue;
    }

    if (section === "versions") {
      const versionMatch = trimmed.match(/^([A-Za-z0-9_.-]+)\s*=\s*"([^"]+)"$/);
      if (versionMatch) {
        versions.set(versionMatch[1], versionMatch[2]);
      }
      continue;
    }

    if (section === "libraries") {
      const libraryMatch = trimmed.match(/^([A-Za-z0-9_.-]+)\s*=\s*\{(.+)\}$/);
      if (!libraryMatch) {
        continue;
      }

      const body = libraryMatch[2];
      const moduleMatch = body.match(/\bmodule\s*=\s*"([^"]+)"/);
      const versionRefMatch = body.match(/\bversion\.ref\s*=\s*"([^"]+)"/);
      const versionMatch = body.match(/\bversion\s*=\s*"([^"]+)"/);

      libraries.set(libraryMatch[1], {
        module: moduleMatch?.[1],
        versionRef: versionRefMatch?.[1],
        version: versionMatch?.[1],
      });
    }
  }

  return { libraries, versions };
}

function morpheDependenciesFromSource(source) {
  const { libraries, versions } = parseVersionCatalog(source);

  return DEPENDABOT_MORPHE_ALIASES.map((alias) => {
    const library = libraries.get(alias);
    if (!library) {
      throw new Error(`Missing ${alias} in ${VERSION_CATALOG} [libraries]`);
    }

    const version = library.version ?? versions.get(library.versionRef);
    if (!version) {
      throw new Error(`Missing version for ${alias} in ${VERSION_CATALOG}`);
    }

    return {
      alias,
      module: library.module ?? alias,
      version,
    };
  });
}

function morpheDependencies(catalogPath) {
  return morpheDependenciesFromSource(fs.readFileSync(catalogPath, "utf8"));
}

function versionMap(dependencies) {
  return new Map(
    dependencies.map(({ module, version }) => [module, version]),
  );
}

function dependenciesChanged(currentDependencies, previousDependencies) {
  const current = versionMap(currentDependencies);
  const previous = versionMap(previousDependencies);

  if (current.size !== previous.size) {
    return true;
  }

  for (const [module, version] of current) {
    if (previous.get(module) !== version) {
      return true;
    }
  }

  return false;
}

function fileAtGitRef(cwd, gitRef, filePath) {
  try {
    return execFileSync("git", ["show", `${gitRef}:${filePath}`], {
      cwd,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (error) {
    if (error.status === 128) {
      return null;
    }

    throw error;
  }
}

function dependencyTable(dependencies) {
  const rows = dependencies.map(
    ({ module, version }) => `| \`${module}\` | \`${version}\` |`,
  );

  return [
    "## Morphe Dependencies",
    "",
    "| Library | Version |",
    "| --- | --- |",
    ...rows,
    "",
  ].join("\n");
}

module.exports = {
  analyzeCommits(pluginConfig, context) {
    const lastReleaseGitHead = context.lastRelease?.gitHead;
    if (!lastReleaseGitHead) {
      return undefined;
    }

    const cwd = context.cwd ?? process.cwd();
    const catalogPath = path.join(cwd, VERSION_CATALOG);
    const currentDependencies = morpheDependencies(catalogPath);
    const previousCatalog = fileAtGitRef(cwd, lastReleaseGitHead, VERSION_CATALOG);

    if (previousCatalog === null) {
      return "patch";
    }

    const previousDependencies = morpheDependenciesFromSource(previousCatalog);

    return dependenciesChanged(currentDependencies, previousDependencies)
      ? "patch"
      : undefined;
  },

  generateNotes(pluginConfig, context) {
    const cwd = context.cwd ?? process.cwd();
    const catalogPath = path.join(cwd, VERSION_CATALOG);

    return dependencyTable(morpheDependencies(catalogPath));
  },
};
