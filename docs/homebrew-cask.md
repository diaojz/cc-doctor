# Homebrew Cask

CC Doctor can be distributed to macOS users via a dedicated Homebrew tap.

## Tap repository

Create a public repository named `homebrew-cc-doctor` under your GitHub account or org.

Recommended initial structure:

```text
homebrew-cc-doctor/
└── Casks/
    └── cc-doctor.rb
```

## Local bootstrap

Generate the cask for the latest release:

```bash
node scripts/generate-homebrew-cask.mjs cc-doctor v3.14.2 f31d492c2256fac665f7378202ba303866645495f4b947c0286c3330ab870d36 > cc-doctor.rb
```

Then copy it into the tap repo:

```bash
mkdir -p Casks
cp cc-doctor.rb Casks/cc-doctor.rb
```

## GitHub Actions automation

This repository includes `.github/workflows/update-homebrew-tap.yml`.
It listens to release publication events and updates the tap automatically.

Required configuration in the source repository (`diaojz/cc-doctor`):

### Repository variables

- `HOMEBREW_TAP_REPO`: target tap repo, e.g. `diaojz/homebrew-cc-doctor`
- `HOMEBREW_TAP_CASK_TOKEN`: defaults to `cc-doctor`

### Repository secret

- `HOMEBREW_TAP_TOKEN`: GitHub token with write access to the tap repo

Optional repository variable:

- `HOMEBREW_TAP_CASK_PATH`: defaults to `Casks/cc-doctor.rb`

## Expected user commands

```bash
brew tap diaojz/cc-doctor
brew install --cask diaojz/cc-doctor/cc-doctor
brew upgrade --cask diaojz/cc-doctor/cc-doctor
```

## Notes

- The release must include a macOS DMG asset named like `CC-Doctor-vX.Y.Z-macOS.dmg`.
- The release payload must expose a SHA256 digest for that DMG asset.
- `brew upgrade --cask cc-doctor` works after the tap repo receives the new version commit.
