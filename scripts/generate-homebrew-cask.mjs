#!/usr/bin/env node
import process from 'node:process';

function fail(message) {
  console.error(message);
  process.exit(1);
}

const [token = 'cc-doctor', version, sha256, owner = 'diaojz', repo = 'cc-doctor'] = process.argv.slice(2);

if (!version || !sha256) {
  fail('Usage: node scripts/generate-homebrew-cask.mjs <cask-token> <version> <sha256> [owner] [repo]');
}

const normalizedVersion = version.startsWith('v') ? version.slice(1) : version;
const tag = version.startsWith('v') ? version : `v${version}`;
const assetName = `CC-Doctor-${tag}-macOS.dmg`;
const url = `https://github.com/${owner}/${repo}/releases/download/${tag}/${assetName}`;

const cask = `cask \"${token}\" do
  version \"${normalizedVersion}\"
  sha256 \"${sha256}\"

  url \"${url}\",
      verified: \"github.com/${owner}/${repo}/\"
  name \"CC Doctor\"
  desc \"All-in-One Assistant for Claude Code, Codex & Gemini CLI\"
  homepage \"https://github.com/${owner}/${repo}\"

  auto_updates true
  depends_on macos: \">= :monterey\"

  app \"CC Doctor.app\"

  zap trash: [
    \"~/Library/Application Support/com.ccdoctor.app\",
    \"~/Library/Caches/com.ccdoctor.app\",
    \"~/Library/HTTPStorages/com.ccdoctor.app\",
    \"~/Library/Logs/com.ccdoctor.app\",
    \"~/Library/Preferences/com.ccdoctor.app.plist\",
    \"~/Library/Saved Application State/com.ccdoctor.app.savedState\"
  ]
end
`;

process.stdout.write(cask);
