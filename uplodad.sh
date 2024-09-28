#/bin/bash
# aws s3 cp stage.flatpakref s3://www.aganzha.online/
aws s3 cp --recursive flatpak_target s3://www.aganzha.online/stage_flatpak_repo/
