#!/bin/bash
current_version="0.1.22"
new_version="0.1.23"
sed -i "s/$current_version/$new_version/g" ./Cargo.toml
sed -i "s/$current_version/$new_version/g" ./io.github.aganzha.Stage.json
sed -i "s/$current_version/$new_version/g" ./io.github.aganzha.Stage.metainfo.xml
sed -i "s/$current_version/$new_version/g" ./rpmbuild.sh
