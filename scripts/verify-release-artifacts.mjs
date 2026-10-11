import { access } from "node:fs/promises";
import path from "node:path";

const artifactDirectory = path.resolve(process.argv[2] ?? "artifacts");
const expectedArtifacts = [
  "sideband-tunes.win32-x64-msvc.node",
  "sideband-tunes.win32-arm64-msvc.node",
  "sideband-tunes.linux-x64-gnu.node",
  "sideband-tunes.linux-arm64-gnu.node",
  "sideband-tunes.darwin-x64.node",
  "sideband-tunes.darwin-arm64.node",
];

const missingArtifacts = [];

for (const artifact of expectedArtifacts) {
  try {
    await access(path.join(artifactDirectory, artifact));
  } catch {
    missingArtifacts.push(artifact);
  }
}

if (missingArtifacts.length > 0) {
  console.error("Release is missing required native artifacts:");
  for (const artifact of missingArtifacts) {
    console.error(`- ${artifact}`);
  }
  process.exitCode = 1;
} else {
  console.log(`Verified ${expectedArtifacts.length} native release artifacts.`);
}
