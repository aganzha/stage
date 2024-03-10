#!/bin/bash
flatpak-builder --repo=flatpak_target flatpak_build com.github.aganzha.stage.json --force-clean
