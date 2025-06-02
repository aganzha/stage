#!/bin/bash
current_version="0.1.17"
new_version="0.1.18"
sed -i "s/$current_version/$new_version/g" ./Cargo.toml
sed -i "s/$current_version/$new_version/g" ./io.github.aganzha.Stage.json
sed -i "s/$current_version/$new_version/g" ./io.github.aganzha.Stage.metainfo.xml
