#!/bin/bash
flatpak-builder --repo=flatpak_target --gpg-sign=D721B759479BF5233A2FAC54196584E65F8849A1 flatpak_build com.github.aganzha.stage.json --force-clean
flatpak build-update-repo --gpg-sign=D721B759479BF5233A2FAC54196584E65F8849A1 --generate-static-deltas --prune flatpak_target/
